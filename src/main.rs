use anyhow::{Context, Result, anyhow, bail};
use clap::{Args, Parser, Subcommand};
use pkgsmith::install;
use pkgsmith::manifest::Manifest;
use pkgsmith::package::{self, Options};
use pkgsmith::plan::{self, Selection};
use pkgsmith::plugins::{self, Registry, Signer};
use pkgsmith::script;
use pkgsmith::signing::Signing;
use pkgsmith::ui::{Mode, Ui};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

#[derive(Parser)]
#[command(
    name = "pkgsmith",
    version,
    about = "Write distribution packages from one script per project",
    long_about = "Write distribution packages from one script per project.\n\n`pkgsmith build [PATH]` runs the script in PATH (pkgsmith.star). The script reads files from a prefix that you made with your own build, and pkgsmith writes the packages to ./dist. pkgsmith does not build your program."
)]
struct Cli {
    /// A folder to search for plugins (pkgsmith-plugin-*, a shared library) before the standard places. Repeatable. See `pkgsmith plugins`.
    #[arg(long, global = true, value_name = "DIR")]
    plugin_path: Vec<PathBuf>,
    #[command(subcommand)]
    command: Command,
}

/// What every command that reads the script takes.
#[derive(Args)]
struct Source {
    /// A folder that holds pkgsmith.star, or the script itself.
    #[arg(default_value = ".")]
    path: PathBuf,
    /// The prefix: a folder with the files to package, laid out like an installed system (usr/bin/..., etc/...). A
    /// script reads it and never changes it. For example the result of `make install DESTDIR=prefix`.
    #[arg(long, default_value = "prefix", value_name = "DIR")]
    prefix: PathBuf,
    /// The script, when it is not PATH/pkgsmith.star.
    #[arg(short = 'f', long, value_name = "FILE")]
    file: Option<PathBuf>,
    /// Formats, comma separated. See `pkgsmith formats`. Default: every format the script declares.
    #[arg(short = 'F', long, value_delimiter = ',', value_name = "LIST")]
    format: Vec<String>,
    /// Only the targets with this value, as KEY=VALUE, for example `--where distro=fedora`. The keys are format, distro,
    /// arch, name and the keys of the tags of a target. Repeatable.
    #[arg(long = "where", value_name = "KEY=VALUE")]
    filter: Vec<String>,
    /// NAME=VALUE for arg("NAME") in the script, on top of the environment. Repeatable.
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
        /// How many packages are written at the same time.
        #[arg(short, long, default_value_t = default_jobs(), value_name = "N")]
        jobs: usize,
        /// Progress output: auto, tty, plain or quiet.
        #[arg(long, default_value = "auto", value_name = "MODE")]
        progress: String,
        /// Print only the paths of the packages, one per line.
        #[arg(short, long)]
        quiet: bool,
        /// Sign the packages. `[FORMAT=][SIGNER:]KEY`: a key file, or for the gpg signer a key id. FORMAT limits the key
        /// to one format and SIGNER names the signer plugin. Repeatable. Also PKGSMITH_SIGN_KEY, a list separated by
        /// `;`. See `pkgsmith plugins` for the signers.
        #[arg(long, value_name = "[FORMAT=][SIGNER:]KEY")]
        sign_key: Vec<String>,
        /// A file that holds the passphrase of the key. Also PKGSMITH_SIGN_PASSPHRASE_FILE.
        #[arg(long, value_name = "FILE")]
        sign_passphrase_file: Option<PathBuf>,
        /// Write detached signatures as ASCII armor in FILE.asc instead of binary in FILE.sig.
        #[arg(long)]
        sign_armor: bool,
    },
    /// List what `build` would do as a table: one row per package, with the file it would write.
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
    },
    /// List the formats the installed plugins write.
    Formats,
    /// List the plugins that are loaded and the folders pkgsmith searches for them. `plugins install` fetches them.
    Plugins {
        #[command(subcommand)]
        action: Option<PluginsAction>,
    },
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

/// The signing setup from the command line, or from `PKGSMITH_SIGN_KEY` and `PKGSMITH_SIGN_PASSPHRASE_FILE`.
fn signing_setup<'a>(
    registry: &'a Registry,
    mut keys: Vec<String>,
    passphrase_file: Option<PathBuf>,
) -> Result<Signing<'a>> {
    if keys.is_empty()
        && let Ok(list) = std::env::var("PKGSMITH_SIGN_KEY")
    {
        keys = list
            .split(';')
            .filter(|s| !s.is_empty())
            .map(String::from)
            .collect();
    }
    let passphrase_file = passphrase_file
        .or_else(|| std::env::var_os("PKGSMITH_SIGN_PASSPHRASE_FILE").map(PathBuf::from));
    Signing::new(registry, &keys, passphrase_file)
}

/// The file name each item would write: the name the plugin picks, or the `file` template of the target.
fn planned_name(registry: &Registry, output: &Path, item: &plan::Item) -> Result<String> {
    let outline = package::outline(
        &item.manifest,
        &item.format,
        &Options::new(output.to_path_buf(), item.job.distro().map(String::from)),
    );
    let name = registry.file_name(&outline)?;
    package::output_name(&item.job, &outline, &name)
}

/// Two targets that write the same file would overwrite each other while they run side by side. Stop before any build.
fn check_unique_names(registry: &Registry, output: &Path, items: &[plan::Item]) -> Result<()> {
    let mut seen: std::collections::HashMap<String, &plan::Item> = std::collections::HashMap::new();
    for item in items {
        let name = planned_name(registry, output, item)?;
        if let Some(first) = seen.insert(name.clone(), item) {
            bail!(
                "{} and {} would both write {name}. Give one of the targets a `file` template, for example `file = \"{{distro}}-{{file}}\"`",
                first.label(),
                item.label()
            );
        }
    }
    Ok(())
}

/// The lane of a package in the progress output: its distro, or the format, and its architecture.
fn lane(manifest: &Manifest) -> String {
    format!("{}/{}", manifest.name, manifest.arch)
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

/// The `--build-arg`s as a map. They win over the environment inside the script.
fn build_args(set: &[String]) -> Result<std::collections::BTreeMap<String, String>> {
    Ok(pairs(set, "--build-arg")?.into_iter().collect())
}

/// The script and the packages it declares.
fn load(source: &Source, registry: &Registry) -> Result<Vec<plan::Item>> {
    let file = if source.path.is_file() {
        source.path.clone()
    } else {
        match &source.file {
            Some(file) => file.clone(),
            None => {
                let file = source.path.join("pkgsmith.star");
                if !file.is_file() {
                    bail!(
                        "no pkgsmith.star in {}; name the script with -f",
                        source.path.display()
                    );
                }
                file
            }
        }
    };
    let base = std::fs::canonicalize(
        file.parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new(".")),
    )?;
    let prefix = std::fs::canonicalize(&source.prefix).with_context(|| {
        format!(
            "cannot read the prefix {}; make it with your build, or name it with --prefix",
            source.prefix.display()
        )
    })?;
    let specs = script::evaluate(&file, build_args(&source.build_arg)?, &prefix)?;
    let selection = Selection {
        formats: source.format.clone(),
        filters: pairs(&source.filter, "--where")?,
    };
    let items = plan::items(specs, &base, &prefix, &registry.format_names(), &selection)?;
    if items.is_empty() {
        bail!("nothing to build: no target of this script matches the selection");
    }
    Ok(items)
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
    file: Result<String, String>,
}

fn plan_table(registry: &Registry, items: &[plan::Item], json: bool) -> Result<()> {
    use std::io::IsTerminal;
    let mut rows = Vec::new();
    for item in items {
        let bound = &item.manifest;
        let outline = package::outline(
            bound,
            &item.format,
            &Options::new(PathBuf::from("."), item.job.distro().map(String::from)),
        );
        let file = registry
            .file_name(&outline)
            .and_then(|name| package::output_name(&item.job, &outline, &name))
            .map_err(|e| format!("{e:#}"));
        let arch = bound.arch.clone();
        rows.push(Row {
            format: item.format.clone(),
            distro: item.job.distro().unwrap_or("").to_string(),
            arch,
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
    println!(
        "{bold}{:>3}  {:<w_format$}  {:<w_distro$}  {:<w_arch$}  FILE{reset}",
        "#", "FORMAT", "DISTRO", "ARCH",
    );
    for (number, row) in rows.iter().enumerate() {
        let file = match &row.file {
            Ok(name) => name.clone(),
            Err(_) => format!("{red}?{reset}"),
        };
        println!(
            "{dim}{:>3}{reset}  {:<w_format$}  {:<w_distro$}  {:<w_arch$}  {file}",
            number + 1,
            row.format,
            row.distro,
            row.arch,
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
    println!("\n{} packages.", rows.len());
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
                    "no plugins found. Plugins are shared libraries named pkgsmith-plugin-*; run `pkgsmith plugins` to see where pkgsmith looks"
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
        Command::Plan { source, json } => {
            let items = load(&source, &registry)?;
            plan_table(&registry, &items, json)?;
        }
        Command::Files {
            source,
            jobs,
            progress,
        } => {
            let items = load(&source, &registry)?;
            let ui = Ui::new(Mode::parse(&progress).map_err(|e| anyhow!(e))?);
            let (listings, errors) = run_all(&items, jobs, |item| {
                let bound = &item.manifest;
                let options = Options::new(PathBuf::from("."), item.job.distro().map(String::from));
                let package = package::assemble(bound, &item.format, &options)?;
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
            sign_key,
            sign_passphrase_file,
            sign_armor,
        } => {
            let items = load(&source, &registry)?;
            check_unique_names(&registry, &output, &items)?;
            let signing = signing_setup(&registry, sign_key, sign_passphrase_file)?;
            let signer: Option<&Signing> = (!signing.is_off()).then_some(&signing);
            if let Some(signing) = signer {
                // Every key is opened and every signer asked before the first build starts.
                let mut seen: Vec<&str> = Vec::new();
                for item in &items {
                    if !seen.contains(&item.format.as_str()) {
                        seen.push(&item.format);
                        signing.require(&item.format)?;
                    }
                }
            }
            let mode = if quiet {
                Mode::Quiet
            } else {
                Mode::parse(&progress).map_err(|e| anyhow!(e))?
            };
            let ui = Ui::new(mode);
            let job_number = AtomicUsize::new(0);
            let (written, errors) = run_all(&items, jobs, |item| {
                let bound = &item.manifest;
                // Each job builds in a private folder inside the output folder. Two jobs can make a file of the same default
                // name, and the `file` template gives them different final names. The destinations were checked to be
                // different before the first build started.
                let private = output.join(format!(
                    ".pkgsmith-{}-{}",
                    std::process::id(),
                    job_number.fetch_add(1, Ordering::Relaxed)
                ));
                let options = Options::new(private.clone(), item.job.distro().map(String::from));
                let package = package::assemble(bound, &item.format, &options)?;
                let step = ui.step(&lane(bound), format!("package {}", item.format));
                let built = registry
                    .build(
                        &package,
                        &|_, message| step.log(message),
                        signer.map(|k| k as &dyn Signer),
                    )
                    .and_then(|file| {
                        // A `file` template of the target renames the result before it is signed.
                        let name = package::output_name(
                            &item.job,
                            &package,
                            &file
                                .file_name()
                                .map(|n| n.to_string_lossy().into_owned())
                                .unwrap_or_default(),
                        )?;
                        let destination = output.join(name);
                        std::fs::rename(&file, &destination).with_context(|| {
                            format!(
                                "cannot move {} to {}",
                                file.display(),
                                destination.display()
                            )
                        })?;
                        let _ = std::fs::remove_dir_all(&private);
                        let file = destination;
                        let signed = match signer {
                            Some(signing) => {
                                signing.sign_detached(&item.format, &file, sign_armor)?
                            }
                            None => "",
                        };
                        Ok((file, signed))
                    });
                if built.is_err() {
                    let _ = std::fs::remove_dir_all(&private);
                }
                match built {
                    Ok((file, signed)) => {
                        step.log(&file.display().to_string());
                        step.done();
                        Ok((item.format.clone(), file, signed))
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
                    .for_each(|(_, file, _)| println!("{}", file.display()));
            } else {
                ui.note(&format!("\nPackages in {}:", output.display()));
                for (format, file, signed) in &written {
                    let bytes = std::fs::metadata(file).map(|m| m.len()).unwrap_or(0);
                    let name = file
                        .file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_default();
                    let signed = if signed.is_empty() {
                        String::new()
                    } else {
                        format!(", {signed}")
                    };
                    ui.note(&format!("  {format:<10} {name}  ({}{signed})", size(bytes)));
                }
            }
        }
    }
    Ok(())
}
