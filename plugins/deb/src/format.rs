//! Debian packages (`.deb`) and OpenWrt packages (`.ipk`), which share one control file layout.

use anyhow::Result;
use pkgsmith_plugin::Package;
use pkgsmith_plugin::archive::{self, Compression, Leading};
use pkgsmith_plugin::entry::{self, Data};
use pkgsmith_plugin::hash::digests;
use pkgsmith_plugin::version::Version;
use serde::Deserialize;
use std::fmt::Write as _;
use std::path::PathBuf;

/// The `deb:` section of the manifest.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct DebOptions {
    section: Option<String>,
    priority: Option<String>,
    /// `xz` (default), `zstd`, `gzip`.
    compression: Option<String>,
}

/// The `ipk:` section of the manifest.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct IpkOptions {
    /// The OpenWrt architecture, for example `x86_64` or `aarch64_generic`. Default: derived from `arch`.
    arch: Option<String>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Flavour {
    Deb,
    Ipk,
}

/// The architecture name the format uses.
fn architecture(package: &Package, flavour: Flavour) -> Result<String> {
    let arch = package.arch.as_str();
    if flavour == Flavour::Ipk {
        let options: IpkOptions = package.options()?;
        if let Some(own) = options.arch {
            return Ok(own);
        }
        return Ok(match arch {
            "amd64" => "x86_64".into(),
            "arm64" => "aarch64_generic".into(),
            other => other.into(),
        });
    }
    Ok(arch.into())
}

/// `name_version-release_arch.deb` or `.ipk`.
pub fn file_name(package: &Package, flavour: Flavour) -> Result<String> {
    let version = format!(
        "{}-{}",
        Version::parse(&package.version)?.tilde(),
        package.release
    );
    let extension = if flavour == Flavour::Ipk {
        "ipk"
    } else {
        "deb"
    };
    Ok(format!(
        "{}_{version}_{}.{extension}",
        package.name,
        architecture(package, flavour)?
    ))
}

/// `Description:` is a summary line, then the body with every line indented by one space and blank lines as ` .`.
fn description(package: &Package) -> String {
    let mut lines = package.description.trim_end().lines();
    let mut text = lines.next().unwrap_or("").trim().to_string();
    for line in lines {
        text.push_str("\n ");
        text.push_str(if line.trim().is_empty() {
            "."
        } else {
            line.trim_end()
        });
    }
    text
}

fn control(package: &Package, flavour: Flavour, version: &str) -> Result<String> {
    let options: DebOptions = if flavour == Flavour::Deb {
        package.options()?
    } else {
        DebOptions::default()
    };
    let mut text = String::new();
    let mut field = |name: &str, value: &str| {
        if !value.is_empty() {
            let _ = writeln!(text, "{name}: {value}");
        }
    };
    field("Package", &package.name);
    field("Version", version);
    field("Architecture", &architecture(package, flavour)?);
    field(
        "Maintainer",
        package
            .maintainer
            .as_deref()
            .unwrap_or("Unknown <unknown@localhost>"),
    );
    field(
        "Installed-Size",
        &package.installed_size().div_ceil(1024).to_string(),
    );
    field("Depends", &package.depends.join(", "));
    field("Recommends", &package.recommends.join(", "));
    field("Suggests", &package.suggests.join(", "));
    field("Conflicts", &package.conflicts.join(", "));
    field("Replaces", &package.replaces.join(", "));
    field("Provides", &package.provides.join(", "));
    field("Section", options.section.as_deref().unwrap_or("misc"));
    field(
        "Priority",
        options.priority.as_deref().unwrap_or("optional"),
    );
    field("Homepage", package.homepage.as_deref().unwrap_or(""));
    field("Description", &description(package));
    Ok(text)
}

pub fn build(package: &Package, flavour: Flavour) -> Result<PathBuf> {
    let semver = Version::parse(&package.version)?;
    let version = format!("{}-{}", semver.tilde(), package.release);
    let mtime = package.mtime;

    let mut control_files = vec![Leading {
        name: "./control".into(),
        data: control(package, flavour, &version)?.into_bytes(),
        mode: 0o644,
    }];
    let mut sums = String::new();
    for entry in &package.entries {
        if let Data::File { src, .. } = &entry.data {
            let _ = writeln!(
                sums,
                "{}  {}",
                digests(&mut std::fs::File::open(src)?)?.0,
                entry.path.trim_start_matches('/')
            );
        }
    }
    control_files.push(Leading {
        name: "./md5sums".into(),
        data: sums.into_bytes(),
        mode: 0o644,
    });
    let conffiles: String = package
        .entries
        .iter()
        .filter(|e| e.config)
        .map(|e| format!("{}\n", e.path))
        .collect();
    if !conffiles.is_empty() {
        control_files.push(Leading {
            name: "./conffiles".into(),
            data: conffiles.into_bytes(),
            mode: 0o644,
        });
    }
    let scripts = &package.scripts;
    for (name, value) in [
        ("preinst", &scripts.pre_install),
        ("postinst", &scripts.post_install),
        ("prerm", &scripts.pre_remove),
        ("postrm", &scripts.post_remove),
    ] {
        if let Some(text) = value {
            control_files.push(Leading {
                name: format!("./{name}"),
                data: text.clone().into_bytes(),
                mode: 0o755,
            });
        }
    }

    let (compression, suffix) = match flavour {
        Flavour::Deb => {
            let options: DebOptions = package.options()?;
            let name = options.compression.as_deref().unwrap_or("xz");
            let compression = Compression::parse(name)?;
            (compression, compression.extension())
        }
        Flavour::Ipk => (Compression::Gzip, ".gz"),
    };
    let control_tar = compression.compress(&archive::tar(&[], "./", mtime, &control_files)?)?;
    let data_tar = compression.compress(&archive::tar(
        &entry::with_parents(&package.entries),
        "./",
        mtime,
        &[],
    )?)?;

    let control_name = format!("control.tar{suffix}");
    let data_name = format!("data.tar{suffix}");
    let bytes = match flavour {
        Flavour::Deb => archive::ar(
            &[
                ("debian-binary", b"2.0\n"),
                (&control_name, &control_tar),
                (&data_name, &data_tar),
            ],
            mtime,
        )?,
        Flavour::Ipk => {
            // opkg reads a gzip tar that holds the three members, in this order.
            let members = [
                Leading {
                    name: "./debian-binary".into(),
                    data: b"2.0\n".to_vec(),
                    mode: 0o644,
                },
                Leading {
                    name: "./data.tar.gz".into(),
                    data: data_tar,
                    mode: 0o644,
                },
                Leading {
                    name: "./control.tar.gz".into(),
                    data: control_tar,
                    mode: 0o644,
                },
            ];
            Compression::Gzip.compress(&archive::tar(&[], "", mtime, &members)?)?
        }
    };
    std::fs::create_dir_all(&package.out)?;
    let file = package.output(&file_name(package, flavour)?);
    std::fs::write(&file, bytes)?;
    Ok(file)
}
