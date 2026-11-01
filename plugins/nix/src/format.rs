//! A Nix package: an archive with a `default.nix` and the files it installs.
//!
//! The archive unpacks to `NAME-VERSION/`, which holds `default.nix` and `files/`. `nix-build` of that folder gives a
//! store path with the files below it. The derivation does not build anything: it copies `files/` into `$out`, so the
//! files must be built already, as for the other formats.
//!
//! Specifications used:
//! - `stdenv.mkDerivation` and its phases: https://nixos.org/manual/nixpkgs/stable/#chap-stdenv
//! - `autoPatchelfHook`: https://nixos.org/manual/nixpkgs/stable/#setup-hook-autopatchelfhook
//! - The `meta` attributes (`description`, `homepage`, `license`, `platforms`):
//!   https://nixos.org/manual/nixpkgs/stable/#chap-meta
//! - String literals and their escapes: https://nix.dev/manual/nix/stable/language/string-literals.html
//!
//! Entries below `prefix` (default `/usr`) lose it: `/usr/bin/hi` is `$out/bin/hi`. An entry outside the prefix, such as
//! `/etc/hi.conf`, goes to `$out/etc/hi.conf` and warns, because a Nix package cannot write outside its store path.
//! Nix keeps only the executable bit of a file mode, and it ignores owners.

use anyhow::{Result, bail};
use pkgsmith_plugin::archive::{self, Compression, Leading};
use pkgsmith_plugin::version::Version;
use pkgsmith_plugin::{Data, Entry, Host, Package};
use serde::Deserialize;
use std::path::PathBuf;

/// The `nix:` section of the manifest.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct Options {
    /// Entries are taken relative to this absolute path. Default `/usr`.
    prefix: Option<String>,
    /// Attribute names of nixpkgs for `buildInputs`, for example `openssl` or `xorg.libX11`.
    #[serde(default)]
    inputs: Vec<String>,
    /// Attribute names of nixpkgs for `nativeBuildInputs`.
    #[serde(default)]
    native_inputs: Vec<String>,
    /// Add `autoPatchelfHook`, which points the compiled programs at the libraries of `inputs`. Default `false`.
    #[serde(default)]
    patchelf: bool,
    /// Nix systems the package is for, for example `x86_64-linux`. Default: from `arch`, and every system for `all`.
    platforms: Option<Vec<String>>,
    /// The flake input that provides nixpkgs for `flake.nix`. Default `github:NixOS/nixpkgs/nixos-unstable`.
    nixpkgs: Option<String>,
    /// `xz` (default), `gzip`, `zstd`, `bzip2`, `none`.
    compression: Option<String>,
}

/// `name-version-arch.nix.tar.xz`.
pub fn file_name(package: &Package) -> Result<String> {
    let options: Options = package.options()?;
    let version = Version::parse(&package.version)?.plain();
    let compression = Compression::parse(options.compression.as_deref().unwrap_or("xz"))?;
    Ok(format!(
        "{}-{version}-{}.nix.tar{}",
        package.name,
        package.arch,
        compression.extension()
    ))
}

pub fn build(package: &Package, host: &Host) -> Result<PathBuf> {
    let options: Options = package.options()?;
    let version = Version::parse(&package.version)?.plain();
    let compression = Compression::parse(options.compression.as_deref().unwrap_or("xz"))?;
    warn_unexpressed(package, host);
    let entries = store_entries(package, &options, host)?;
    if !entries.iter().any(|e| matches!(e.data, Data::File { .. })) {
        bail!("a Nix package needs at least one file");
    }
    let expression = expression(package, &options, &version, host)?;
    let flake = flake(package, &options)?;
    let top = format!("{}-{version}", package.name);
    let leading = [
        Leading {
            name: format!("{top}/default.nix"),
            data: expression.into_bytes(),
            mode: 0o644,
        },
        Leading {
            name: format!("{top}/flake.nix"),
            data: flake.into_bytes(),
            mode: 0o644,
        },
    ];
    let tar = archive::tar(&entries, &format!("{top}/files/"), package.mtime, &leading)?;
    let file = package.output(&file_name(package)?);
    std::fs::create_dir_all(&package.out)?;
    std::fs::write(&file, compression.compress(&tar)?)?;
    Ok(file)
}

/// The entries with the prefix removed, so that `files/` is the content of the store path.
fn store_entries(package: &Package, options: &Options, host: &Host) -> Result<Vec<Entry>> {
    let prefix = options
        .prefix
        .as_deref()
        .unwrap_or("/usr")
        .trim_end_matches('/');
    if !prefix.is_empty() && !prefix.starts_with('/') {
        bail!("nix.prefix {prefix:?} must be an absolute path");
    }
    let mut outside = 0;
    let mut entries = Vec::new();
    for entry in &package.entries {
        let mut entry = entry.clone();
        match entry.path.strip_prefix(prefix) {
            Some("") => continue,
            Some(rest) if rest.starts_with('/') => entry.path = rest.to_owned(),
            _ => outside += 1,
        }
        entries.push(entry);
    }
    if outside > 0 {
        host.warn(&format!(
            "{outside} entries are outside the prefix {prefix:?}; they go below $out with their whole path, because a Nix package cannot write outside its store path"
        ));
    }
    let absolute_links = entries
        .iter()
        .filter(|e| matches!(&e.data, Data::Symlink { target } if target.starts_with('/')))
        .count();
    if absolute_links > 0 {
        host.warn(&format!(
            "{absolute_links} symbolic links have an absolute target, which does not point into the store path"
        ));
    }
    Ok(entries)
}

/// A string literal of the Nix language. `${` would start an antiquotation, so the `$` is escaped.
fn literal(text: &str) -> String {
    let mut out = String::from("\"");
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '$' if chars.peek() == Some(&'{') => out.push_str("\\$"),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// An attribute path of nixpkgs such as `openssl` or `xorg.libX11`.
fn attribute(name: &str, key: &str) -> Result<String> {
    let valid = !name.is_empty()
        && name.split('.').all(|part| {
            let mut chars = part.chars();
            matches!(chars.next(), Some(c) if c.is_ascii_alphabetic() || c == '_')
                && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '\''))
        });
    if !valid {
        bail!(
            "nix.{key} {name:?} is not an attribute name of nixpkgs, such as openssl or xorg.libX11"
        );
    }
    Ok(name.to_owned())
}

fn license(spdx: &str) -> Option<&'static str> {
    Some(match spdx {
        "MIT" => "mit",
        "Apache-2.0" => "asl20",
        "BSD-2-Clause" => "bsd2",
        "BSD-3-Clause" => "bsd3",
        "ISC" => "isc",
        "MPL-2.0" => "mpl20",
        "Unlicense" => "unlicense",
        "0BSD" => "bsd0",
        "GPL-2.0-only" | "GPL-2.0" => "gpl2Only",
        "GPL-2.0-or-later" => "gpl2Plus",
        "GPL-3.0-only" | "GPL-3.0" => "gpl3Only",
        "GPL-3.0-or-later" => "gpl3Plus",
        "LGPL-2.1-only" => "lgpl21Only",
        "LGPL-2.1-or-later" => "lgpl21Plus",
        "LGPL-3.0-only" => "lgpl3Only",
        "LGPL-3.0-or-later" => "lgpl3Plus",
        "AGPL-3.0-only" => "agpl3Only",
        "AGPL-3.0-or-later" => "agpl3Plus",
        _ => return None,
    })
}

/// The Nix systems of the package: `platforms`, else from `arch`. `all` has no compiled code, so it covers Linux and macOS.
fn systems(package: &Package, options: &Options) -> Result<Vec<String>> {
    if let Some(list) = &options.platforms {
        for system in list {
            let valid = system.split_once('-').is_some_and(|(cpu, os)| {
                !cpu.is_empty()
                    && !os.is_empty()
                    && system
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-'))
            });
            if !valid {
                bail!("nix.platforms {system:?} must look like x86_64-linux");
            }
        }
        return Ok(list.clone());
    }
    let list: &[&str] = match package.arch.as_str() {
        "amd64" => &["x86_64-linux"],
        "arm64" => &["aarch64-linux"],
        _ => &[
            "x86_64-linux",
            "aarch64-linux",
            "x86_64-darwin",
            "aarch64-darwin",
        ],
    };
    Ok(list.iter().map(|s| s.to_string()).collect())
}

/// A `flake.nix`, so that `nix build URL` and `nix profile install URL` work on the archive itself.
fn flake(package: &Package, options: &Options) -> Result<String> {
    let nixpkgs = options
        .nixpkgs
        .as_deref()
        .unwrap_or("github:NixOS/nixpkgs/nixos-unstable");
    if nixpkgs.is_empty()
        || !nixpkgs
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || ":/@._-?=&".contains(c))
    {
        bail!(
            "nix.nixpkgs {nixpkgs:?} is not a flake reference such as github:NixOS/nixpkgs/nixos-unstable"
        );
    }
    let systems = systems(package, options)?
        .iter()
        .map(|s| literal(s))
        .collect::<Vec<_>>()
        .join(" ");
    let summary = package.description.lines().next().unwrap_or("").trim();
    Ok(format!(
        "{{\n  description = {};\n\n  inputs.nixpkgs.url = {};\n\n  outputs = {{ self, nixpkgs }}:\n    let\n      systems = [ {systems} ];\n      forAll = f: nixpkgs.lib.genAttrs systems (system: f nixpkgs.legacyPackages.${{system}});\n    in {{\n      packages = forAll (pkgs:\n        let package = import ./default.nix {{ inherit pkgs; }};\n        in {{ default = package; {} = package; }});\n    }};\n}}\n",
        literal(if summary.is_empty() {
            &package.name
        } else {
            summary
        }),
        literal(nixpkgs),
        literal(&package.name),
    ))
}

fn expression(package: &Package, options: &Options, version: &str, host: &Host) -> Result<String> {
    let inputs: Vec<String> = options
        .inputs
        .iter()
        .map(|n| attribute(n, "inputs"))
        .collect::<Result<_>>()?;
    let native: Vec<String> = options
        .native_inputs
        .iter()
        .map(|n| attribute(n, "native_inputs"))
        .collect::<Result<_>>()?;
    let platforms = if options.platforms.is_none() && package.arch == "all" {
        "lib.platforms.all".to_owned()
    } else {
        format!(
            "[ {} ]",
            systems(package, options)?
                .iter()
                .map(|s| literal(s))
                .collect::<Vec<_>>()
                .join(" ")
        )
    };

    let mut native_inputs = native;
    if options.patchelf {
        native_inputs.insert(0, "autoPatchelfHook".into());
    }
    let list = |items: &[String]| items.join(" ");
    let summary = package.description.lines().next().unwrap_or("").trim();
    let mut out = String::new();
    out.push_str("{ pkgs ? import <nixpkgs> { } }:\n\nwith pkgs;\n\nstdenv.mkDerivation {\n");
    out.push_str(&format!("  pname = {};\n", literal(&package.name)));
    out.push_str(&format!("  version = {};\n\n", literal(version)));
    out.push_str("  src = ./files;\n\n");
    out.push_str("  dontConfigure = true;\n  dontBuild = true;\n\n");
    if !native_inputs.is_empty() {
        out.push_str(&format!(
            "  nativeBuildInputs = [ {} ];\n",
            list(&native_inputs)
        ));
    }
    if !inputs.is_empty() {
        out.push_str(&format!("  buildInputs = [ {} ];\n", list(&inputs)));
    }
    out.push_str(
        "\n  installPhase = ''\n    runHook preInstall\n    mkdir -p $out\n    cp -a . $out/\n    runHook postInstall\n  '';\n\n",
    );
    out.push_str("  meta = {\n");
    if !summary.is_empty() {
        out.push_str(&format!("    description = {};\n", literal(summary)));
    }
    if let Some(homepage) = &package.homepage {
        out.push_str(&format!("    homepage = {};\n", literal(homepage)));
    }
    if let Some(spdx) = &package.license {
        match license(spdx) {
            Some(attr) => out.push_str(&format!("    license = lib.licenses.{attr};\n")),
            None => {
                host.warn(&format!(
                    "the license {spdx:?} is not one pkgsmith knows in nixpkgs, so it is written by its SPDX id only"
                ));
                out.push_str(&format!(
                    "    license = {{ spdxId = {}; shortName = {}; }};\n",
                    literal(spdx),
                    literal(spdx)
                ));
            }
        }
    }
    out.push_str(&format!("    platforms = {platforms};\n"));
    out.push_str("  };\n}\n");
    Ok(out)
}

fn warn_unexpressed(package: &Package, host: &Host) {
    let lists = [
        ("depends", &package.depends),
        ("recommends", &package.recommends),
        ("suggests", &package.suggests),
        ("conflicts", &package.conflicts),
        ("replaces", &package.replaces),
        ("provides", &package.provides),
    ];
    for (field, values) in lists {
        if !values.is_empty() {
            host.warn(&format!(
                "the {field} names of a distro mean nothing to Nix, so {} is not written; use nix.inputs for libraries",
                values.join(", ")
            ));
        }
    }
    let scripts = &package.scripts;
    if [
        &scripts.pre_install,
        &scripts.post_install,
        &scripts.pre_remove,
        &scripts.post_remove,
    ]
    .iter()
    .any(|script| script.is_some())
    {
        host.warn("a Nix package runs no install or remove scripts, so the package scripts are not written");
    }
    if package.maintainer.is_some() {
        host.warn(
            "the maintainer is not written: meta.maintainers needs a nixpkgs maintainer entry",
        );
    }
    let ghosts = package
        .entries
        .iter()
        .filter(|e| matches!(e.data, Data::Ghost))
        .count();
    if ghosts > 0 {
        host.warn(&format!(
            "{ghosts} ghost entries are owned by no package, so they are not written"
        ));
    }
    if package.entries.iter().any(|e| e.config) {
        host.warn("a Nix package has no config flag: configuration files are ordinary files in the store path");
    }
    if package
        .entries
        .iter()
        .any(|e| !matches!(e.mode & 0o7777, 0o644 | 0o755 | 0o777))
    {
        host.warn("Nix keeps only the executable bit of a mode, and it ignores owners");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn string_literals_escape_what_nix_would_expand() {
        assert_eq!(
            literal("a \"b\" ${c} \\ $d"),
            "\"a \\\"b\\\" \\${c} \\\\ $d\""
        );
    }

    #[test]
    fn attribute_names_are_nixpkgs_paths() {
        assert!(attribute("xorg.libX11", "inputs").is_ok());
        assert!(attribute("openssl_3", "inputs").is_ok());
        for bad in ["", "a b", "a;b", "1a", "a..b", "a.", "${x}"] {
            assert!(attribute(bad, "inputs").is_err(), "{bad:?}");
        }
    }
}
