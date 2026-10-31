//! tar and ar archives with the compression the package formats use.

use crate::tree::{Data, Entry};
use anyhow::{Result, bail};
use std::io::{self, Write};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Compression {
    None,
    Gzip,
    Xz,
    Zstd,
}

impl Compression {
    pub fn parse(name: &str) -> Result<Compression> {
        Ok(match name {
            "none" => Compression::None,
            "gzip" | "gz" => Compression::Gzip,
            "xz" => Compression::Xz,
            "zstd" | "zst" => Compression::Zstd,
            other => bail!("unknown compression {other:?}; use none, gzip, xz or zstd"),
        })
    }

    pub fn extension(self) -> &'static str {
        match self {
            Compression::None => "",
            Compression::Gzip => ".gz",
            Compression::Xz => ".xz",
            Compression::Zstd => ".zst",
        }
    }

    pub fn compress(self, data: &[u8]) -> Result<Vec<u8>> {
        Ok(match self {
            Compression::None => data.to_vec(),
            Compression::Gzip => {
                // The gzip header holds a time stamp; flate2 writes zero, so equal input gives equal bytes.
                let mut encoder =
                    flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::best());
                encoder.write_all(data)?;
                encoder.finish()?
            }
            Compression::Xz => {
                let mut encoder = xz2::write::XzEncoder::new(Vec::new(), 6);
                encoder.write_all(data)?;
                encoder.finish()?
            }
            Compression::Zstd => zstd::stream::encode_all(data, 19)?,
        })
    }
}

/// A file added before the packaged files, such as `.PKGINFO`.
pub struct Leading {
    pub name: String,
    pub data: Vec<u8>,
    pub mode: u32,
}

/// A tar of the entries with `prefix` before every path (`./` for deb, empty for Arch). Paths lose their leading `/`.
/// Ghost entries are not carried. Owners are written by name, with the numeric id 0.
pub fn tar(entries: &[Entry], prefix: &str, mtime: u64, leading: &[Leading]) -> Result<Vec<u8>> {
    let mut builder = tar::Builder::new(Vec::new());
    builder.mode(tar::HeaderMode::Complete);
    let header = |mode: u32,
                  kind: tar::EntryType,
                  size: u64,
                  owner: &str,
                  group: &str|
     -> Result<tar::Header> {
        let mut header = tar::Header::new_gnu();
        header.set_mode(mode);
        header.set_entry_type(kind);
        header.set_size(size);
        header.set_mtime(mtime);
        header.set_uid(0);
        header.set_gid(0);
        header.set_username(owner)?;
        header.set_groupname(group)?;
        Ok(header)
    };
    for file in leading {
        let mut h = header(
            file.mode,
            tar::EntryType::Regular,
            file.data.len() as u64,
            "root",
            "root",
        )?;
        builder.append_data(&mut h, &file.name, file.data.as_slice())?;
    }
    for entry in entries {
        let name = format!("{prefix}{}", entry.path.trim_start_matches('/'));
        match &entry.data {
            Data::Ghost => {}
            Data::Dir => {
                let mut h = header(
                    entry.mode,
                    tar::EntryType::Directory,
                    0,
                    &entry.owner,
                    &entry.group,
                )?;
                builder.append_data(&mut h, format!("{name}/"), io::empty())?;
            }
            Data::Symlink { target } => {
                let mut h = header(
                    entry.mode,
                    tar::EntryType::Symlink,
                    0,
                    &entry.owner,
                    &entry.group,
                )?;
                builder.append_link(&mut h, &name, target)?;
            }
            Data::File { src, size } => {
                let mut h = header(
                    entry.mode,
                    tar::EntryType::Regular,
                    *size,
                    &entry.owner,
                    &entry.group,
                )?;
                builder.append_data(&mut h, &name, std::fs::File::open(src)?)?;
            }
        }
    }
    Ok(builder.into_inner()?)
}

/// A Unix `ar` archive: the container of deb packages.
pub fn ar(members: &[(&str, &[u8])], mtime: u64) -> Result<Vec<u8>> {
    let mut out = b"!<arch>\n".to_vec();
    for (name, data) in members {
        if name.len() > 16 {
            bail!("ar member name {name:?} is longer than 16 characters");
        }
        out.extend(
            format!(
                "{name:<16}{mtime:<12}{:<6}{:<6}{:<8}{:<10}`\n",
                0,
                0,
                "100644",
                data.len()
            )
            .bytes(),
        );
        out.extend_from_slice(data);
        if data.len() % 2 == 1 {
            out.push(b'\n');
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ar_pads_members_to_even_length_and_writes_fixed_width_headers() {
        let archive = ar(&[("a", b"abc"), ("b", b"de")], 0).unwrap();
        assert_eq!(&archive[..8], b"!<arch>\n");
        assert_eq!(archive.len(), 8 + 60 + 4 + 60 + 2);
        assert_eq!(&archive[8 + 58..8 + 60], b"`\n");
    }

    #[test]
    fn compression_round_trips_through_each_codec() {
        let data = b"gnoblin ".repeat(500);
        for name in ["gzip", "xz", "zstd", "none"] {
            let packed = Compression::parse(name).unwrap().compress(&data).unwrap();
            let restored = match name {
                "gzip" => io::Read::bytes(flate2::read::GzDecoder::new(packed.as_slice()))
                    .collect::<Result<Vec<_>, _>>()
                    .unwrap(),
                "xz" => io::Read::bytes(xz2::read::XzDecoder::new(packed.as_slice()))
                    .collect::<Result<Vec<_>, _>>()
                    .unwrap(),
                "zstd" => zstd::stream::decode_all(packed.as_slice()).unwrap(),
                _ => packed.clone(),
            };
            assert_eq!(restored, data, "{name}");
        }
    }
}
