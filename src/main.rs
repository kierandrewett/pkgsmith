use anyhow::{Context, Result, anyhow, bail};
use clap::{Args, Parser, Subcommand};
use pkgsmith::container::Engine;
use pkgsmith::expand::Scope;
use pkgsmith::install;
use pkgsmith::manifest::Manifest;
use pkgsmith::package::{self, Options};
use pkgsmith::plan;
use pkgsmith::plugins::{self, Registry};
use pkgsmith::runner::{Runner, build_key, lane};
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
    /// A folder to search for plugins (pkgsmith-plugin-*.so) before the standard places. Repeatable. See `pkgsmith plugins`.
    #[arg(long, global = true, value_name = "DIR")]
    plugin_path: Vec<PathBuf>,
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
    /// List what `build` would do as a table: one row per job, with the file it would write and which jobs share a build.
    #[command(alias = "ls")]
    Plan {
        #[command(flatten)]
        source: Source,
        /// Print the jobs as JSON, for scripts.
        #[arg(long)]
        json: bool,
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
    /// List the formats the installed plugins write.
    Formats,
    /// List the plugins that are loaded and the folders pkgsmith searches for them. `plugins install` fetches them.
    Plugins {
        #[command(subcommand)]
        action: Option<PluginsAction>,
    },
    /// Remove the cached setup images.
    Prune,
}

#[derive(Subcommand)]
enum PluginsAction {
    /// Download the plugins of this version of pkgsmith from its GitHub release and install them.
    ///
    /// `cargo install pkgsmith` and `cargo binstall pkgsmith` install the program only. Run this once afterwards.
    Install {
        /// The release to take the plugins from. Default: the version of this pkgsmith.
        #[arg(long, value_name = "VERSION")]
        version: Option<String>,
        /// The folder to install into. Default: the pkgsmith folder under XDG_DATA_HOME (~/.local/share/pkgsmith/plugins).
        #[arg(long, value_name = "DIR")]
        dir: Option<PathBuf>,
        /// Read this pkgsmith-TARGET.tar.gz instead of downloading one.
        #[arg(long, value_name = "FILE")]
        from: Option<PathBuf>,
        /// Where the assets of a release are, for a mirror or a fork. {version} is replaced by the version.
        #[arg(long, value_name = "URL", default_value = install::DEFAULT_RELEASE_URL)]
        release_url: String,
    },
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
fn load(source: &Source, registry: &Registry) -> Result<(Manifest, Vec<plan::Item>)> {
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
    manifest.check_options(&registry.format_names())?;
    let mut items = plan::items(
        &manifest,
        &registry.format_names(),
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

/// One job as `plan` shows it.
struct Row {
    format: String,
    distro: String,
    arch: String,
    image: String,
    build: Option<String>,
    file: Result<String, String>,
}

/// Letters for the distinct builds, in order of first use: a, b, c, ... aa, ab.
fn build_letter(index: usize) -> String {
    let mut n = index;
    let mut letters = String::new();
    loop {
        letters.insert(0, (b'a' + (n % 26) as u8) as char);
        if n < 26 {
            return letters;
        }
        n = n / 26 - 1;
    }
}

fn plan_table(
    registry: &Registry,
    manifest: &Manifest,
    items: &[plan::Item],
    scope: &Scope,
    json: bool,
) -> Result<()> {
    use std::io::IsTerminal;
    let mut keys: Vec<String> = Vec::new();
    let mut rows = Vec::new();
    for item in items {
        let bound = manifest.bind(scope, &item.job, &item.format);
        let (arch, image, build, file) = match &bound {
            Ok(bound) => {
                let letter = build_key(bound, &item.job).map(|key| {
                    let index = keys.iter().position(|k| *k == key).unwrap_or_else(|| {
                        keys.push(key);
                        keys.len() - 1
                    });
                    build_letter(index)
                });
                (
                    bound.arch.clone(),
                    bound.image.clone().unwrap_or_default(),
                    letter,
                    registry
                        .file_name(&package::outline(
                            bound,
                            &item.format,
                            &Options::new(PathBuf::from("."), item.job.distro().map(String::from)),
                        ))
                        .map_err(|e| format!("{e:#}")),
                )
            }
            Err(error) => (
                item.job.arch().unwrap_or(&manifest.arch).to_string(),
                String::new(),
                None,
                Err(format!("{error:#}")),
            ),
        };
        rows.push(Row {
            format: item.format.clone(),
            distro: item.job.distro().unwrap_or("").to_string(),
            arch,
            image,
            build,
            file,
        });
    }
    if json {
        let list: Vec<serde_json::Value> = items
            .iter()
            .zip(&rows)
            .map(|(item, row)| {
                serde_json::json!({
                    "format": row.format,
                    "distro": row.distro,
                    "arch": row.arch,
                    "image": row.image,
                    "build": row.build,
                    "file": row.file.as_ref().ok(),
                    "error": row.file.as_ref().err(),
                    "values": item.job.values,
                })
            })
            .collect();
        println!("{}", serde_json::to_string_pretty(&list)?);
        return Ok(());
    }
    let tty = std::io::stdout().is_terminal();
    let (bold, dim, reset, red) = if tty {
        ("\x1b[1m", "\x1b[2m", "\x1b[0m", "\x1b[31m")
    } else {
        ("", "", "", "")
    };
    let cell = |rows: &[Row], head: &str, get: &dyn Fn(&Row) -> String| {
        rows.iter()
            .map(|r| get(r).chars().count())
            .chain([head.chars().count()])
            .max()
            .unwrap_or(0)
    };
    let w_format = cell(&rows, "FORMAT", &|r| r.format.clone());
    let w_distro = cell(&rows, "DISTRO", &|r| r.distro.clone());
    let w_arch = cell(&rows, "ARCH", &|r| r.arch.clone());
    let w_image = cell(&rows, "IMAGE", &|r| r.image.clone());
    let has_builds = rows.iter().any(|r| r.build.is_some());
    println!(
        "{bold}{:>3}  {:<w_format$}  {:<w_distro$}  {:<w_arch$}  {:<w_image$}  {}FILE{reset}",
        "#",
        "FORMAT",
        "DISTRO",
        "ARCH",
        "IMAGE",
        if has_builds { "BUILD  " } else { "" },
    );
    for (number, row) in rows.iter().enumerate() {
        let build = if has_builds {
            format!("{:<5}  ", row.build.as_deref().unwrap_or("-"))
        } else {
            String::new()
        };
        let file = match &row.file {
            Ok(name) => name.clone(),
            Err(_) => format!("{red}?{reset}"),
        };
        println!(
            "{dim}{:>3}{reset}  {:<w_format$}  {:<w_distro$}  {:<w_arch$}  {:<w_image$}  {build}{file}",
            number + 1,
            row.format,
            row.distro,
            row.arch,
            row.image,
        );
    }
    let mut problems: Vec<&str> = rows
        .iter()
        .filter_map(|row| {
            row.file
                .as_ref()
                .err()
                .map(|e| e.lines().next().unwrap_or(""))
        })
        .collect();
    problems.dedup();
    let builds = keys.len();
    let shared = if builds > 0 && builds < rows.len() {
        " Jobs with the same BUILD letter share one build."
    } else {
        ""
    };
    println!(
        "\n{} packages{}.{shared}",
        rows.len(),
        if builds > 0 {
            format!(" from {builds} builds")
        } else {
            String::new()
        }
    );
    for problem in problems {
        println!("{red}error:{reset} {problem}");
    }
    Ok(())
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
    let cli = Cli::parse();
    let registry = Registry::discover(&cli.plugin_path);
    if !matches!(cli.command, Command::Plugins { .. }) {
        registry
            .warnings
            .iter()
            .for_each(|warning| eprintln!("warning: {warning}"));
    }
    if registry.format_names().is_empty()
        && matches!(
            cli.command,
            Command::Build { .. } | Command::Plan { .. } | Command::Files { .. }
        )
    {
        bail!(
            "no plugins found, so no package format is available. Run `pkgsmith plugins install` to download them, or `pkgsmith plugins` to see where pkgsmith looks"
        );
    }
    match cli.command {
        Command::Formats => {
            let formats = registry.formats();
            let width = formats.iter().map(|f| f.name.len()).max().unwrap_or(0);
            for format in &formats {
                println!("{:<width$}  {}", format.name, format.description);
            }
            if formats.is_empty() {
                bail!(
                    "no plugins found. Plugins are files named pkgsmith-plugin-*.so; run `pkgsmith plugins` to see where pkgsmith looks"
                );
            }
        }
        Command::Plugins {
            action:
                Some(PluginsAction::Install {
                    version,
                    dir,
                    from,
                    release_url,
                }),
        } => {
            let version = version.unwrap_or_else(|| env!("CARGO_PKG_VERSION").to_string());
            let dir = match dir {
                Some(dir) => dir,
                None => install::default_dir()?,
            };
            let written = install::install(&version, &dir, from.as_deref(), &release_url)?;
            for file in &written {
                println!("{}", file.display());
            }
            println!("Installed {} plugins into {}", written.len(), dir.display());
            let searched = plugins::search_path(&cli.plugin_path);
            if !searched.iter().any(|searched| searched == &dir) {
                println!(
                    "pkgsmith does not look there by default. Use --plugin-path {} or PKGSMITH_PLUGIN_PATH.",
                    dir.display()
                );
            }
        }
        Command::Plugins { action: None } => {
            println!("Search path:");
            for dir in plugins::search_path(&cli.plugin_path) {
                println!("  {}", dir.display());
            }
            println!("\nLoaded:");
            let mut any = false;
            for (info, path) in registry.plugins() {
                any = true;
                let formats: Vec<&str> = info.formats.iter().map(|f| f.name.as_str()).collect();
                println!(
                    "  {} {}  {}  ({})",
                    info.name,
                    info.version,
                    formats.join(", "),
                    path.display()
                );
            }
            if !any {
                println!("  none");
            }
            for warning in &registry.warnings {
                println!("\nwarning: {warning}");
            }
        }
        Command::Prune => {
            let removed = Engine::connect()?.prune()?;
            println!("Removed {removed} cached setup images");
        }
        Command::Plan { source, json } => {
            let scope = scope(&source.build_arg)?;
            let (manifest, items) = load(&source, &registry)?;
            plan_table(&registry, &manifest, &items, &scope, json)?;
        }
        Command::Files {
            source,
            jobs,
            progress,
            work,
        } => {
            let scope = scope(&source.build_arg)?;
            let (manifest, items) = load(&source, &registry)?;
            let ui = Ui::new(Mode::parse(&progress).map_err(|e| anyhow!(e))?);
            let runner = Runner::new(work, ui.clone(), true);
            let (listings, errors) = run_all(&items, jobs, |item| {
                let bound = manifest.bind(&scope, &item.job, &item.format)?;
                let options = Options::new(PathBuf::from("."), item.job.distro().map(String::from));
                let tracked = runner.tracked(&bound, &item.job)?;
                let package = package::assemble(&bound, &item.format, &options, tracked)?;
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
            let (manifest, items) = load(&source, &registry)?;
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
                let package = package::assemble(&bound, &item.format, &options, tracked)?;
                let step = ui.step(&lane(&bound, &item.job), format!("package {}", item.format));
                match registry.build(&package, &|_, message| step.log(message)) {
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
