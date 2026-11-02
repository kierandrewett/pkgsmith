//! Apple disk images (`.dmg`): a UDIF image holding one HFS+ volume with the files of the package.
//!
//! The image is laid out the way `hdiutil` lays out a disk image with one Apple partition map:
//! - sector 0: the Driver Descriptor Map (`ER` signature), one sector;
//! - sectors 1 to 63: the Apple Partition Map, with an entry for the map itself and one for the volume;
//! - sector 64 on: the HFS+ volume, with the files written into it.
//!
//! The volume is written by the `hfsplus` module, a clean-room HFS+ writer built from Apple TN1150. The image wrapper, with its
//! zlib block tables and `koly` trailer, is written with `udif`, whose trailer fields are then moved to the offsets of
//! the UDIF specification (see `fix_koly_layout`). Compression is zlib by default, or none.
//!
//! References:
//! - Apple TN1150, HFS Plus Volume Format: <https://developer.apple.com/library/archive/technotes/tn/tn1150.html>
//! - Apple partition maps and the UDIF trailer, as written by libdmg-hfsplus: <https://github.com/planetbeing/libdmg-hfsplus>
//!
//! Limits of the format as written here:
//! - the files are read into memory: the volume and the image each exist once in full while the image is written;
//! - the volume's own create and modify dates are the package's modification time;
//! - owners are written as uid 0 and gid 0, and symbolic links are refused. Names are ASCII only: a name with other characters, or two names in one folder that differ only in ASCII case, is an error that names the path.

use crate::hfsplus;
use anyhow::{Context, Result, bail};
use pkgsmith_plugin::entry::{self, Data};
use pkgsmith_plugin::version::Version;
use pkgsmith_plugin::{Entry, Host, Package};
use serde::Deserialize;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use udif::CompressionMethod;

const SECTOR: u64 = 512;
/// The allocation block of the HFS+ volume.
const BLOCK: u32 = hfsplus::BLOCK_SIZE;
const MIB: u64 = 1 << 20;
/// The sectors of the Apple Partition Map: sector 1 to 63.
const APM_SECTORS: u32 = 63;
/// The first sector of the HFS+ volume.
const HFS_START: u32 = 1 + APM_SECTORS;
/// The longest volume name: the partition name field is 32 bytes with a NUL.
const MAX_VOLUME_NAME: usize = 31;

/// The `dmg:` section of the manifest.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct Options {
    /// A folder of the volume that receives every entry: with `/Applications`, `/usr/bin/hi` is written as
    /// `Applications/usr/bin/hi`. Must start with `/`. Default: none, so the volume mirrors the installed system.
    prefix: Option<String>,
    /// The name of the volume, as the Finder shows it. At most 31 bytes, with no `:` and no NUL. Default: the
    /// package name.
    volume_name: Option<String>,
    /// `zlib` (default) or `none`.
    compression: Option<String>,
}

/// The options after they are checked, shared by the file name and the build.
struct Checked {
    compression: CompressionMethod,
    volume: String,
    /// The folder of the volume that receives the entries, without a trailing `/`. Empty for the root.
    prefix: String,
}

fn checked(package: &Package) -> Result<Checked> {
    let options: Options = package.options()?;
    let compression = match options.compression.as_deref().unwrap_or("zlib") {
        "zlib" => CompressionMethod::Zlib,
        "none" => CompressionMethod::Raw,
        other => bail!("compression {other:?} must be zlib or none"),
    };
    let volume = options.volume_name.unwrap_or_else(|| package.name.clone());
    if volume.is_empty() || volume.len() > MAX_VOLUME_NAME {
        bail!("volume_name {volume:?} must be 1 to {MAX_VOLUME_NAME} bytes long");
    }
    if volume.contains([':', '\0']) {
        bail!("volume_name {volume:?} must not hold ':' or a NUL");
    }
    let prefix = match options.prefix {
        None => String::new(),
        Some(prefix) if prefix.starts_with('/') => prefix.trim_end_matches('/').to_string(),
        Some(prefix) => bail!("prefix {prefix:?} must start with /"),
    };
    Version::parse(&package.version)?;
    Ok(Checked {
        compression,
        volume,
        prefix,
    })
}

/// `name-version-release-arch.dmg`.
pub fn file_name(package: &Package) -> Result<String> {
    checked(package)?;
    Ok(format!(
        "{}-{}-{}-{}.dmg",
        package.name, package.version, package.release, package.arch
    ))
}

/// Says, once per field, which manifest fields the format cannot carry.
fn warn_unexpressed(package: &Package, host: &Host) {
    let fields = [
        ("description", !package.description.is_empty()),
        ("maintainer", package.maintainer.is_some()),
        ("homepage", package.homepage.is_some()),
        ("license", package.license.is_some()),
        ("depends", !package.depends.is_empty()),
        ("recommends", !package.recommends.is_empty()),
        ("suggests", !package.suggests.is_empty()),
        ("conflicts", !package.conflicts.is_empty()),
        ("replaces", !package.replaces.is_empty()),
        ("provides", !package.provides.is_empty()),
    ];
    for (field, present) in fields {
        if present {
            host.warn(&format!(
                "a DMG has no {field} field, so {field} is not written"
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
        host.warn("a DMG runs no install or removal scripts, so the scripts are not written");
    }
}

/// The entries that the volume holds: ghosts dropped, the prefix applied, and every parent folder present.
fn volume_entries(package: &Package, prefix: &str, host: &Host) -> Result<Vec<Entry>> {
    let mut ghosts = 0usize;
    let mut configs = 0usize;
    let mut owned = false;
    let mut kept = Vec::with_capacity(package.entries.len());
    for entry in &package.entries {
        match entry.data {
            Data::Ghost => {
                ghosts += 1;
                continue;
            }
            Data::File { .. } | Data::Dir | Data::Symlink { .. } => {}
        }
        if entry.config {
            configs += 1;
        }
        if !matches!(entry.owner.as_str(), "root" | "wheel")
            || !matches!(entry.group.as_str(), "root" | "wheel")
        {
            owned = true;
        }
        let mut kept_entry = entry.clone();
        kept_entry.path = format!("{prefix}{}", entry.path);
        kept.push(kept_entry);
    }
    if ghosts > 0 {
        host.warn(&format!(
            "{ghosts} ghost entries are owned by the package and are not written"
        ));
    }
    if configs > 0 {
        host.warn("a DMG has no configuration files, so the config flag is not written");
    }
    if owned {
        host.warn("a DMG holds no owners: every file is written as uid 0 and gid 0");
    }
    Ok(entry::with_parents(&kept))
}

/// The size of the volume: the data in whole blocks and the file system overhead, with a quarter spare, in whole MiB
/// and at least 16 MiB.
fn volume_size(entries: &[Entry]) -> u64 {
    let block = u64::from(BLOCK);
    let data: u64 = entries
        .iter()
        .map(|e| match &e.data {
            // A link's target is its data fork, which takes at least one block.
            Data::Symlink { target } => (target.len() as u64).div_ceil(block) * block,
            _ => e.size().div_ceil(block) * block,
        })
        .sum();
    let overhead = entries.len() as u64 * 8 * 1024 + 8 * MIB;
    let total = (data + overhead) * 5 / 4;
    total.div_ceil(MIB).max(16) * MIB
}

/// The Driver Descriptor Map: sector 0, signature `ER`, the block size and the size of the whole disk in sectors.
fn driver_descriptor_map(volume_sectors: u32) -> Result<Vec<u8>> {
    let total = HFS_START
        .checked_add(volume_sectors)
        .context("the volume is too large for a disk image")?;
    let mut block = vec![0u8; SECTOR as usize];
    block[0..2].copy_from_slice(&0x4552u16.to_be_bytes());
    block[2..4].copy_from_slice(&(SECTOR as u16).to_be_bytes());
    block[4..8].copy_from_slice(&total.to_be_bytes());
    Ok(block)
}

/// One Apple Partition Map entry: where the partition starts, how many sectors it has, its name, its type and its
/// status.
struct MapEntry<'a> {
    start: u32,
    sectors: u32,
    name: &'a str,
    kind: &'a str,
    status: u32,
}

/// Writes a map entry into one sector. `map_entries` is the number of entries in the map.
fn write_map_entry(block: &mut [u8], map_entries: u32, entry: &MapEntry) {
    block[0..2].copy_from_slice(&0x504Du16.to_be_bytes());
    block[4..8].copy_from_slice(&map_entries.to_be_bytes());
    block[8..12].copy_from_slice(&entry.start.to_be_bytes());
    block[12..16].copy_from_slice(&entry.sectors.to_be_bytes());
    put_text(&mut block[16..48], entry.name);
    put_text(&mut block[48..80], entry.kind);
    block[84..88].copy_from_slice(&entry.sectors.to_be_bytes());
    block[88..92].copy_from_slice(&entry.status.to_be_bytes());
}

/// Text in a fixed field, padded with NULs.
fn put_text(field: &mut [u8], text: &str) {
    let n = text.len().min(field.len());
    field[..n].copy_from_slice(&text.as_bytes()[..n]);
}

/// The Apple Partition Map: the entry of the map itself, then the HFS+ volume.
fn partition_map(volume_sectors: u32, volume: &str) -> Vec<u8> {
    let mut map = vec![0u8; APM_SECTORS as usize * SECTOR as usize];
    let (map_block, rest) = map.split_at_mut(SECTOR as usize);
    write_map_entry(
        map_block,
        2,
        &MapEntry {
            start: 1,
            sectors: APM_SECTORS,
            name: "Apple",
            kind: "Apple_partition_map",
            status: 0x33,
        },
    );
    write_map_entry(
        &mut rest[..SECTOR as usize],
        2,
        &MapEntry {
            start: HFS_START,
            sectors: volume_sectors,
            name: volume,
            kind: "Apple_HFS",
            status: 0x4000_0033,
        },
    );
    map
}

/// Writes the UDIF image: the Driver Descriptor Map, the partition map and the volume.
fn write_image(
    path: &Path,
    volume: &[u8],
    name: &str,
    compression: CompressionMethod,
) -> Result<()> {
    let volume_sectors = u32::try_from(volume.len() as u64 / SECTOR)
        .context("the volume is too large for a disk image")?;
    let mut writer = udif::create(path)
        .with_context(|| format!("cannot create {}", path.display()))?
        .compression(compression);
    writer.add_partition(
        "Driver Descriptor Map (DDM : 0)",
        &driver_descriptor_map(volume_sectors)?,
    )?;
    writer.add_partition(
        "Apple (Apple_partition_map : 1)",
        &partition_map(volume_sectors, name),
    )?;
    writer.add_partition(&format!("{name} (Apple_HFS : 2)"), volume)?;
    writer.finish()?;
    fix_koly_layout(path, u64::from(HFS_START) + u64::from(volume_sectors))?;
    Ok(())
}

/// The `koly` trailer is 512 bytes at the end of the file.
const KOLY_SIZE: usize = 512;
/// Where `udif` 0.4.2 writes the master checksum, the image variant and the sector count. It keeps the reserved area at
/// 64 bytes instead of 120, so these three fields sit 56 bytes before the offsets of the UDIF specification.
const UDIF_MASTER_CHECKSUM: usize = 296;
const UDIF_IMAGE_VARIANT: usize = 432;
const UDIF_SECTOR_COUNT: usize = 436;
/// The offsets the UDIF specification gives those fields. 7-Zip reads the sector count at 0x1EC and reports a header
/// error when it does not match the partitions.
const KOLY_MASTER_CHECKSUM: usize = 0x160;
const KOLY_IMAGE_VARIANT: usize = 0x1E8;
const KOLY_SECTOR_COUNT: usize = 0x1EC;

/// Moves the fields `udif` writes to the offsets of the UDIF specification. Every other field of the trailer has the same
/// offset in both layouts. Refuses to change the trailer unless the sector count `udif` wrote is `sector_count`, so a
/// `udif` that writes the specified layout is detected instead of corrupted.
fn fix_koly_layout(path: &Path, sector_count: u64) -> Result<()> {
    let mut file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(path)
        .with_context(|| format!("cannot reopen {} to finish its trailer", path.display()))?;
    let trailer_at = file.seek(SeekFrom::End(-(KOLY_SIZE as i64)))?;
    let mut written = [0u8; KOLY_SIZE];
    file.read_exact(&mut written)?;
    if &written[..4] != b"koly" {
        bail!("{} has no koly trailer", path.display());
    }
    let written_sectors = u64::from_be_bytes(
        written[UDIF_SECTOR_COUNT..UDIF_SECTOR_COUNT + 8]
            .try_into()
            .context("the koly trailer is truncated")?,
    );
    if written_sectors != sector_count {
        bail!(
            "udif wrote {written_sectors} sectors where {sector_count} were expected, so its koly layout is not the one this plugin fixes"
        );
    }
    let mut koly = [0u8; KOLY_SIZE];
    koly[..UDIF_MASTER_CHECKSUM].copy_from_slice(&written[..UDIF_MASTER_CHECKSUM]);
    koly[KOLY_MASTER_CHECKSUM..KOLY_IMAGE_VARIANT]
        .copy_from_slice(&written[UDIF_MASTER_CHECKSUM..UDIF_IMAGE_VARIANT]);
    koly[KOLY_IMAGE_VARIANT..KOLY_SECTOR_COUNT]
        .copy_from_slice(&written[UDIF_IMAGE_VARIANT..UDIF_SECTOR_COUNT]);
    koly[KOLY_SECTOR_COUNT..KOLY_SECTOR_COUNT + 8]
        .copy_from_slice(&written[UDIF_SECTOR_COUNT..UDIF_SECTOR_COUNT + 8]);
    file.seek(SeekFrom::Start(trailer_at))?;
    file.write_all(&koly)?;
    Ok(())
}

pub fn build(package: &Package, host: &Host) -> Result<PathBuf> {
    let checked = checked(package)?;
    let file = package.output(&file_name(package)?);
    warn_unexpressed(package, host);
    let entries = volume_entries(package, &checked.prefix, host)?;
    let image = hfsplus::write_volume(
        &entries,
        volume_size(&entries),
        &checked.volume,
        package.mtime,
    )?;
    if let Err(error) = write_image(&file, &image, &checked.volume, checked.compression) {
        // Never leave a half-written image behind.
        let _ = std::fs::remove_file(&file);
        return Err(error);
    }
    Ok(file)
}
