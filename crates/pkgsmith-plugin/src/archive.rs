//! tar and ar archives with the compression the package formats use.

use crate::entry::{Data, Entry};
use anyhow::{Result, bail};
use sha1::Digest;
use std::io::{self, Write};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Compression {
    None,
    Gzip,
    Xz,
    Zstd,
    Bzip2,
}

impl Compression {
    pub fn parse(name: &str) -> Result<Compression> {
        Ok(match name {
            "none" => Compression::None,
            "gzip" | "gz" => Compression::Gzip,
            "xz" => Compression::Xz,
            "zstd" | "zst" => Compression::Zstd,
            "bzip2" | "bz2" => Compression::Bzip2,
            other => bail!("unknown compression {other:?}; use none, gzip, xz, zstd or bzip2"),
        })
    }

    pub fn extension(self) -> &'static str {
        match self {
            Compression::None => "",
            Compression::Gzip => ".gz",
            Compression::Xz => ".xz",
            Compression::Zstd => ".zst",
            Compression::Bzip2 => ".bz2",
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
            Compression::Bzip2 => {
                let mut encoder =
                    bzip2::write::BzEncoder::new(Vec::new(), bzip2::Compression::best());
                encoder.write_all(data)?;
                encoder.finish()?
            }
        })
    }
}

/// A file added before the packaged files, such as `.PKGINFO`.
pub struct Leading {
    pub name: String,
    pub data: Vec<u8>,
    pub mode: u32,
}

/// What differs between the tar flavours of the package formats.
#[derive(Debug, Clone, Copy, Default)]
pub struct TarStyle {
    /// Write a pax header with `APK-TOOLS.checksum.SHA1` before every regular file, as Alpine packages do.
    pub apk_checksums: bool,
    /// Leave out the two zero blocks that end a tar. Alpine joins several tar streams into one package and the first
    /// ones must not end.
    pub open_ended: bool,
}

/// A tar of the entries with `prefix` before every path (`./` for deb, empty for Arch). Paths lose their leading `/`.
/// Ghost entries are not carried. Owners are written by name, with the numeric id 0.
pub fn tar(entries: &[Entry], prefix: &str, mtime: u64, leading: &[Leading]) -> Result<Vec<u8>> {
    tar_styled(entries, prefix, mtime, leading, TarStyle::default())
}

pub fn tar_styled(
    entries: &[Entry],
    prefix: &str,
    mtime: u64,
    leading: &[Leading],
    style: TarStyle,
) -> Result<Vec<u8>> {
    let mut builder = tar::Builder::new(Vec::new());
    builder.mode(tar::HeaderMode::Complete);
    let header = |mode: u32,
                  kind: tar::EntryType,
                  size: u64,
                  owner: &str,
                  group: &str|
     -> Result<tar::Header> {
        let mut header = if style.apk_checksums || style.open_ended {
            tar::Header::new_ustar()
        } else {
            tar::Header::new_gnu()
        };
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
                if style.apk_checksums {
                    let mut sha1 = sha1::Sha1::new();
                    io::copy(&mut std::fs::File::open(src)?, &mut HashWriter(&mut sha1))?;
                    let value = crate::hash::hex(&sha1.finalize());
                    append_pax(
                        &mut builder,
                        &name,
                        &format!("APK-TOOLS.checksum.SHA1={value}"),
                        mtime,
                    )?;
                }
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
    let mut bytes = builder.into_inner()?;
    if style.open_ended {
        let end = bytes.len() - 1024;
        debug_assert!(
            bytes[end..].iter().all(|b| *b == 0),
            "tar ends with two zero blocks"
        );
        bytes.truncate(end);
    }
    Ok(bytes)
}

/// A pax extended header before a file, named `dir/PaxHeaders/file` and written as a ustar entry like abuild does.
/// apk-tools reads its checksum record there.
fn append_pax(
    builder: &mut tar::Builder<Vec<u8>>,
    path: &str,
    record: &str,
    mtime: u64,
) -> Result<()> {
    // A pax record is "<length> <key>=<value>\n", and the length counts its own digits.
    let body = {
        let mut length = record.len() + 3;
        while format!("{length} {record}\n").len() != length {
            length = format!("{length} {record}\n").len();
        }
        format!("{length} {record}\n")
    };
    let (dir, base) = path
        .rsplit_once('/')
        .map(|(d, b)| (format!("{d}/"), b))
        .unwrap_or((String::new(), path));
    let mut header = tar::Header::new_ustar();
    header.set_entry_type(tar::EntryType::XHeader);
    header.set_size(body.len() as u64);
    header.set_mode(0o644);
    header.set_mtime(mtime);
    header.set_uid(0);
    header.set_gid(0);
    header.set_path(format!("{dir}PaxHeaders/{base}"))?;
    header.set_cksum();
    builder.append(&header, body.as_bytes())?;
    Ok(())
}

/// Lets `io::copy` feed a digest.
struct HashWriter<'a, D: sha1::Digest>(&'a mut D);

impl<D: sha1::Digest> Write for HashWriter<'_, D> {
    fn write(&mut self, data: &[u8]) -> io::Result<usize> {
        self.0.update(data);
        Ok(data.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
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
        for name in ["gzip", "xz", "zstd", "bzip2", "none"] {
            let packed = Compression::parse(name).unwrap().compress(&data).unwrap();
            let restored = match name {
                "gzip" => io::Read::bytes(flate2::read::GzDecoder::new(packed.as_slice()))
                    .collect::<Result<Vec<_>, _>>()
                    .unwrap(),
                "xz" => io::Read::bytes(xz2::read::XzDecoder::new(packed.as_slice()))
                    .collect::<Result<Vec<_>, _>>()
                    .unwrap(),
                "zstd" => zstd::stream::decode_all(packed.as_slice()).unwrap(),
                "bzip2" => io::Read::bytes(bzip2::read::BzDecoder::new(packed.as_slice()))
                    .collect::<Result<Vec<_>, _>>()
                    .unwrap(),
                _ => packed.clone(),
            };
            assert_eq!(restored, data, "{name}");
        }
    }
}
