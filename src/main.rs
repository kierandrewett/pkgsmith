use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use pkgsmith::expand::Scope;
use pkgsmith::formats;
use pkgsmith::manifest::Manifest;
use pkgsmith::package::{Options, Package};
use pkgsmith::plan;
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    name = "pkgsmith",
    version,
    about = "Build distribution packages from one manifest per package"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(clap::Args)]
struct Selection {
    /// The manifest (YAML).
    manifest: PathBuf,
    /// Formats, comma separated. See `pkgsmith formats`. Default: every format the matrix allows.
    #[arg(short, long, value_delimiter = ',')]
    format: Vec<String>,
    /// Only the matrix combinations with this value, as KEY=VALUE, for example `--job arch=arm64`. Repeatable.
    #[arg(short, long = "job", value_name = "KEY=VALUE")]
    job: Vec<String>,
    /// Distro for `overrides` when the matrix names none: fedora, opensuse, debian, ubuntu, archlinux.
    #[arg(long)]
    distro: Option<String>,
    /// NAME=VALUE for ${NAME} in the manifest, on top of the environment. Repeatable.
    #[arg(long = "set", value_name = "NAME=VALUE")]
    set: Vec<String>,
}

#[derive(Subcommand)]
enum Command {
    /// Build every package the manifest and the selection describe.
    Build {
        #[command(flatten)]
        selection: Selection,
        /// Folder that receives the packages.
        #[arg(short, long, default_value = ".")]
        out: PathBuf,
    },
    /// List what `build` would do, one line per matrix combination and format.
    Plan {
        #[command(flatten)]
        selection: Selection,
    },
    /// List the files one build would package.
    Check {
        #[command(flatten)]
        selection: Selection,
    },
    /// List the supported formats.
    Formats,
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
    for (name, value) in pairs(set, "--set")? {
        scope.set(name, value);
    }
    Ok(scope)
}

fn items(selection: &Selection) -> Result<(Manifest, Vec<plan::Item>)> {
    let manifest = Manifest::parse(&selection.manifest)?;
    let items = plan::items(
        &manifest,
        &selection.format,
        &pairs(&selection.job, "--job")?,
        selection.distro.as_deref(),
    )?;
    Ok((manifest, items))
}

fn main() -> Result<()> {
    match Cli::parse().command {
        Command::Formats => formats::ALL.iter().for_each(|name| println!("{name}")),
        Command::Plan { selection } => {
            let (_, items) = items(&selection)?;
            items.iter().for_each(|item| println!("{}", item.label()));
        }
        Command::Check { selection } => {
            let scope = scope(&selection.set)?;
            let (manifest, items) = items(&selection)?;
            for item in &items {
                let bound = manifest.bind(&scope, &item.job, &item.format)?;
                let options = Options::new(PathBuf::from("."), item.job.distro().map(String::from));
                let package = Package::new(&bound, &item.format, &options)?;
                println!("== {}", item.label());
                for entry in &package.entries {
                    println!(
                        "{:o} {}:{} {}",
                        entry.mode, entry.owner, entry.group, entry.path
                    );
                }
                println!(
                    "{} files, {} bytes installed",
                    package.entries.len(),
                    package.installed_size()
                );
            }
        }
        Command::Build { selection, out } => {
            let scope = scope(&selection.set)?;
            let (manifest, items) = items(&selection)?;
            for item in &items {
                let bound = manifest.bind(&scope, &item.job, &item.format)?;
                let options = Options::new(out.clone(), item.job.distro().map(String::from));
                let package = Package::new(&bound, &item.format, &options)?;
                let file = formats::build(&package)
                    .with_context(|| format!("building {}", item.label()))?;
                println!("{}: {}", item.label(), file.display());
            }
        }
    }
    Ok(())
}
