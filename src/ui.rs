//! Progress output in the style of `docker build`, with one lane per concurrent build.
//!
//! A lane is one pipeline, for example `fedora/amd64`: its setup, its build, its packages. Lanes run side by side.
//! On a terminal the build shows as a block that redraws in place. It has a header with the elapsed time and the number
//! of steps that are done, and then every lane with its steps and their times. A running step shows the last lines of
//! its output under it, so each lane streams on its own. A failed step prints its whole output below the block.
//! Without a terminal (or with `--progress plain`) every line prints once with the lane in front of it, in the way of
//! `docker compose`, so CI logs read well and lanes can be told apart.

use std::collections::VecDeque;
use std::io::{IsTerminal, Write};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// An in-place block when stderr is a terminal, plain lines otherwise.
    Auto,
    Tty,
    Plain,
    /// Nothing, except errors.
    Quiet,
}

impl Mode {
    pub fn parse(text: &str) -> Result<Mode, String> {
        match text {
            "auto" => Ok(Mode::Auto),
            "tty" => Ok(Mode::Tty),
            "plain" => Ok(Mode::Plain),
            "quiet" => Ok(Mode::Quiet),
            other => Err(format!(
                "unknown progress mode {other:?}; use auto, tty, plain or quiet"
            )),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Status {
    Running,
    Done,
    Cached,
    Failed,
}

struct StepState {
    lane: usize,
    name: String,
    started: Instant,
    ended: Option<Instant>,
    status: Status,
    /// The last lines of output, shown while the step runs.
    tail: VecDeque<String>,
    /// All output, printed when the step fails.
    log: Vec<String>,
}

struct State {
    mode: Mode,
    started: Instant,
    lanes: Vec<String>,
    steps: Vec<StepState>,
    /// Lines the last redraw printed, so the next one can move back over them.
    drawn: usize,
    frame: usize,
    closed: bool,
}

/// How many lines of output a running step shows.
const TAIL: usize = 5;

/// How many lines of a failed step's output the final report shows.
const FAILURE_LINES: usize = 40;

fn seconds(duration: Duration) -> String {
    format!("{:.1}s", duration.as_secs_f64())
}

fn width() -> usize {
    terminal_size::terminal_size()
        .map(|(w, _)| w.0 as usize)
        .unwrap_or(100)
}

fn clip(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    let cut: String = text.chars().take(max.saturating_sub(1)).collect();
    format!("{cut}…")
}

const BOLD: &str = "\x1b[1m";
const DIM: &str = "\x1b[2m";
const BLUE: &str = "\x1b[34m";
const GREEN: &str = "\x1b[32m";
const RED: &str = "\x1b[31m";
const RESET: &str = "\x1b[0m";
/// Lane colours, in the order lanes appear.
const LANE_COLOURS: [&str; 6] = [
    "\x1b[36m", "\x1b[35m", "\x1b[33m", "\x1b[32m", "\x1b[34m", "\x1b[31m",
];

impl State {
    fn colour(&self, lane: usize) -> &'static str {
        LANE_COLOURS[lane % LANE_COLOURS.len()]
    }

    fn count(&self, wanted: impl Fn(Status) -> bool) -> usize {
        self.steps.iter().filter(|s| wanted(s.status)).count()
    }

    fn header(&self, finished: bool) -> String {
        let done = self.count(|s| s != Status::Running);
        let running = self.count(|s| s == Status::Running);
        let failed = self.count(|s| s == Status::Failed) > 0;
        let state = if failed {
            format!("{RED}FAILED{RESET}")
        } else if finished {
            format!("{GREEN}FINISHED{RESET}")
        } else {
            format!("{DIM}{running} running{RESET}")
        };
        format!(
            "{BOLD}[+] Building {}{RESET} ({done}/{}) {state}",
            seconds(self.started.elapsed()),
            self.steps.len()
        )
    }

    /// The block as lines of text: a header, then every lane with its steps.
    fn block(&self, finished: bool) -> Vec<String> {
        let columns = width();
        let spinner = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"][self.frame % 10];
        let mut lines = vec![self.header(finished)];
        for (lane, name) in self.lanes.iter().enumerate() {
            let colour = self.colour(lane);
            lines.push(format!(" {colour}{BOLD}{name}{RESET}"));
            for step in self.steps.iter().filter(|s| s.lane == lane) {
                let (mark, note) = match step.status {
                    Status::Running => (
                        format!("{BLUE}{spinner}{RESET}"),
                        seconds(step.started.elapsed()),
                    ),
                    Status::Done => (
                        format!("{GREEN}✔{RESET}"),
                        seconds(step.ended.unwrap_or(step.started) - step.started),
                    ),
                    Status::Cached => (format!("{BLUE}✔{RESET}"), format!("{BLUE}CACHED{RESET}")),
                    Status::Failed => (
                        format!("{RED}✘{RESET}"),
                        seconds(step.ended.unwrap_or(step.started) - step.started),
                    ),
                };
                let room = columns.saturating_sub(18);
                let title = clip(&step.name, room);
                let pad = room.saturating_sub(title.chars().count());
                lines.push(format!(
                    "   {mark} {title}{}{DIM}{note}{RESET}",
                    " ".repeat(pad + 1)
                ));
                if step.status == Status::Running {
                    for line in &step.tail {
                        lines.push(format!(
                            "     {colour}│{RESET} {DIM}{}{RESET}",
                            clip(line, columns.saturating_sub(8))
                        ));
                    }
                }
            }
        }
        lines
    }

    fn redraw(&mut self, finished: bool) {
        if self.mode != Mode::Tty {
            return;
        }
        let mut err = std::io::stderr().lock();
        if self.drawn > 0 {
            let _ = write!(err, "\x1b[{}A\x1b[J", self.drawn);
        }
        let lines = self.block(finished);
        for line in &lines {
            let _ = writeln!(err, "{line}");
        }
        let _ = err.flush();
        self.drawn = lines.len();
    }

    /// One line of plain output, with the lane in front of it.
    fn plain(&self, lane: usize, text: &str) {
        let name = &self.lanes[lane];
        let room = self
            .lanes
            .iter()
            .map(|n| n.chars().count())
            .max()
            .unwrap_or(0);
        eprintln!("{}{name:<room$}{RESET} │ {text}", self.colour(lane));
    }
}

#[derive(Clone)]
pub struct Ui {
    state: Arc<Mutex<State>>,
    ticker: Arc<Mutex<Option<std::thread::JoinHandle<()>>>>,
}

pub struct Step {
    ui: Ui,
    index: usize,
}

impl Ui {
    pub fn new(mode: Mode) -> Ui {
        let mode = match mode {
            Mode::Auto if std::io::stderr().is_terminal() => Mode::Tty,
            Mode::Auto => Mode::Plain,
            other => other,
        };
        let state = Arc::new(Mutex::new(State {
            mode,
            started: Instant::now(),
            lanes: Vec::new(),
            steps: Vec::new(),
            drawn: 0,
            frame: 0,
            closed: false,
        }));
        let ui = Ui {
            state: state.clone(),
            ticker: Arc::new(Mutex::new(None)),
        };
        if mode == Mode::Tty {
            let handle = std::thread::spawn(move || {
                loop {
                    std::thread::sleep(Duration::from_millis(120));
                    let mut state = state.lock().unwrap();
                    if state.closed {
                        return;
                    }
                    state.frame += 1;
                    state.redraw(false);
                }
            });
            *ui.ticker.lock().unwrap() = Some(handle);
        }
        ui
    }

    /// Begin a step in a lane. A lane is created the first time its name is used.
    pub fn step(&self, lane: &str, name: impl Into<String>) -> Step {
        let name = name.into();
        let mut state = self.state.lock().unwrap();
        let lane_index = match state.lanes.iter().position(|l| l == lane) {
            Some(index) => index,
            None => {
                state.lanes.push(lane.to_string());
                state.lanes.len() - 1
            }
        };
        let index = state.steps.len();
        state.steps.push(StepState {
            lane: lane_index,
            name: name.clone(),
            started: Instant::now(),
            ended: None,
            status: Status::Running,
            tail: VecDeque::new(),
            log: Vec::new(),
        });
        if state.mode == Mode::Plain {
            state.plain(lane_index, &format!("{BOLD}▶ {name}{RESET}"));
        }
        state.redraw(false);
        Step {
            ui: self.clone(),
            index,
        }
    }

    /// Print the final block, and for a failed build the output of the steps that failed. Call once, at the end.
    pub fn finish(&self) {
        let mut state = self.state.lock().unwrap();
        state.closed = true;
        if state.mode == Mode::Tty {
            state.redraw(true);
        }
        if state.mode != Mode::Quiet {
            for step in state.steps.iter().filter(|s| s.status == Status::Failed) {
                eprintln!(
                    "\n{RED}------{RESET}\n {RED}>{RESET} {} {}:",
                    state.lanes[step.lane], step.name
                );
                let skipped = step.log.len().saturating_sub(FAILURE_LINES);
                if skipped > 0 {
                    eprintln!("   {DIM}… {skipped} earlier lines{RESET}");
                }
                for line in &step.log[skipped..] {
                    eprintln!("   {line}");
                }
                eprintln!("{RED}------{RESET}");
            }
        }
        drop(state);
        if let Some(handle) = self.ticker.lock().unwrap().take() {
            let _ = handle.join();
        }
    }

    /// A line under the progress block, for results such as the files that were written.
    pub fn note(&self, text: &str) {
        let state = self.state.lock().unwrap();
        if state.mode != Mode::Quiet {
            eprintln!("{text}");
        }
    }
}

impl Step {
    /// A line of output of this step.
    pub fn log(&self, line: &str) {
        let line = line
            .rsplit('\r')
            .find(|part| !part.trim().is_empty())
            .unwrap_or("")
            .trim_end()
            .to_string();
        if line.is_empty() {
            return;
        }
        let mut state = self.ui.state.lock().unwrap();
        let lane = state.steps[self.index].lane;
        if state.mode == Mode::Plain {
            state.plain(lane, &format!("{DIM}{line}{RESET}"));
        }
        let step = &mut state.steps[self.index];
        step.log.push(line.clone());
        step.tail.push_back(line);
        while step.tail.len() > TAIL {
            step.tail.pop_front();
        }
    }

    fn end(&self, status: Status) {
        let mut state = self.ui.state.lock().unwrap();
        let step = &mut state.steps[self.index];
        step.status = status;
        step.ended = Some(Instant::now());
        let took = step.ended.unwrap() - step.started;
        let lane = step.lane;
        let title = step.name.clone();
        if state.mode == Mode::Plain {
            let text = match status {
                Status::Cached => format!("{BLUE}✔ {title} (cached){RESET}"),
                Status::Failed => format!("{RED}✘ {title} ({}){RESET}", seconds(took)),
                _ => format!("{GREEN}✔ {title} ({}){RESET}", seconds(took)),
            };
            state.plain(lane, &text);
        }
        state.redraw(false);
    }

    pub fn done(self) {
        self.end(Status::Done);
    }

    pub fn cached(self) {
        self.end(Status::Cached);
    }

    pub fn fail(self) {
        self.end(Status::Failed);
    }
}

/// Splits chunks of output into lines for a step.
pub struct LineSplitter<'a> {
    step: &'a Step,
    partial: String,
}

impl<'a> LineSplitter<'a> {
    pub fn new(step: &'a Step) -> LineSplitter<'a> {
        LineSplitter {
            step,
            partial: String::new(),
        }
    }

    pub fn push(&mut self, bytes: &[u8]) {
        self.partial.push_str(&String::from_utf8_lossy(bytes));
        while let Some(end) = self.partial.find('\n') {
            let line: String = self.partial.drain(..=end).collect();
            self.step.log(line.trim_end_matches('\n'));
        }
    }

    pub fn flush(&mut self) {
        if !self.partial.is_empty() {
            let line = std::mem::take(&mut self.partial);
            self.step.log(&line);
        }
    }
}
