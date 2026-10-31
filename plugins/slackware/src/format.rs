//! Slackware packages (`.txz`): a tar.xz of the files, plus `install/slack-desc` and `install/doinst.sh`.
//!
//! `installpkg` unpacks the archive at `/` and runs `install/doinst.sh`. `slack-desc` is the text that the package
//! browser shows. There are no pre-install or removal scripts in the format. Configuration files are shipped as
//! `NAME.new`, and a `config` function in `doinst.sh` renames them when no file of that name exists yet, so an upgrade
//! never overwrites the file of the administrator.
//! Reference: <https://docs.slackware.com/slackbook:package_management> and `makepkg(8)`.

use anyhow::{Result, bail};
use pkgsmith_plugin::archive::{self, Compression, Leading};
use pkgsmith_plugin::entry::{self, Data};
use pkgsmith_plugin::version::Version;
use pkgsmith_plugin::{Entry, Host, Package};
use serde::Deserialize;
use std::path::PathBuf;

/// The `slackware:` section of the manifest.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct Options {
    /// The build tag at the end of the file name, for example `1_example`. Default: the `release`.
    tag: Option<String>,
    /// `xz` (default), `gzip`, `zstd` or `bzip2`. The extension is `.txz`, `.tgz`, `.tzst` or `.tbz`.
    compression: Option<String>,
}

fn architecture(arch: &str) -> &'static str {
    match arch {
        "amd64" => "x86_64",
        "arm64" => "aarch64",
        _ => "noarch",
    }
}

/// Slackware splits `name-version-arch-build` at the last three hyphens, so the version cannot have a hyphen.
fn version(package: &Package) -> Result<String> {
    Ok(Version::parse(&package.version)?.arch())
}

fn compression(options: &Options) -> Result<(Compression, &'static str)> {
    Ok(match options.compression.as_deref().unwrap_or("xz") {
        "xz" => (Compression::Xz, "txz"),
        "gzip" => (Compression::Gzip, "tgz"),
        "zstd" => (Compression::Zstd, "tzst"),
        "bzip2" => (Compression::Bzip2, "tbz"),
        other => bail!("compression {other:?} must be xz, gzip, zstd or bzip2"),
    })
}

/// `name-version-arch-build.txz`.
pub fn file_name(package: &Package) -> Result<String> {
    let options: Options = package.options()?;
    let (_, extension) = compression(&options)?;
    let build = options
        .tag
        .clone()
        .unwrap_or_else(|| package.release.clone());
    if build.contains('-') || build.is_empty() {
        bail!("the build tag {build:?} must not be empty or hold a hyphen");
    }
    Ok(format!(
        "{}-{}-{}-{build}.{extension}",
        package.name,
        version(package)?,
        architecture(&package.arch)
    ))
}

/// `slack-desc`: eleven lines that start with `NAME:`, the first line a summary and the rest the long text, each
/// at most 69 characters after the prefix.
fn slack_desc(package: &Package) -> String {
    let prefix = format!("{}:", package.name);
    let width = 69usize;
    let mut lines: Vec<String> = Vec::new();
    lines.push(format!("{prefix} {} ({})", package.name, package.summary()));
    lines.push(prefix.clone());
    let body: Vec<&str> = package.description.lines().skip(1).collect();
    for paragraph in body.split(|l| l.trim().is_empty()) {
        let text = paragraph
            .iter()
            .map(|l| l.trim())
            .collect::<Vec<_>>()
            .join(" ");
        if text.is_empty() {
            continue;
        }
        let mut line = String::new();
        for word in text.split_whitespace() {
            if !line.is_empty() && line.len() + 1 + word.len() > width {
                lines.push(format!("{prefix} {line}"));
                line.clear();
            }
            if !line.is_empty() {
                line.push(' ');
            }
            line.push_str(word);
        }
        if !line.is_empty() {
            lines.push(format!("{prefix} {line}"));
        }
        lines.push(prefix.clone());
    }
    if let Some(url) = &package.homepage {
        lines.push(format!("{prefix} {url}"));
    }
    lines.truncate(11);
    while lines.len() < 11 {
        lines.push(prefix.clone());
    }
    let mut text = String::from(
        "# HOW TO EDIT THIS FILE:\n# The \"handy ruler\" below makes it easier to edit a package description.\n",
    );
    text.push_str(&format!(
        "{}|-----handy-ruler------------------------------------------------------|\n",
        " ".repeat(prefix.len())
    ));
    for line in lines {
        text.push_str(&line);
        text.push('\n');
    }
    text
}

/// The function of `makepkg` that keeps the file of the administrator.
const CONFIG_FUNCTION: &str = r#"config() {
  NEW="$1"
  OLD="$(dirname "$NEW")/$(basename "$NEW" .new)"
  # If there is no file by that name, move the new one over.
  if [ ! -r "$OLD" ]; then
    mv "$NEW" "$OLD"
  elif [ "$(cat "$OLD" | md5sum)" = "$(cat "$NEW" | md5sum)" ]; then
    # Throw away the redundant copy.
    rm "$NEW"
  fi
  # Otherwise leave the .new file for the administrator to consider.
}
"#;

/// A word for `sh`: single quotes, with a quote inside written as `'\\''`.
fn shell_quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', "'\\''"))
}

pub fn build(package: &Package, host: &Host) -> Result<PathBuf> {
    let options: Options = package.options()?;
    let (compression, _) = compression(&options)?;
    let name = file_name(package)?;
    if package.scripts.pre_install.is_some()
        || package.scripts.pre_remove.is_some()
        || package.scripts.post_remove.is_some()
    {
        host.warn("Slackware packages have only a post-install script, so pre_install, pre_remove and post_remove are ignored");
    }
    if !package.depends.is_empty() {
        host.warn("Slackware packages do not record dependencies, so depends is ignored");
    }

    // Configuration files are shipped as NAME.new.
    let mut config_paths = Vec::new();
    let entries: Vec<Entry> = package
        .entries
        .iter()
        .map(|e| {
            let mut e = e.clone();
            if e.config && matches!(e.data, Data::File { .. }) {
                config_paths.push(e.path.trim_start_matches('/').to_string());
                e.path.push_str(".new");
            }
            e
        })
        .collect();

    let mut doinst = String::new();
    if !config_paths.is_empty() {
        doinst.push_str(CONFIG_FUNCTION);
        for path in &config_paths {
            doinst.push_str(&format!("config {}\n", shell_quote(&format!("{path}.new"))));
        }
    }
    if let Some(script) = &package.scripts.post_install {
        // `installpkg` runs the script inside its own shell, so drop the `#!` line and the `set -e` that pkgsmith adds. A
        // failing command must not stop the installer.
        let body: Vec<&str> = script
            .lines()
            .filter(|l| !l.starts_with("#!") && l.trim() != "set -e")
            .collect();
        doinst.push_str(&body.join("\n"));
        doinst.push('\n');
    }

    let mut leading = vec![Leading {
        name: "install/slack-desc".into(),
        data: slack_desc(package).into_bytes(),
        mode: 0o644,
    }];
    if !doinst.is_empty() {
        leading.push(Leading {
            name: "install/doinst.sh".into(),
            data: doinst.into_bytes(),
            mode: 0o755,
        });
    }
    let tar = archive::tar(&entry::with_parents(&entries), "", package.mtime, &leading)?;
    let file = package.output(&name);
    std::fs::write(&file, compression.compress(&tar)?)?;
    Ok(file)
}
