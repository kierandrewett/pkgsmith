//! OCI container images: an image layout in a tar file, which `podman load`, `docker load` and `skopeo` read.
//!
//! The image has one layer, the files of the package, on top of nothing (`FROM scratch`). That suits a static program
//! or a bundle that carries its own libraries. The archive holds `oci-layout`, `index.json` and the blobs: the layer,
//! the image configuration and the manifest, each named by the SHA-256 digest of its bytes.
//! Reference: <https://github.com/opencontainers/image-spec/blob/main/image-layout.md>.

use anyhow::{Result, bail};
use pkgsmith_plugin::archive::{self, Compression, Leading};
use pkgsmith_plugin::entry;
use pkgsmith_plugin::hash::hex;
use pkgsmith_plugin::version::Version;
use pkgsmith_plugin::{Host, Package};
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::path::PathBuf;

/// The `oci:` section of the manifest.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct Options {
    /// The program the container runs, as a list: `["/usr/bin/hello"]`.
    #[serde(default)]
    entrypoint: Vec<String>,
    /// Default arguments, which `docker run IMAGE ARGS` replaces.
    #[serde(default)]
    cmd: Vec<String>,
    /// Environment variables.
    #[serde(default)]
    env: BTreeMap<String, String>,
    /// The folder the program starts in.
    workdir: Option<String>,
    /// The user the program runs as: a name or number, optionally `user:group`.
    user: Option<String>,
    /// Ports the program listens on, such as `8080/tcp`.
    #[serde(default)]
    ports: Vec<String>,
    /// Extra labels. pkgsmith sets the `org.opencontainers.image.*` labels from the manifest.
    #[serde(default)]
    labels: BTreeMap<String, String>,
    /// The image name in `index.json`. Default: the package name.
    repository: Option<String>,
    /// The image tag. Default: the version.
    tag: Option<String>,
    /// `gzip` (default) or `none` for the layer.
    compression: Option<String>,
}

fn architecture(arch: &str) -> Result<&'static str> {
    match arch {
        "amd64" => Ok("amd64"),
        "arm64" => Ok("arm64"),
        other => bail!("an image is for one architecture; arch {other:?} must be amd64 or arm64"),
    }
}

pub fn file_name(package: &Package) -> Result<String> {
    Ok(format!(
        "{}-{}-{}.oci.tar",
        package.name,
        Version::parse(&package.version)?.plain(),
        architecture(&package.arch)?
    ))
}

fn digest(bytes: &[u8]) -> String {
    format!("sha256:{}", hex(&Sha256::digest(bytes)))
}

fn blob(bytes: Vec<u8>) -> (String, Leading) {
    let digest = digest(&bytes);
    let name = format!("blobs/sha256/{}", digest.trim_start_matches("sha256:"));
    (
        digest,
        Leading {
            name,
            data: bytes,
            mode: 0o644,
        },
    )
}

fn json_bytes(value: &Value) -> Result<Vec<u8>> {
    Ok(serde_json::to_vec(value)?)
}

pub fn build(package: &Package, host: &Host) -> Result<PathBuf> {
    let options: Options = package.options()?;
    let arch = architecture(&package.arch)?;
    let version = Version::parse(&package.version)?.plain();
    let compression = match options.compression.as_deref().unwrap_or("gzip") {
        "gzip" => Compression::Gzip,
        "none" => Compression::None,
        other => bail!("compression {other:?} must be gzip or none"),
    };
    if options.entrypoint.is_empty() && options.cmd.is_empty() {
        host.warn("the image has no entrypoint or cmd, so `run` needs a command");
    }
    if package.scripts.post_install.is_some() || package.scripts.pre_install.is_some() {
        host.warn("an image is not installed, so the install scripts are ignored");
    }

    // The layer: the files, with their folders. The diff id is the digest of the tar before compression.
    let layer_tar = archive::tar(
        &entry::with_parents(&package.entries),
        "",
        package.mtime,
        &[],
    )?;
    let diff_id = digest(&layer_tar);
    let (layer_digest, layer_blob) = blob(compression.compress(&layer_tar)?);
    let layer_size = layer_blob.data.len();
    let layer_type = match compression {
        Compression::Gzip => "application/vnd.oci.image.layer.v1.tar+gzip",
        _ => "application/vnd.oci.image.layer.v1.tar",
    };

    let mut labels: BTreeMap<String, String> = BTreeMap::new();
    labels.insert(
        "org.opencontainers.image.title".into(),
        package.name.clone(),
    );
    labels.insert("org.opencontainers.image.version".into(), version.clone());
    labels.insert(
        "org.opencontainers.image.description".into(),
        package.summary().to_string(),
    );
    if let Some(url) = &package.homepage {
        labels.insert("org.opencontainers.image.url".into(), url.clone());
    }
    if let Some(license) = &package.license {
        labels.insert("org.opencontainers.image.licenses".into(), license.clone());
    }
    labels.extend(options.labels.clone());

    let mut runtime = serde_json::Map::new();
    if !options.entrypoint.is_empty() {
        runtime.insert("Entrypoint".into(), json!(options.entrypoint));
    }
    if !options.cmd.is_empty() {
        runtime.insert("Cmd".into(), json!(options.cmd));
    }
    if !options.env.is_empty() {
        let env: Vec<String> = options
            .env
            .iter()
            .map(|(k, v)| format!("{k}={v}"))
            .collect();
        runtime.insert("Env".into(), json!(env));
    }
    if let Some(dir) = &options.workdir {
        runtime.insert("WorkingDir".into(), json!(dir));
    }
    if let Some(user) = &options.user {
        runtime.insert("User".into(), json!(user));
    }
    if !options.ports.is_empty() {
        let ports: BTreeMap<&String, Value> =
            options.ports.iter().map(|p| (p, json!({}))).collect();
        runtime.insert("ExposedPorts".into(), json!(ports));
    }
    runtime.insert("Labels".into(), json!(labels));

    let created = timestamp(package.mtime);
    let config = json!({
        "created": created,
        "architecture": arch,
        "os": "linux",
        "config": runtime,
        "rootfs": { "type": "layers", "diff_ids": [diff_id] },
        "history": [{ "created": created, "created_by": "pkgsmith" }],
    });
    let (config_digest, config_blob) = blob(json_bytes(&config)?);
    let config_size = config_blob.data.len();

    let manifest = json!({
        "schemaVersion": 2,
        "mediaType": "application/vnd.oci.image.manifest.v1+json",
        "config": {
            "mediaType": "application/vnd.oci.image.config.v1+json",
            "digest": config_digest,
            "size": config_size,
        },
        "layers": [{ "mediaType": layer_type, "digest": layer_digest, "size": layer_size }],
    });
    let (manifest_digest, manifest_blob) = blob(json_bytes(&manifest)?);
    let manifest_size = manifest_blob.data.len();

    let reference = options.tag.clone().unwrap_or(version);
    let repository = options
        .repository
        .clone()
        .unwrap_or_else(|| package.name.clone());
    let index = json!({
        "schemaVersion": 2,
        "mediaType": "application/vnd.oci.image.index.v1+json",
        "manifests": [{
            "mediaType": "application/vnd.oci.image.manifest.v1+json",
            "digest": manifest_digest,
            "size": manifest_size,
            "platform": { "architecture": arch, "os": "linux" },
            "annotations": {
                "org.opencontainers.image.ref.name": reference,
                "io.containerd.image.name": format!("{repository}:{reference}"),
            },
        }],
    });

    let members = vec![
        Leading {
            name: "oci-layout".into(),
            data: json_bytes(&json!({"imageLayoutVersion": "1.0.0"}))?,
            mode: 0o644,
        },
        Leading {
            name: "index.json".into(),
            data: json_bytes(&index)?,
            mode: 0o644,
        },
        layer_blob,
        config_blob,
        manifest_blob,
    ];
    let bytes = archive::tar(&[], "", package.mtime, &members)?;
    let file = package.output(&file_name(package)?);
    std::fs::write(&file, bytes)?;
    Ok(file)
}

/// `2026-10-31T21:26:40Z` for a Unix time.
fn timestamp(seconds: u64) -> String {
    let days = (seconds / 86_400) as i64;
    let rest = seconds % 86_400;
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let day_of_era = z.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_index = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_index + 2) / 5 + 1;
    let month = if month_index < 10 {
        month_index + 3
    } else {
        month_index - 9
    };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        rest / 3600,
        rest % 3600 / 60,
        rest % 60
    )
}
