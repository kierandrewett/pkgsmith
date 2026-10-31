use anyhow::{Context, Result, anyhow, bail};
use clap::{Args, Parser, Subcommand};
use pkgsmith::container::Engine;
use pkgsmith::expand::Scope;
use pkgsmith::formats;
use pkgsmith::manifest::Manifest;
use pkgsmith::package::{Options, Package};
use pkgsmith::plan;
use pkgsmith::runner::{Runner, lane};
use pkgsmith::ui::{Mode, Ui};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

#[derive(Parser)]
#[command(
    name = "pkgsmith",
    version,
    about = "Build distribution packages from one manifest per package",
    long_about = "Build distribution packages from one manifest per package.\n\nLike docker build, `pkgsmith build [PATH]` reads the manifest in PATH (pkgsmith.yaml), runs its build in a container, and writes the packages to ./dist."
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

/// What every command that reads a manifest takes.
#[derive(Args)]
struct Source {
    /// The build context: a folder that holds pkgsmith.yaml, or the manifest itself. The folder is mounted at /src
    /// in a tracked build.
    #[arg(default_value = ".")]
    path: PathBuf,
    /// The manifest, when it is not PATH/pkgsmith.yaml.
    #[arg(short = 'f', long, value_name = "FILE")]
    file: Option<PathBuf>,
    /// Formats, comma separated. See `pkgsmith formats`. Default: every format the matrix allows.
    #[arg(short = 'F', long, value_delimiter = ',', value_name = "LIST")]
    format: Vec<String>,
    /// Only the matrix combinations with this value, as KEY=VALUE, for example `--job distro=fedora`. Repeatable.
    #[arg(long = "job", value_name = "KEY=VALUE")]
    job: Vec<String>,
    /// Only these platforms, for example `linux/arm64,linux/amd64`. Selects the matrix `arch`.
    #[arg(long, value_delimiter = ',', value_name = "LIST")]
    platform: Vec<String>,
    /// Distro for `overrides` when the matrix names none: fedora, opensuse, debian, ubuntu, archlinux.
    #[arg(long)]
    distro: Option<String>,
    /// NAME=VALUE for ${NAME} in the manifest, on top of the environment. Repeatable.
    #[arg(long = "build-arg", visible_alias = "set", value_name = "NAME=VALUE")]
    build_arg: Vec<String>,
}

#[derive(Subcommand)]
enum Command {
    /// Build the packages a manifest describes.
    Build {
        #[command(flatten)]
        source: Source,
        /// Folder that receives the packages.
        #[arg(short, long, default_value = "dist")]
        output: PathBuf,
        /// How many builds and packages run at the same time. Builds that need the same image and script run once.
        #[arg(short, long, default_value_t = default_jobs(), value_name = "N")]
        jobs: usize,
        /// Progress output: auto, tty, plain or quiet.
        #[arg(long, default_value = "auto", value_name = "MODE")]
        progress: String,
        /// Print only the paths of the packages, one per line.
        #[arg(short, long)]
        quiet: bool,
        /// Run the setup scripts again instead of reusing the cached images.
        #[arg(long)]
        no_cache: bool,
        /// Folder for the files a tracked build installs.
        #[arg(long, default_value = ".pkgsmith", value_name = "DIR")]
        work: PathBuf,
    },
    /// List what `build` would do, one line per matrix combination and format.
    #[command(alias = "ls")]
    Plan {
        #[command(flatten)]
        source: Source,
    },
    /// Build, then list the files each package would carry, without writing packages.
    Files {
        #[command(flatten)]
        source: Source,
        #[arg(short, long, default_value_t = default_jobs(), value_name = "N")]
        jobs: usize,
        #[arg(long, default_value = "auto", value_name = "MODE")]
        progress: String,
        #[arg(long, default_value = ".pkgsmith", value_name = "DIR")]
        work: PathBuf,
    },
    /// List the supported formats.
    Formats,
    /// Remove the cached setup images.
    Prune,
}

fn default_jobs() -> usize {
    std::thread::available_parallelism()
        .map(|n| n.get().min(8))
        .unwrap_or(1)
}

fn pairs(items: &[String], flag: &str) -> Result<Vec<(String, String)>> {
    items
        .iter()
        .map(|item| {
            let (name, value) = item
                .split_once('=')
                .with_context(|| format!("{flag} {item:?} must be NAME=VALUE"))?;
            Ok((name.to_string(), value.to_string()))
        })
        .collect()
}

fn scope(set: &[String]) -> Result<Scope> {
    let mut scope = Scope::from_environment();
    for (name, value) in pairs(set, "--build-arg")? {
        scope.set(name, value);
    }
    Ok(scope)
}

/// `linux/arm64` -> `arm64`.
fn platform_arch(platform: &str) -> Result<String> {
    match platform.strip_prefix("linux/") {
        Some(arch @ ("amd64" | "arm64")) => Ok(arch.to_string()),
        _ => bail!("platform {platform:?} must be linux/amd64 or linux/arm64"),
    }
}

/// The manifest and what to build from it.
fn load(source: &Source) -> Result<(Manifest, Vec<plan::Item>)> {
    let (file, context) = if source.path.is_file() {
        (
            source.path.clone(),
            source.path.parent().unwrap_or(Path::new(".")).to_path_buf(),
        )
    } else {
        let file = match &source.file {
            Some(file) => file.clone(),
            None => ["pkgsmith.yaml", "pkgsmith.yml"]
                .iter()
                .map(|name| source.path.join(name))
                .find(|path| path.is_file())
                .ok_or_else(|| {
                    anyhow!(
                        "no pkgsmith.yaml in {}; name the manifest with -f",
                        source.path.display()
                    )
                })?,
        };
        (file, source.path.clone())
    };
    let file = source
        .file
        .clone()
        .filter(|_| source.path.is_file())
        .unwrap_or(file);
    let mut manifest = Manifest::parse(&file)?;
    manifest.context = if context.as_os_str().is_empty() {
        PathBuf::from(".")
    } else {
        context
    };
    let arches: Vec<String> = source
        .platform
        .iter()
        .map(|p| platform_arch(p))
        .collect::<Result<_>>()?;
    let mut items = plan::items(
        &manifest,
        &source.format,
        &pairs(&source.job, "--job")?,
        source.distro.as_deref(),
    )?;
    if !arches.is_empty() {
        items.retain(|item| {
            item.job
                .arch()
                .is_none_or(|arch| arches.iter().any(|a| a == arch))
        });
    }
    if items.is_empty() {
        bail!("nothing to build: no matrix combination of this manifest matches the selection");
    }
    Ok((manifest, items))
}

/// Run `work` for every item, `jobs` at a time. The first failure stops new items from starting.
fn run_all<T: Send>(
    items: &[plan::Item],
    jobs: usize,
    work: impl Fn(&plan::Item) -> Result<T> + Sync,
) -> (Vec<T>, Vec<String>) {
    let next = AtomicUsize::new(0);
    let failed = AtomicBool::new(false);
    let results = Mutex::new(Vec::new());
    let errors = Mutex::new(Vec::new());
    std::thread::scope(|scope| {
        for _ in 0..jobs.clamp(1, items.len()) {
            scope.spawn(|| {
                loop {
                    let index = next.fetch_add(1, Ordering::SeqCst);
                    if index >= items.len() || failed.load(Ordering::SeqCst) {
                        return;
                    }
                    match work(&items[index]) {
                        Ok(value) => results.lock().unwrap().push((index, value)),
                        Err(error) => {
                            failed.store(true, Ordering::SeqCst);
                            errors
                                .lock()
                                .unwrap()
                                .push(format!("{}: {error:#}", items[index].label()));
                        }
                    }
                }
            });
        }
    });
    let mut results = results.into_inner().unwrap();
    results.sort_by_key(|(index, _)| *index);
    (
        results.into_iter().map(|(_, value)| value).collect(),
        errors.into_inner().unwrap(),
    )
}

fn size(bytes: u64) -> String {
    match bytes {
        b if b < 1024 => format!("{b} B"),
        b if b < 1 << 20 => format!("{:.1} KiB", b as f64 / 1024.0),
        b => format!("{:.1} MiB", b as f64 / (1 << 20) as f64),
    }
}

fn main() {
    if let Err(error) = run() {
        eprintln!("ERROR: {error:#}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    match Cli::parse().command {
        Command::Formats => formats::ALL.iter().for_each(|name| println!("{name}")),
        Command::Prune => {
            let removed = Engine::connect()?.prune()?;
            println!("Removed {removed} cached setup images");
        }
        Command::Plan { source } => {
            let (_, items) = load(&source)?;
            items.iter().for_each(|item| println!("{}", item.label()));
        }
        Command::Files {
            source,
            jobs,
            progress,
            work,
        } => {
            let scope = scope(&source.build_arg)?;
            let (manifest, items) = load(&source)?;
            let ui = Ui::new(Mode::parse(&progress).map_err(|e| anyhow!(e))?);
            let runner = Runner::new(work, ui.clone(), true);
            let (listings, errors) = run_all(&items, jobs, |item| {
                let bound = manifest.bind(&scope, &item.job, &item.format)?;
                let options = Options::new(PathBuf::from("."), item.job.distro().map(String::from));
                let tracked = runner.tracked(&bound, &item.job)?;
                let package = Package::with_tracked(&bound, &item.format, &options, tracked)?;
                let mut text = format!("== {}\n", item.label());
                for entry in &package.entries {
                    text += &format!(
                        "{:o} {}:{} {}\n",
                        entry.mode, entry.owner, entry.group, entry.path
                    );
                }
                text += &format!(
                    "{} files, {} bytes installed\n",
                    package.entries.len(),
                    package.installed_size()
                );
                Ok(text)
            });
            ui.finish();
            listings.iter().for_each(|text| print!("{text}"));
            if !errors.is_empty() {
                bail!("{}", errors.join("\n"));
            }
        }
        Command::Build {
            source,
            output,
            jobs,
            progress,
            quiet,
            no_cache,
            work,
        } => {
            let scope = scope(&source.build_arg)?;
            let (manifest, items) = load(&source)?;
            let mode = if quiet {
                Mode::Quiet
            } else {
                Mode::parse(&progress).map_err(|e| anyhow!(e))?
            };
            let ui = Ui::new(mode);
            let runner = Runner::new(work, ui.clone(), !no_cache);
            let (written, errors) = run_all(&items, jobs, |item| {
                let bound = manifest.bind(&scope, &item.job, &item.format)?;
                let options = Options::new(output.clone(), item.job.distro().map(String::from));
                let tracked = runner.tracked(&bound, &item.job)?;
                let package = Package::with_tracked(&bound, &item.format, &options, tracked)?;
                let step = ui.step(&lane(&bound, &item.job), format!("package {}", item.format));
                match formats::build(&package) {
                    Ok(file) => {
                        step.log(&file.display().to_string());
                        step.done();
                        Ok((item.format.clone(), file))
                    }
                    Err(error) => {
                        step.log(&format!("{error:#}"));
                        step.fail();
                        Err(error)
                    }
                }
            });
            ui.finish();
            if !errors.is_empty() {
                bail!("{}", errors.join("\n"));
            }
            if quiet {
                written
                    .iter()
                    .for_each(|(_, file)| println!("{}", file.display()));
            } else {
                ui.note(&format!("\nPackages in {}:", output.display()));
                for (format, file) in &written {
                    let bytes = std::fs::metadata(file).map(|m| m.len()).unwrap_or(0);
                    let name = file
                        .file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_default();
                    ui.note(&format!("  {format:<10} {name}  ({})", size(bytes)));
                }
            }
        }
    }
    Ok(())
}
