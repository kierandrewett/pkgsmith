//! The `nix-cache` and `nix-export` formats: the files as a real Nix store object.
//!
//! The files become one store path, `STORE_DIR/DIGEST-NAME-VERSION`. Nix computes the digest of a path from a derivation.
//! There is none here, so the path is content-addressed: the digest comes from the NAR of the files, the references, the
//! store directory and the name. A store object named like that proves its own content, so a store accepts it without a
//! signature.
//!
//! - `nix-cache` is a binary cache in a tar: `nix-cache-info`, `DIGEST.narinfo` and `nar/FILEHASH.nar.xz`. Unpack it
//!   where a web server or `file://` can reach it, and `nix copy --from` or a substituter reads it. This is the layout
//!   that `mkBinaryCache` of nixpkgs writes.
//! - `nix-export` is the stream that `nix-store --export` writes, which `nix-store --import` reads. It is one file.
//!
//! Specifications used:
//! - Store paths: https://nix.dev/manual/nix/stable/protocols/store-path.html
//! - NAR: https://nix.dev/manual/nix/stable/protocols/nix-archive.html
//! - Binary cache store: https://nix.dev/manual/nix/stable/store/types/http-binary-cache-store.html
//! - `.narinfo` fields, and the export stream: `nar-info.cc` and `export-import.cc` in
//!   https://github.com/NixOS/nix/tree/master/src/libstore

use crate::nar;
use crate::options::{self, Options};
use anyhow::{Result, bail};
use pkgsmith_plugin::archive::{self, Compression, Leading};
use pkgsmith_plugin::version::Version;
use pkgsmith_plugin::{Host, Package};
use std::path::PathBuf;

/// The files as a store object, before it is written in a format.
struct Object {
    nar: Vec<u8>,
    nar_hash: [u8; 32],
    /// The full store path.
    path: String,
    /// `DIGEST-NAME`.
    base: String,
    /// Full store paths, sorted.
    references: Vec<String>,
}

pub fn file_name(package: &Package) -> Result<String> {
    let options = Options::of(package)?;
    let version = Version::parse(&package.version)?.plain();
    let (stem, compression) = match package.format.as_str() {
        "nix-cache" => {
            // The outer file is a plain tar: its members are compressed already.
            Compression::parse(options.compression.as_deref().unwrap_or("xz"))?;
            ("nixcache.tar", Compression::None)
        }
        _ => (
            "nixexport",
            Compression::parse(options.compression.as_deref().unwrap_or("none"))?,
        ),
    };
    Ok(format!(
        "{}-{version}-{}.{stem}{}",
        package.name,
        package.arch,
        compression.extension()
    ))
}

pub fn build(package: &Package, host: &Host) -> Result<PathBuf> {
    let options = Options::of(package)?;
    options::warn_unexpressed(package, host);
    let object = object(package, &options, host)?;
    let file = package.output(&file_name(package)?);
    std::fs::create_dir_all(&package.out)?;
    let bytes = match package.format.as_str() {
        "nix-cache" => {
            let compression = Compression::parse(options.compression.as_deref().unwrap_or("xz"))?;
            cache(package, &options, &object, compression)?
        }
        _ => {
            let compression = Compression::parse(options.compression.as_deref().unwrap_or("none"))?;
            compression.compress(&export(&object))?
        }
    };
    std::fs::write(&file, bytes)?;
    host.info(&format!("store path {}", object.path));
    Ok(file)
}

fn object(package: &Package, options: &Options, host: &Host) -> Result<Object> {
    let store_dir = options.store_dir()?;
    let version = Version::parse(&package.version)?.plain();
    let name = format!("{}-{version}", package.name);
    let entries = options::relocate(package, options, host)?;
    options::warn_entries(&entries, host);
    let nar = nar::nar(&entries)?;
    let nar_hash = nar::sha256(&nar);

    for reference in &options.references {
        let valid = reference
            .strip_prefix(store_dir)
            .and_then(|r| r.strip_prefix('/'))
            .and_then(|r| r.split_once('-'))
            .is_some_and(|(digest, name)| {
                digest.len() == 32
                    && digest
                        .bytes()
                        .all(|b| b"0123456789abcdfghijklmnpqrsvwxyz".contains(&b))
                    && nar::check_name(name).is_ok()
            });
        if !valid {
            bail!(
                "the option references has {reference:?}, which is not a store path below {store_dir}"
            );
        }
    }
    let references = nar::scan_references(
        &nar,
        store_dir,
        &options.references,
        options.scan_references.unwrap_or(true),
    );
    if !references.is_empty() {
        host.warn(&format!(
            "the files refer to {}; a store that imports the package needs those paths too, so a binary cache must offer them",
            references.join(", ")
        ));
    }
    let base = nar::store_base_name(&nar_hash, &references, store_dir, &name)?;
    Ok(Object {
        nar,
        nar_hash,
        path: format!("{store_dir}/{base}"),
        base,
        references,
    })
}

/// `sha256:` and the Nix32 of a digest, as a `.narinfo` writes a hash.
fn nix32_hash(digest: &[u8; 32]) -> String {
    format!("sha256:{}", nar::nix32(digest))
}

fn base_name(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

/// The tar of a binary cache.
fn cache(
    package: &Package,
    options: &Options,
    object: &Object,
    compression: Compression,
) -> Result<Vec<u8>> {
    let packed = compression.compress(&object.nar)?;
    let file_hash = nar::sha256(&packed);
    let (name, extension) = match compression {
        Compression::None => ("none", ""),
        Compression::Gzip => ("gzip", ".gz"),
        Compression::Xz => ("xz", ".xz"),
        Compression::Zstd => ("zstd", ".zst"),
        Compression::Bzip2 => ("bzip2", ".bz2"),
    };
    let url = format!("nar/{}.nar{extension}", nar::nix32(&file_hash));
    let references = object
        .references
        .iter()
        .map(|r| base_name(r))
        .collect::<Vec<_>>()
        .join(" ");
    let narinfo = format!(
        "StorePath: {}\nURL: {url}\nCompression: {name}\nFileHash: {}\nFileSize: {}\nNarHash: {}\nNarSize: {}\nReferences: {references}\nCA: fixed:r:{}\n",
        object.path,
        nix32_hash(&file_hash),
        packed.len(),
        nix32_hash(&object.nar_hash),
        object.nar.len(),
        nix32_hash(&object.nar_hash),
    );
    let leading = [
        Leading {
            name: "nix-cache-info".into(),
            data: format!(
                "StoreDir: {}\nWantMassQuery: 1\nPriority: 30\n",
                options.store_dir()?
            )
            .into_bytes(),
            mode: 0o644,
        },
        Leading {
            name: format!("{}.narinfo", object.base.split('-').next().unwrap_or("")),
            data: narinfo.into_bytes(),
            mode: 0o644,
        },
        Leading {
            name: url,
            data: packed,
            mode: 0o644,
        },
    ];
    archive::tar(&[], "", package.mtime, &leading)
}

/// The stream of `nix-store --export` for one store path. Every number is 64 bits, little endian, and every string is
/// the length, the bytes and zeros up to a multiple of 8.
fn export(object: &Object) -> Vec<u8> {
    fn number(out: &mut Vec<u8>, n: u64) {
        out.extend_from_slice(&n.to_le_bytes());
    }
    fn string(out: &mut Vec<u8>, text: &str) {
        number(out, text.len() as u64);
        out.extend_from_slice(text.as_bytes());
        out.resize(out.len() + (8 - text.len() % 8) % 8, 0);
    }
    let mut out = Vec::with_capacity(object.nar.len() + 1024);
    number(&mut out, 1);
    out.extend_from_slice(&object.nar);
    // The magic of an exported path, "NIXE" read as a number.
    number(&mut out, 0x4558_494e);
    string(&mut out, &object.path);
    number(&mut out, object.references.len() as u64);
    for reference in &object.references {
        string(&mut out, reference);
    }
    // No deriver, and no signature.
    string(&mut out, "");
    number(&mut out, 0);
    number(&mut out, 0);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn object(references: Vec<String>) -> Object {
        Object {
            nar: nar::nar(&[]).unwrap(),
            nar_hash: [7; 32],
            path: "/nix/store/00000000000000000000000000000000-x-1".into(),
            base: "00000000000000000000000000000000-x-1".into(),
            references,
        }
    }

    #[test]
    fn an_export_ends_with_the_path_and_two_zero_numbers() {
        let object = object(vec![]);
        let bytes = export(&object);
        assert_eq!(&bytes[..8], &1u64.to_le_bytes());
        assert_eq!(bytes.len() % 8, 0);
        assert_eq!(&bytes[bytes.len() - 16..], &[0u8; 16]);
        let text = String::from_utf8_lossy(&bytes);
        assert!(text.contains(&object.path));
    }

    #[test]
    fn export_names_its_references() {
        let other = "/nix/store/11111111111111111111111111111111-y-2".to_owned();
        let bytes = export(&object(vec![other.clone()]));
        assert!(String::from_utf8_lossy(&bytes).contains(&other));
    }

    const GLIBC: &str = "/nix/store/0123456789abcdfghijklmnpqrsvwxyz-glibc-2.40";

    fn narinfo(
        members: &std::collections::BTreeMap<String, Vec<u8>>,
    ) -> std::collections::BTreeMap<String, String> {
        let (_, text) = members
            .iter()
            .find(|(k, _)| k.ends_with(".narinfo"))
            .unwrap();
        String::from_utf8(text.clone())
            .unwrap()
            .lines()
            .map(|l| {
                let (k, v) = l.split_once(": ").unwrap_or((l.trim_end_matches(':'), ""));
                (k.to_owned(), v.to_owned())
            })
            .collect()
    }

    #[test]
    fn a_cache_is_consistent_and_names_its_references() {
        let dir = tempfile::tempdir().unwrap();
        let text = format!("#!/bin/sh\nexec {GLIBC}/bin/true\n");
        let package = crate::testing::package(
            dir.path(),
            "nix-cache",
            &[("/usr/bin/hi", &text, 0o755)],
            serde_json::json!({}),
        );
        let file = build(&package, &Host::silent()).unwrap();
        assert_eq!(file.file_name().unwrap(), "hi-1.0.0-amd64.nixcache.tar");
        let members = crate::testing::members(&std::fs::read(file).unwrap());
        let info = narinfo(&members);
        assert_eq!(
            members["nix-cache-info"],
            b"StoreDir: /nix/store\nWantMassQuery: 1\nPriority: 30\n"
        );

        // The file hash covers the compressed NAR that the URL names.
        let packed = &members[&info["URL"]];
        assert_eq!(info["FileHash"], nix32_hash(&nar::sha256(packed)));
        assert_eq!(info["FileSize"], packed.len().to_string());
        assert!(info["URL"].ends_with(".nar.xz") && info["Compression"] == "xz");
        let mut nar_bytes = Vec::new();
        std::io::copy(
            &mut liblzma::read::XzDecoder::new(packed.as_slice()),
            &mut nar_bytes,
        )
        .unwrap();
        assert_eq!(info["NarSize"], nar_bytes.len().to_string());
        assert_eq!(info["NarHash"], nix32_hash(&nar::sha256(&nar_bytes)));
        assert_eq!(info["CA"], format!("fixed:r:{}", info["NarHash"]));

        // The store path is the one that the specification gives for this NAR and this reference.
        let base = nar::store_base_name(
            &nar::sha256(&nar_bytes),
            &[GLIBC.to_owned()],
            "/nix/store",
            "hi-1.0.0",
        )
        .unwrap();
        assert_eq!(info["StorePath"], format!("/nix/store/{base}"));
        assert_eq!(
            info["References"],
            "0123456789abcdfghijklmnpqrsvwxyz-glibc-2.40"
        );
        assert!(members.contains_key(&format!("{}.narinfo", base.split('-').next().unwrap())));
    }

    #[test]
    fn references_can_be_off_and_must_be_store_paths() {
        let dir = tempfile::tempdir().unwrap();
        let text = format!("{GLIBC}/lib");
        let mut package = crate::testing::package(
            dir.path(),
            "nix-cache",
            &[("/usr/lib/libhi.so", &text, 0o644)],
            serde_json::json!({"scan_references": false, "compression": "none"}),
        );
        let file = build(&package, &Host::silent()).unwrap();
        let members = crate::testing::members(&std::fs::read(file).unwrap());
        let info = narinfo(&members);
        assert_eq!(info["References"], "");
        assert!(info["URL"].ends_with(".nar") && info["Compression"] == "none");

        package.options = serde_json::json!({"references": ["/usr/lib/libc.so"]});
        assert!(build(&package, &Host::silent()).is_err());
        package.options = serde_json::json!({"store_dir": "/nix/store/"});
        assert!(build(&package, &Host::silent()).is_err());
        // A named path is a candidate: it is kept when its digest is in the files, as Nix does for the inputs of a build.
        package.options = serde_json::json!({"scan_references": false, "references": [GLIBC]});
        let file = build(&package, &Host::silent()).unwrap();
        let info = narinfo(&crate::testing::members(&std::fs::read(file).unwrap()));
        assert_eq!(
            info["References"],
            "0123456789abcdfghijklmnpqrsvwxyz-glibc-2.40"
        );
    }

    #[test]
    fn an_export_is_what_nix_store_import_reads() {
        let dir = tempfile::tempdir().unwrap();
        let package = crate::testing::package(
            dir.path(),
            "nix-export",
            &[("/usr/bin/hi", "#!/bin/sh\n", 0o755)],
            serde_json::json!({}),
        );
        let file = build(&package, &Host::silent()).unwrap();
        assert_eq!(file.file_name().unwrap(), "hi-1.0.0-amd64.nixexport");
        let bytes = std::fs::read(file).unwrap();
        assert_eq!(&bytes[..8], &1u64.to_le_bytes());
        let nar_length = nar::nar(&[]).unwrap().len();
        assert!(bytes.len() > nar_length);
    }
}
