//! Writes one HFS Plus volume (not HFSX) into bytes, from Apple TN1150, "HFS Plus Volume Format".
//!
//! Layout, in allocation blocks of `BLOCK_SIZE` bytes (doubled only while the volume would have more than 2^32 blocks):
//! - block 0: the boot blocks (zero) and the volume header at byte 1024;
//! - the allocation file (the bitmap), the extents overflow file, then each file's data, then the catalog file;
//! - the last block holds the alternate volume header, 1024 bytes from the end of the volume.
//!
//! Every fork is one contiguous extent, so the extents overflow file is an empty B-tree (its header node alone). The
//! attributes file has no extent, which TN1150 defines as "the attributes file does not exist". The catalog is a B-tree
//! with as many index levels as its leaves need.
//!
//! Names are ASCII only. HFS Plus compares names after Unicode decomposition and case folding (TN1150, "String
//! Comparison Algorithm"). This writer does not implement that table, so it folds ASCII letters only and refuses every
//! other name with an error that names the path. Two names in one folder that differ only in ASCII case are also refused.
//!
//! Owners are written as uid 0 and gid 0. Every date is the package's modification time.

use anyhow::{Context, Result, bail, ensure};
use pkgsmith_plugin::{Data, Entry};
use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::Read;
use std::ops::Range;
use std::path::Path;

/// The allocation block size. The catalog and extents B-trees use `NODE_SIZE` nodes, which the blocks hold whole.
pub const BLOCK_SIZE: u32 = 4096;
/// The B-tree node size, the same as the block size. TN1150 requires at least 4 KB nodes for the catalog.
const NODE_SIZE: usize = 4096;
const HEADER_AT: usize = 1024;
const HEADER_LEN: usize = 512;
const ROOT_PARENT_ID: u32 = 1;
const ROOT_FOLDER_ID: u32 = 2;
const FIRST_USER_CNID: u32 = 16;
/// Seconds from 1 January 1904, the HFS Plus epoch, to 1 January 1970.
const HFS_EPOCH_OFFSET: u64 = 2_082_844_800;
const MAX_NAME_LEN: usize = 255;
const SIGNATURE: u16 = 0x482B;
const VERSION: u16 = 4;
/// `kHFSVolumeUnmountedBit` (bit 8): the volume was flushed and unmounted.
const ATTR_UNMOUNTED: u32 = 1 << 8;
/// `lastMountedVersion`: identifies this writer, as "PKSG".
const LAST_MOUNTED_VERSION: u32 = 0x504B_5347;
/// `encodingsBitmap` bit 0: MacRoman.
const ENCODING_MAC_ROMAN: u64 = 1;
const BTREE_BIG_KEYS: u32 = 0x0002;
const BTREE_VARIABLE_INDEX_KEYS: u32 = 0x0004;
const CATALOG_MAX_KEY: u16 = 516;
const EXTENTS_MAX_KEY: u16 = 10;
const NODE_LEAF: u8 = 0xFF;
const NODE_INDEX: u8 = 0;
const NODE_HEADER: u8 = 1;
const NODE_DESCRIPTOR: usize = 14;
const HEADER_RECORD_LEN: usize = 106;
const USER_RECORD_LEN: usize = 128;
/// The header node's map record: the space after its descriptor, its three records' offsets and the free offset.
const HEADER_MAP_LEN: usize = NODE_SIZE - NODE_DESCRIPTOR - HEADER_RECORD_LEN - USER_RECORD_LEN - 8;
const REC_FOLDER: u16 = 1;
const REC_FILE: u16 = 2;
const REC_FOLDER_THREAD: u16 = 3;
const REC_FILE_THREAD: u16 = 4;
/// `kHFSFileThreadExistsMask`.
const FILE_THREAD_EXISTS: u16 = 0x0002;
/// BSD file-type bits of `fileMode`: directory and regular file.
const BSD_DIR: u16 = 0o040000;
const BSD_FILE: u16 = 0o100000;
const ROOT_MODE: u16 = 0o755;

/// Writes the volume of `size` bytes holding `entries`, named `name`, with every date set to `mtime` (unix seconds).
/// The entries are the volume's paths with a leading `/`, every parent folder present, and no symbolic links.
pub fn write_volume(entries: &[Entry], size: u64, name: &str, mtime: u64) -> Result<Vec<u8>> {
    check_name(&format!("the volume name {name:?}"), name)?;
    let block = block_size(size)?;
    let total = size / block;
    ensure!(total >= 4, "the volume is too small: {size} bytes");
    let (items, next_cnid) = number(entries)?;

    // The blocks, in order: the bitmap, the extents file, each file's data, then the catalog. The last block holds the
    // alternate volume header, so the allocator stops one block short of the end.
    let mut alloc = Allocator {
        next: 1,
        end: total - 1,
    };
    let bitmap_bytes = total.div_ceil(8);
    let bitmap_blocks = bitmap_bytes.div_ceil(block);
    let bitmap = Fork {
        logical: bitmap_bytes,
        blocks: to_u32(bitmap_blocks, "the bitmap")?,
        start: alloc.take(bitmap_blocks)?,
    };
    let extents_blocks = (NODE_SIZE as u64).div_ceil(block);
    let extents_fork = Fork {
        logical: NODE_SIZE as u64,
        blocks: to_u32(extents_blocks, "the extents file")?,
        start: alloc.take(extents_blocks)?,
    };
    let mut forks = Vec::with_capacity(items.len());
    for item in &items {
        forks.push(match item.kind {
            Kind::Dir => Fork::default(),
            Kind::File { size, .. } => {
                let blocks = size.div_ceil(block);
                let blocks = u32::try_from(blocks).map_err(|_| {
                    anyhow::anyhow!("{}: the file is too large for HFS Plus", item.path)
                })?;
                let start = if blocks == 0 {
                    0
                } else {
                    alloc.take(u64::from(blocks))?
                };
                Fork {
                    logical: size,
                    blocks,
                    start,
                }
            }
        });
    }

    let records = catalog_records(&items, &forks, name, mtime);
    let record_bytes: Vec<Vec<u8>> = records.iter().map(Record::bytes).collect();
    let catalog_nodes = build_tree(
        record_bytes,
        CATALOG_MAX_KEY,
        BTREE_BIG_KEYS | BTREE_VARIABLE_INDEX_KEYS,
    )?;
    let catalog_len = (catalog_nodes.len() * NODE_SIZE) as u64;
    let catalog_blocks = catalog_len.div_ceil(block);
    let catalog_fork = Fork {
        logical: catalog_len,
        blocks: to_u32(catalog_blocks, "the catalog file")?,
        start: alloc.take(catalog_blocks)?,
    };
    let extents_nodes = build_tree(Vec::new(), EXTENTS_MAX_KEY, BTREE_BIG_KEYS)?;

    let used = alloc.next;
    let header = VolumeHeader {
        block,
        total,
        free: total - used - 1,
        next_alloc: used,
        file_count: count(&items, |k| matches!(k, Kind::File { .. }))?,
        folder_count: count(&items, |k| matches!(k, Kind::Dir))?,
        next_cnid,
        date: hfs_date(mtime),
        allocation: bitmap,
        extents: extents_fork,
        catalog: catalog_fork,
    }
    .bytes();

    let mut image = vec![0u8; size as usize];
    for (item, fork) in items.iter().zip(&forks) {
        if let Kind::File { src, size } = item.kind
            && size > 0
        {
            let at = fork.start as usize * block as usize;
            read_into(&mut image, at, src, size, item.path)?;
        }
    }
    put_nodes(&mut image, catalog_fork.start, block, &catalog_nodes);
    put_nodes(&mut image, extents_fork.start, block, &extents_nodes);
    put(
        &mut image,
        bitmap.start as usize * block as usize,
        &bitmap_map(total, used),
    );
    put(&mut image, HEADER_AT, &header);
    put(&mut image, size as usize - HEADER_AT, &header);
    Ok(image)
}

/// The allocation block size: `BLOCK_SIZE`, doubled while the volume would have more than 2^32 blocks.
fn block_size(size: u64) -> Result<u64> {
    let mut block = u64::from(BLOCK_SIZE);
    while size / block > u64::from(u32::MAX) {
        block *= 2;
    }
    ensure!(
        size.is_multiple_of(block),
        "the volume size {size} is not a multiple of its {block}-byte block"
    );
    Ok(block)
}

fn to_u32(value: u64, what: &str) -> Result<u32> {
    u32::try_from(value).with_context(|| format!("{what} is too large for HFS Plus"))
}

fn count(items: &[Item], keep: impl Fn(&Kind) -> bool) -> Result<u32> {
    to_u32(
        items.iter().filter(|i| keep(&i.kind)).count() as u64,
        "the volume",
    )
}

/// Hands out contiguous allocation blocks, from `next` up to `end` (exclusive).
struct Allocator {
    next: u64,
    end: u64,
}

impl Allocator {
    fn take(&mut self, blocks: u64) -> Result<u32> {
        let start = self.next;
        let next = start + blocks;
        ensure!(
            next <= self.end,
            "the volume is too small for its files: they need more than {} allocation blocks",
            self.end
        );
        self.next = next;
        to_u32(start, "a block number")
    }
}

/// One item of the volume: a folder or a file, with its catalog number and its parent's.
struct Item<'a> {
    path: &'a str,
    name: &'a str,
    cnid: u32,
    parent: u32,
    mode: u16,
    kind: Kind<'a>,
}

#[derive(Clone, Copy)]
enum Kind<'a> {
    Dir,
    File { src: &'a Path, size: u64 },
}

/// Checks the names and gives every entry a catalog number, from `FIRST_USER_CNID` in path order. Returns the items
/// and the next free catalog number.
fn number(entries: &[Entry]) -> Result<(Vec<Item<'_>>, u32)> {
    let mut sorted: Vec<&Entry> = entries.iter().collect();
    sorted.sort_by(|a, b| a.path.cmp(&b.path));

    let mut folders: HashSet<&str> = HashSet::from([""]);
    let mut siblings: HashSet<(&str, String)> = HashSet::new();
    let mut split: Vec<(&Entry, &str, &str)> = Vec::with_capacity(sorted.len());
    for entry in sorted {
        let rel = entry.path.trim_start_matches('/');
        let (parent, name) = rel.rsplit_once('/').unwrap_or(("", rel));
        check_name(&entry.path, name)?;
        ensure!(
            siblings.insert((parent, name.to_ascii_lowercase())),
            "{}: another entry in the same folder has this name, ignoring ASCII case",
            entry.path
        );
        if matches!(entry.data, Data::Dir) {
            folders.insert(rel);
        }
        split.push((entry, parent, name));
    }

    let mut ids: HashMap<&str, u32> = HashMap::from([("", ROOT_FOLDER_ID)]);
    let mut next = FIRST_USER_CNID;
    for (entry, _, _) in &split {
        ids.insert(entry.path.trim_start_matches('/'), next);
        next += 1;
    }

    let mut items = Vec::with_capacity(split.len());
    for (entry, parent, name) in split {
        ensure!(
            folders.contains(parent),
            "{}: its parent is not a folder of the volume",
            entry.path
        );
        let kind = match &entry.data {
            Data::Dir => Kind::Dir,
            Data::File { src, size } => Kind::File {
                src: src.as_path(),
                size: *size,
            },
            Data::Symlink { .. } => bail!(
                "{}: a symbolic link cannot be written into a DMG, because the HFS+ writer does not write symbolic links",
                entry.path
            ),
            Data::Ghost => bail!("{}: a ghost entry is not part of the volume", entry.path),
        };
        items.push(Item {
            path: &entry.path,
            name,
            cnid: ids[entry.path.trim_start_matches('/')],
            parent: ids[parent],
            mode: (entry.mode & 0o7777) as u16,
            kind,
        });
    }
    Ok((items, next))
}

/// Refuses a name HFS Plus cannot hold, or one this writer cannot compare exactly. `path` names the entry in the error.
fn check_name(path: &str, name: &str) -> Result<()> {
    ensure!(
        !name.is_empty() && name != "." && name != "..",
        "{path}: the name is empty or a dot name, which HFS Plus does not take"
    );
    ensure!(
        name.is_ascii(),
        "{path}: the name is not ASCII. This writer writes ASCII names only, because HFS Plus compares other names after Unicode normalisation and case folding, which it does not implement"
    );
    ensure!(!name.contains('\0'), "{path}: the name holds a NUL");
    ensure!(
        name.len() <= MAX_NAME_LEN,
        "{path}: the name is {} characters long, and HFS Plus allows {MAX_NAME_LEN}",
        name.len()
    );
    Ok(())
}

fn name_units(name: &str) -> Vec<u16> {
    name.bytes().map(u16::from).collect()
}

/// The catalog key: the parent's catalog number and the name, in UTF-16 units.
#[derive(Clone)]
struct CatKey {
    parent: u32,
    name: Vec<u16>,
}

/// The order of the catalog: the parent, then the name with ASCII letters folded to lower case. A name that starts
/// with another sorts after it.
impl Ord for CatKey {
    fn cmp(&self, other: &Self) -> Ordering {
        self.parent.cmp(&other.parent).then_with(|| {
            self.name
                .iter()
                .map(|&u| fold(u))
                .cmp(other.name.iter().map(|&u| fold(u)))
        })
    }
}

impl PartialOrd for CatKey {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl PartialEq for CatKey {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == Ordering::Equal
    }
}

impl Eq for CatKey {}

fn fold(unit: u16) -> u16 {
    if (u16::from(b'A')..=u16::from(b'Z')).contains(&unit) {
        unit + 32
    } else {
        unit
    }
}

struct Record {
    key: CatKey,
    data: Vec<u8>,
}

impl Record {
    fn bytes(&self) -> Vec<u8> {
        let mut out = key_bytes(&self.key);
        out.extend_from_slice(&self.data);
        out
    }
}

/// The catalog key: `keyLength`, `parentID`, `nodeName` (length, then UTF-16 units). `keyLength` excludes itself.
fn key_bytes(key: &CatKey) -> Vec<u8> {
    let key_len = 6 + 2 * key.name.len();
    let mut out = Vec::with_capacity(2 + key_len);
    out.extend_from_slice(&(key_len as u16).to_be_bytes());
    out.extend_from_slice(&key.parent.to_be_bytes());
    out.extend_from_slice(&(key.name.len() as u16).to_be_bytes());
    for unit in &key.name {
        out.extend_from_slice(&unit.to_be_bytes());
    }
    out
}

/// Every catalog record, sorted by key: the root folder, then each item's record and its thread record.
fn catalog_records(items: &[Item], forks: &[Fork], volume: &str, mtime: u64) -> Vec<Record> {
    let date = hfs_date(mtime);
    let mut valence: HashMap<u32, u32> = HashMap::new();
    for item in items {
        *valence.entry(item.parent).or_default() += 1;
    }
    let mut records = Vec::with_capacity(items.len() * 2 + 2);
    for (item, fork) in items.iter().zip(forks) {
        let name = name_units(item.name);
        let (data, thread) = match item.kind {
            Kind::Dir => (
                folder_record(
                    item.cnid,
                    valence.get(&item.cnid).copied().unwrap_or(0),
                    item.mode,
                    date,
                ),
                REC_FOLDER_THREAD,
            ),
            Kind::File { .. } => (
                file_record(item.cnid, item.mode, date, *fork),
                REC_FILE_THREAD,
            ),
        };
        records.push(Record {
            key: CatKey {
                parent: item.parent,
                name: name.clone(),
            },
            data,
        });
        records.push(Record {
            key: CatKey {
                parent: item.cnid,
                name: Vec::new(),
            },
            data: thread_record(thread, item.parent, &name),
        });
    }
    let volume_name = name_units(volume);
    records.push(Record {
        key: CatKey {
            parent: ROOT_PARENT_ID,
            name: volume_name.clone(),
        },
        data: folder_record(
            ROOT_FOLDER_ID,
            valence.get(&ROOT_FOLDER_ID).copied().unwrap_or(0),
            ROOT_MODE,
            date,
        ),
    });
    records.push(Record {
        key: CatKey {
            parent: ROOT_FOLDER_ID,
            name: Vec::new(),
        },
        data: thread_record(REC_FOLDER_THREAD, ROOT_PARENT_ID, &volume_name),
    });
    records.sort_by(|a, b| a.key.cmp(&b.key));
    records
}

/// `HFSPlusBSDInfo`, 16 bytes: owner and group 0, then the file type and permission bits in `fileMode`.
fn bsd_info(type_bits: u16, mode: u16) -> [u8; 16] {
    let mut out = [0u8; 16];
    out[10..12].copy_from_slice(&(type_bits | (mode & 0o7777)).to_be_bytes());
    out
}

/// `HFSPlusCatalogFolder`, 88 bytes.
fn folder_record(id: u32, valence: u32, mode: u16, date: u32) -> Vec<u8> {
    let mut out = Vec::with_capacity(88);
    out.extend_from_slice(&REC_FOLDER.to_be_bytes());
    out.extend_from_slice(&0u16.to_be_bytes());
    out.extend_from_slice(&valence.to_be_bytes());
    out.extend_from_slice(&id.to_be_bytes());
    for _ in 0..4 {
        out.extend_from_slice(&date.to_be_bytes());
    }
    out.extend_from_slice(&0u32.to_be_bytes());
    out.extend_from_slice(&bsd_info(BSD_DIR, mode));
    out.extend_from_slice(&[0u8; 16]);
    out.extend_from_slice(&[0u8; 16]);
    out.extend_from_slice(&[0u8; 8]);
    out
}

/// `HFSPlusCatalogFile`, 248 bytes: the data fork, and an empty resource fork.
fn file_record(id: u32, mode: u16, date: u32, data: Fork) -> Vec<u8> {
    let mut out = Vec::with_capacity(248);
    out.extend_from_slice(&REC_FILE.to_be_bytes());
    out.extend_from_slice(&FILE_THREAD_EXISTS.to_be_bytes());
    out.extend_from_slice(&0u32.to_be_bytes());
    out.extend_from_slice(&id.to_be_bytes());
    for _ in 0..4 {
        out.extend_from_slice(&date.to_be_bytes());
    }
    out.extend_from_slice(&0u32.to_be_bytes());
    out.extend_from_slice(&bsd_info(BSD_FILE, mode));
    out.extend_from_slice(&[0u8; 16]);
    out.extend_from_slice(&[0u8; 16]);
    out.extend_from_slice(&[0u8; 8]);
    out.extend_from_slice(&data.bytes());
    out.extend_from_slice(&[0u8; 80]);
    out
}

/// A thread record: the parent's catalog number and the name of the item it stands for.
fn thread_record(kind: u16, parent: u32, name: &[u16]) -> Vec<u8> {
    let mut out = Vec::with_capacity(10 + 2 * name.len());
    out.extend_from_slice(&kind.to_be_bytes());
    out.extend_from_slice(&0u16.to_be_bytes());
    out.extend_from_slice(&parent.to_be_bytes());
    out.extend_from_slice(&(name.len() as u16).to_be_bytes());
    for unit in name {
        out.extend_from_slice(&unit.to_be_bytes());
    }
    out
}

/// `HFSPlusForkData`, 80 bytes, with one extent that holds the whole fork.
#[derive(Clone, Copy, Default)]
struct Fork {
    logical: u64,
    blocks: u32,
    start: u32,
}

impl Fork {
    fn bytes(self) -> [u8; 80] {
        let mut out = [0u8; 80];
        out[0..8].copy_from_slice(&self.logical.to_be_bytes());
        out[12..16].copy_from_slice(&self.blocks.to_be_bytes());
        out[16..20].copy_from_slice(&self.start.to_be_bytes());
        out[20..24].copy_from_slice(&self.blocks.to_be_bytes());
        out
    }
}

struct VolumeHeader {
    block: u64,
    total: u64,
    free: u64,
    next_alloc: u64,
    file_count: u32,
    folder_count: u32,
    next_cnid: u32,
    date: u32,
    allocation: Fork,
    extents: Fork,
    catalog: Fork,
}

impl VolumeHeader {
    /// `HFSPlusVolumeHeader`, 512 bytes, with the fields at the offsets of TN1150.
    fn bytes(&self) -> [u8; HEADER_LEN] {
        let block = self.block as u32;
        let mut out = [0u8; HEADER_LEN];
        put(&mut out, 0, &SIGNATURE.to_be_bytes());
        put(&mut out, 2, &VERSION.to_be_bytes());
        put(&mut out, 4, &ATTR_UNMOUNTED.to_be_bytes());
        put(&mut out, 8, &LAST_MOUNTED_VERSION.to_be_bytes());
        put(&mut out, 16, &self.date.to_be_bytes());
        put(&mut out, 20, &self.date.to_be_bytes());
        put(&mut out, 28, &self.date.to_be_bytes());
        put(&mut out, 32, &self.file_count.to_be_bytes());
        put(&mut out, 36, &self.folder_count.to_be_bytes());
        put(&mut out, 40, &block.to_be_bytes());
        put(&mut out, 44, &(self.total as u32).to_be_bytes());
        put(&mut out, 48, &(self.free as u32).to_be_bytes());
        put(&mut out, 52, &(self.next_alloc as u32).to_be_bytes());
        put(&mut out, 56, &block.to_be_bytes());
        put(&mut out, 60, &block.to_be_bytes());
        put(&mut out, 64, &self.next_cnid.to_be_bytes());
        put(&mut out, 68, &1u32.to_be_bytes());
        put(&mut out, 72, &ENCODING_MAC_ROMAN.to_be_bytes());
        put(&mut out, 112, &self.allocation.bytes());
        put(&mut out, 192, &self.extents.bytes());
        put(&mut out, 272, &self.catalog.bytes());
        out
    }
}

/// HFS Plus date: seconds since 1904, saturating at the largest value.
fn hfs_date(unix: u64) -> u32 {
    u32::try_from(unix.saturating_add(HFS_EPOCH_OFFSET)).unwrap_or(u32::MAX)
}

/// The allocation file: bit `n` (most significant bit first) is set when block `n` is in use. Blocks `0..used` are in
/// use, as is the alternate header in the last block. The bits past the end of the volume are set.
fn bitmap_map(total: u64, used: u64) -> Vec<u8> {
    let mut map = vec![0u8; total.div_ceil(8) as usize];
    for bit in 0..used {
        set_bit(&mut map, bit);
    }
    set_bit(&mut map, total - 1);
    for bit in total..(map.len() as u64 * 8) {
        set_bit(&mut map, bit);
    }
    map
}

fn set_bit(map: &mut [u8], bit: u64) {
    map[(bit / 8) as usize] |= 0x80 >> (bit % 8);
}

/// Reads exactly `size` bytes of `src` into the image at `at`. A file whose size changed while the volume was built is an
/// error.
fn read_into(image: &mut [u8], at: usize, src: &Path, size: u64, path: &str) -> Result<()> {
    let mut file =
        File::open(src).with_context(|| format!("{path}: cannot read {}", src.display()))?;
    file.read_exact(&mut image[at..at + size as usize])
        .with_context(|| format!("{path}: {} is shorter than its {size} bytes", src.display()))?;
    let mut probe = [0u8; 1];
    ensure!(
        file.read(&mut probe)? == 0,
        "{path}: {} is longer than its {size} bytes",
        src.display()
    );
    Ok(())
}

/// Writes the nodes of a B-tree file, one node after another, from allocation block `start`.
fn put_nodes(image: &mut [u8], start: u32, block: u64, nodes: &[Vec<u8>]) {
    let base = start as usize * block as usize;
    for (i, node) in nodes.iter().enumerate() {
        put(image, base + i * NODE_SIZE, node);
    }
}

fn put(out: &mut [u8], at: usize, bytes: &[u8]) {
    out[at..at + bytes.len()].copy_from_slice(bytes);
}

/// A B-tree header record: the tree's depth, root, leaves and keys, and its node count.
struct TreeHeader {
    depth: u16,
    root: u32,
    leaf_records: u32,
    first_leaf: u32,
    last_leaf: u32,
    max_key: u16,
    total_nodes: u32,
    attributes: u32,
}

/// The header node, node 0: the header record, a user-data record, and the map of the nodes in use.
fn header_node(header: &TreeHeader) -> Result<Vec<u8>> {
    ensure!(
        u64::from(header.total_nodes) <= HEADER_MAP_LEN as u64 * 8,
        "the B-tree has {} nodes, more than its header map holds",
        header.total_nodes
    );
    let mut record = Vec::with_capacity(HEADER_RECORD_LEN);
    record.extend_from_slice(&header.depth.to_be_bytes());
    record.extend_from_slice(&header.root.to_be_bytes());
    record.extend_from_slice(&header.leaf_records.to_be_bytes());
    record.extend_from_slice(&header.first_leaf.to_be_bytes());
    record.extend_from_slice(&header.last_leaf.to_be_bytes());
    record.extend_from_slice(&(NODE_SIZE as u16).to_be_bytes());
    record.extend_from_slice(&header.max_key.to_be_bytes());
    record.extend_from_slice(&header.total_nodes.to_be_bytes());
    record.extend_from_slice(&0u32.to_be_bytes());
    record.extend_from_slice(&0u16.to_be_bytes());
    record.extend_from_slice(&0u32.to_be_bytes());
    // btreeType 0 (kHFSBTreeType); keyCompareType 0, which TN1150 defines only for HFSX volumes.
    record.push(0);
    record.push(0);
    record.extend_from_slice(&header.attributes.to_be_bytes());
    record.extend_from_slice(&[0u8; 64]);
    debug_assert_eq!(record.len(), HEADER_RECORD_LEN);

    let mut map = vec![0u8; HEADER_MAP_LEN];
    for n in 0..header.total_nodes {
        map[(n / 8) as usize] |= 0x80 >> (n % 8);
    }
    Ok(node(
        0,
        0,
        NODE_HEADER,
        0,
        &[record, vec![0u8; USER_RECORD_LEN], map],
    ))
}

/// One node: its descriptor, its records, and the offset of each record and of the free space at the end.
fn node(fwd: u32, bwd: u32, kind: u8, height: u8, records: &[Vec<u8>]) -> Vec<u8> {
    let mut out = vec![0u8; NODE_SIZE];
    put(&mut out, 0, &fwd.to_be_bytes());
    put(&mut out, 4, &bwd.to_be_bytes());
    out[8] = kind;
    out[9] = height;
    put(&mut out, 10, &(records.len() as u16).to_be_bytes());
    let mut at = NODE_DESCRIPTOR;
    for (i, record) in records.iter().enumerate() {
        put(&mut out, at, record);
        put(
            &mut out,
            NODE_SIZE - 2 * (i + 1),
            &(at as u16).to_be_bytes(),
        );
        at += record.len();
    }
    put(
        &mut out,
        NODE_SIZE - 2 * (records.len() + 1),
        &(at as u16).to_be_bytes(),
    );
    out
}

/// Splits sorted records into runs that each fit one node, with their offset table.
fn group(records: &[Vec<u8>]) -> Result<Vec<Range<usize>>> {
    let mut groups = Vec::new();
    let (mut start, mut used) = (0usize, 0usize);
    for (i, record) in records.iter().enumerate() {
        ensure!(
            NODE_DESCRIPTOR + record.len() + 4 <= NODE_SIZE,
            "a B-tree record of {} bytes is too large for a {NODE_SIZE}-byte node",
            record.len()
        );
        let count = i - start;
        if count > 0 && NODE_DESCRIPTOR + used + record.len() + 2 * (count + 2) > NODE_SIZE {
            groups.push(start..i);
            start = i;
            used = 0;
        }
        used += record.len();
    }
    if start < records.len() {
        groups.push(start..records.len());
    }
    Ok(groups)
}

/// The index record for a child node: the child's first key, then the child's node number.
fn index_record(first: &[u8], child: u32) -> Vec<u8> {
    let key_len = 2 + usize::from(u16::from_be_bytes([first[0], first[1]]));
    let mut out = first[..key_len].to_vec();
    out.extend_from_slice(&child.to_be_bytes());
    out
}

/// A complete B-tree as its nodes, node 0 first. With no records the tree is its header node alone.
fn build_tree(records: Vec<Vec<u8>>, max_key: u16, attributes: u32) -> Result<Vec<Vec<u8>>> {
    let leaf_records = records.len() as u32;
    // Level 0 holds the leaves. Each further level indexes the level below, until one node is left.
    let mut levels: Vec<Vec<Vec<Vec<u8>>>> = Vec::new();
    let mut bases: Vec<u32> = Vec::new();
    let mut next_node = 1u32;
    let mut current = records;
    while !current.is_empty() {
        let nodes: Vec<Vec<Vec<u8>>> = group(&current)?
            .into_iter()
            .map(|g| current[g].to_vec())
            .collect();
        let base = next_node;
        bases.push(base);
        next_node += nodes.len() as u32;
        current = if nodes.len() > 1 {
            nodes
                .iter()
                .enumerate()
                .map(|(j, n)| index_record(&n[0], base + j as u32))
                .collect()
        } else {
            Vec::new()
        };
        levels.push(nodes);
    }

    let (root, first_leaf, last_leaf) = match levels.first() {
        Some(leaves) => (
            bases[bases.len() - 1],
            bases[0],
            bases[0] + leaves.len() as u32 - 1,
        ),
        None => (0, 0, 0),
    };
    let header = TreeHeader {
        depth: levels.len() as u16,
        root,
        leaf_records,
        first_leaf,
        last_leaf,
        max_key,
        total_nodes: next_node,
        attributes,
    };
    let mut nodes = vec![header_node(&header)?];
    for (k, level) in levels.iter().enumerate() {
        for (j, records) in level.iter().enumerate() {
            let number = bases[k] + j as u32;
            let fwd = if j + 1 < level.len() { number + 1 } else { 0 };
            let bwd = if j > 0 { number - 1 } else { 0 };
            let (kind, height) = if k == 0 {
                (NODE_LEAF, 1)
            } else {
                (NODE_INDEX, (k + 1) as u8)
            };
            nodes.push(node(fwd, bwd, kind, height, records));
        }
    }
    Ok(nodes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_ascii_and_bounded() {
        assert!(check_name("/a", "plain name").is_ok());
        assert!(check_name("/é", "é").is_err());
        assert!(check_name("/x", "").is_err());
        assert!(check_name("/x", "..").is_err());
        assert!(check_name("/x", &"n".repeat(256)).is_err());
    }

    #[test]
    fn catalog_order_ignores_ascii_case_and_orders_parents_first() {
        let key = |parent, name: &str| CatKey {
            parent,
            name: name_units(name),
        };
        assert!(key(2, "Apple") == key(2, "apple"));
        assert!(key(2, "b") < key(3, "a"));
        assert!(key(2, "ab") < key(2, "abc"));
        assert!(key(2, "Zed") > key(2, "apple"));
    }
}
