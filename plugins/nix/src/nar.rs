//! The Nix Archive (NAR) of a file tree, and the store path of a content-addressed store object.
//!
//! Specifications used:
//! - NAR format: https://nix.dev/manual/nix/stable/protocols/nix-archive.html
//! - Store path calculation, `source:` type: https://nix.dev/manual/nix/stable/protocols/store-path.html
//! - Nix32: https://nix.dev/manual/nix/stable/protocols/nix32.html
//!
//! A tree that was never built by Nix has no derivation, so its store path comes from its content: the SHA-256 of its NAR,
//! its references, the store directory and its name. Nix calls this a content-addressed ("source") store object.

use anyhow::{Context, Result, bail};
use pkgsmith_plugin::{Data, Entry};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::path::PathBuf;

/// A node of the tree that becomes the NAR.
enum Node {
    File { src: PathBuf, executable: bool },
    Link(String),
    Dir(BTreeMap<String, Node>),
}

/// The NAR of the entries. Paths are relative to the store path: `/bin/hi` becomes `bin/hi`. The root is a folder.
/// A ghost entry is not carried. Nix keeps the executable bit of the owner and nothing else of a mode.
pub fn nar(entries: &[Entry]) -> Result<Vec<u8>> {
    let mut root = BTreeMap::new();
    for entry in entries {
        let relative = entry.path.trim_matches('/');
        if relative.is_empty() {
            continue;
        }
        let node = match &entry.data {
            Data::Ghost => continue,
            Data::Dir => Node::Dir(BTreeMap::new()),
            Data::Symlink { target } => Node::Link(target.clone()),
            Data::File { src, .. } => Node::File {
                src: src.clone(),
                executable: entry.mode & 0o100 != 0,
            },
        };
        insert(&mut root, relative, node)?;
    }
    let mut out = Vec::new();
    string(&mut out, b"nix-archive-1");
    write_node(&mut out, &Node::Dir(root))?;
    Ok(out)
}

fn insert(dir: &mut BTreeMap<String, Node>, path: &str, node: Node) -> Result<()> {
    let mut parts = path.split('/').peekable();
    let mut current = dir;
    while let Some(part) = parts.next() {
        if part.is_empty() || part == "." || part == ".." || part.contains('\0') {
            bail!("{path:?} has a name that a Nix archive cannot hold");
        }
        if parts.peek().is_none() {
            match (current.get_mut(part), node) {
                // A folder entry for a folder that its files already created: keep the files.
                (Some(Node::Dir(_)), Node::Dir(_)) => {}
                (Some(_), _) => bail!("{path:?} is in the package twice"),
                (None, node) => {
                    current.insert(part.to_owned(), node);
                }
            }
            return Ok(());
        }
        let child = current
            .entry(part.to_owned())
            .or_insert_with(|| Node::Dir(BTreeMap::new()));
        current = match child {
            Node::Dir(children) => children,
            _ => bail!("{path:?} is below a file or a symbolic link"),
        };
    }
    Ok(())
}

/// `str(s)` of the specification: the length as 64 bits, little endian, then the bytes, padded with zeros to 8.
fn string(out: &mut Vec<u8>, bytes: &[u8]) {
    out.extend_from_slice(&(bytes.len() as u64).to_le_bytes());
    out.extend_from_slice(bytes);
    out.resize(out.len() + (8 - bytes.len() % 8) % 8, 0);
}

fn write_node(out: &mut Vec<u8>, node: &Node) -> Result<()> {
    string(out, b"(");
    string(out, b"type");
    match node {
        Node::File { src, executable } => {
            string(out, b"regular");
            if *executable {
                string(out, b"executable");
                string(out, b"");
            }
            string(out, b"contents");
            let data = std::fs::read(src).with_context(|| format!("reading {}", src.display()))?;
            string(out, &data);
        }
        Node::Link(target) => {
            string(out, b"symlink");
            string(out, b"target");
            string(out, target.as_bytes());
        }
        Node::Dir(children) => {
            string(out, b"directory");
            // A BTreeMap of strings is ordered by bytes, the order that a NAR requires.
            for (name, child) in children {
                string(out, b"entry");
                string(out, b"(");
                string(out, b"name");
                string(out, name.as_bytes());
                string(out, b"node");
                write_node(out, child)?;
                string(out, b")");
            }
        }
    }
    string(out, b")");
    Ok(())
}

pub fn sha256(data: &[u8]) -> [u8; 32] {
    Sha256::digest(data).into()
}

const NIX32: &[u8; 32] = b"0123456789abcdfghijklmnpqrsvwxyz";

/// Nix32: base 32 over the bytes from the last one to the first, five bits at a time.
pub fn nix32(hash: &[u8]) -> String {
    let length = (hash.len() * 8 - 1) / 5 + 1;
    (0..length)
        .rev()
        .map(|n| {
            let bit = n * 5;
            let (index, shift) = (bit / 8, bit % 8);
            let low = u32::from(hash[index]) >> shift;
            let high = hash
                .get(index + 1)
                .map_or(0, |b| u32::from(*b) << (8 - shift));
            NIX32[((low | high) & 0x1f) as usize] as char
        })
        .collect()
}

/// Folds a hash to 20 bytes by XOR, as the store path digest requires.
fn compress_to_20(hash: &[u8; 32]) -> [u8; 20] {
    let mut out = [0u8; 20];
    for (i, byte) in hash.iter().enumerate() {
        out[i % 20] ^= byte;
    }
    out
}

/// The characters that a store object name may hold.
pub fn check_name(name: &str) -> Result<()> {
    let valid = !name.is_empty()
        && name.len() <= 211
        && !name.starts_with('.')
        && !name.ends_with(".drv")
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"+-._?=".contains(&b));
    if !valid {
        bail!(
            "{name:?} is not a store object name: use up to 211 letters, digits and + - . _ ? =, with no leading dot and no .drv ending"
        );
    }
    Ok(())
}

/// The base name `DIGEST-NAME` of the content-addressed store path of a NAR with the given SHA-256 and references. A
/// reference is a full store path, such as `/nix/store/DIGEST-glibc-2.40`.
pub fn store_base_name(
    nar_hash: &[u8; 32],
    references: &[String],
    store_dir: &str,
    name: &str,
) -> Result<String> {
    check_name(name)?;
    let mut sorted: Vec<&String> = references.iter().collect();
    sorted.sort_by_key(|path| path.rsplit('/').next().unwrap_or(path).to_owned());
    let mut fingerprint = String::from("source");
    for reference in sorted {
        fingerprint.push(':');
        fingerprint.push_str(reference);
    }
    fingerprint.push_str(":sha256:");
    fingerprint.push_str(&pkgsmith_plugin::hash::hex(nar_hash));
    fingerprint.push(':');
    fingerprint.push_str(store_dir);
    fingerprint.push(':');
    fingerprint.push_str(name);
    let digest = compress_to_20(&sha256(fingerprint.as_bytes()));
    Ok(format!("{}-{name}", nix32(&digest)))
}

/// The store paths in `data`: every `STORE_DIR/DIGEST-NAME` where DIGEST is 32 Nix32 characters. `known` are full store
/// paths that the caller names; a digest that matches one of them gives that path. With `discover`, any other digest
/// gives the path that the text spells, whose name ends at the first character that a name cannot hold. Nix itself
/// scans for the digests of the paths that the build could use. pkgsmith has no build, so it reads the paths from the text.
pub fn scan_references(
    data: &[u8],
    store_dir: &str,
    known: &[String],
    discover: bool,
) -> Vec<String> {
    let needle = format!("{store_dir}/");
    let needle = needle.as_bytes();
    let mut found: BTreeMap<String, String> = BTreeMap::new();
    let mut from = 0;
    while let Some(at) = find(&data[from..], needle) {
        let start = from + at + needle.len();
        from = start;
        let Some(digest) = data.get(start..start + 32) else {
            break;
        };
        if !digest.iter().all(|b| NIX32.contains(b)) {
            continue;
        }
        let digest = String::from_utf8_lossy(digest).into_owned();
        if let Some(path) = known.iter().find(|p| p.contains(&format!("/{digest}-"))) {
            found.insert(digest, path.clone());
            continue;
        }
        if discover && data.get(start + 32) == Some(&b'-') {
            let name: String = data[start + 33..]
                .iter()
                .take_while(|b| b.is_ascii_alphanumeric() || b"+-._?=".contains(b))
                .map(|b| *b as char)
                .collect();
            let name = name.trim_end_matches('.');
            if !name.is_empty() && name.len() <= 211 {
                found
                    .entry(digest.clone())
                    .or_insert_with(|| format!("{store_dir}/{digest}-{name}"));
            }
        }
    }
    found.into_values().collect()
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(path: &str, src: &std::path::Path, mode: u32) -> Entry {
        Entry {
            path: path.into(),
            data: Data::File {
                src: src.to_owned(),
                size: 0,
            },
            mode,
            owner: "root".into(),
            group: "root".into(),
            config: false,
        }
    }

    #[test]
    fn nix32_of_the_known_digest() {
        // `nix hash to-base32` of the SHA-256 of "abc" is 1b8m03r63zqhnjf7l5wnldhh7c134ap5vpj0850ymkq1iyzicy5s.
        assert_eq!(
            nix32(&sha256(b"abc")),
            "1b8m03r63zqhnjf7l5wnldhh7c134ap5vpj0850ymkq1iyzicy5s"
        );
    }

    #[test]
    fn a_store_path_matches_the_one_nix_gives() {
        // `echo hello > d/f; nix store add --mode nar --name x-1 d` printed
        // /nix/store/34iwnl73jrwnfm9znrglzgjk0v6pjff0-x-1 (Nix 2.35.4).
        let dir = std::env::temp_dir().join(format!("pkgsmith-path-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join("f");
        std::fs::write(&f, b"hello\n").unwrap();
        let nar = nar(&[file("/f", &f, 0o644)]).unwrap();
        let base = store_base_name(&sha256(&nar), &[], "/nix/store", "x-1").unwrap();
        assert_eq!(base, "34iwnl73jrwnfm9znrglzgjk0v6pjff0-x-1");
        // The store directory is part of the digest.
        let other = store_base_name(&sha256(&nar), &[], "/gnu/store", "x-1").unwrap();
        assert_ne!(base, other);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_nar_is_padded_and_ordered() {
        let dir = std::env::temp_dir().join(format!("pkgsmith-nar-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let a = dir.join("a");
        std::fs::write(&a, b"abc").unwrap();
        let entries = [file("/b", &a, 0o755), file("/a", &a, 0o644)];
        let nar = nar(&entries).unwrap();
        assert_eq!(nar.len() % 8, 0);
        let text = String::from_utf8_lossy(&nar);
        assert!(text.starts_with("\u{d}\0\0\0\0\0\0\0nix-archive-1"));
        // `a` comes before `b` and only `b` is executable.
        let (first, second) = (
            text.find("\u{1}\0\0\0\0\0\0\0a").unwrap(),
            text.find("\u{1}\0\0\0\0\0\0\0b").unwrap(),
        );
        assert!(first < second);
        assert_eq!(text.matches("executable").count(), 1);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn folders_and_files_mix_and_clashes_fail() {
        let mut root = BTreeMap::new();
        insert(&mut root, "a/b", Node::Link("x".into())).unwrap();
        insert(&mut root, "a", Node::Dir(BTreeMap::new())).unwrap();
        assert!(insert(&mut root, "a/b", Node::Link("y".into())).is_err());
        assert!(insert(&mut root, "a/b/c", Node::Link("y".into())).is_err());
        assert!(insert(&mut root, "a/../c", Node::Link("y".into())).is_err());
    }

    #[test]
    fn names_follow_the_store_rules() {
        assert!(check_name("hi-1.0.0").is_ok());
        for bad in ["", ".x", "a b", "x.drv", "a/b", &"a".repeat(212)] {
            assert!(check_name(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn references_are_found_by_their_digest() {
        let digest = "0123456789abcdfghijklmnpqrsvwxyz";
        let text =
            format!("x -L/nix/store/{digest}-glibc-2.40/lib:/nix/store/{digest}-glibc-2.40.\0");
        let path = format!("/nix/store/{digest}-glibc-2.40");
        assert_eq!(
            scan_references(text.as_bytes(), "/nix/store", &[], true),
            vec![path.clone()]
        );
        assert!(scan_references(text.as_bytes(), "/nix/store", &[], false).is_empty());
        assert_eq!(
            scan_references(
                text.as_bytes(),
                "/nix/store",
                std::slice::from_ref(&path),
                false
            ),
            vec![path]
        );
        // A digest with a letter that Nix32 does not use is no store path.
        assert!(
            scan_references(
                b"/nix/store/eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee-x",
                "/nix/store",
                &[],
                true
            )
            .is_empty()
        );
    }
}
