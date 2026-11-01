//! The zip writer of an MSIX package, laid out as the Microsoft reference writer `makemsix` lays it out.
//!
//! Each member is a local header with no sizes, the data, and a zip64 data descriptor with the real sizes. Deflated
//! members are compressed in 64 KiB blocks, each flushed with `Z_FULL_FLUSH`, so the compressed size of every block is
//! known: the block map records it, and Windows reads one block at a time. The central directory is followed by a
//! zip64 end record, its locator and the classic end record. The zip format is in APPNOTE.TXT, section 4 and 4.3.16,
//! https://pkware.cachefly.net/webdocs/casestudies/APPNOTE.TXT.

use crate::opc;
use anyhow::{Context, Result, bail};
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use flate2::{Compress, Compression, Crc, FlushCompress, Status};
use sha2::{Digest, Sha256};
use std::io::{self, Read, Write};

/// The uncompressed size of a block of the block map.
pub const BLOCK: usize = 64 * 1024;

/// The version a zip writer states: 4.5, which zip64 needs.
const VERSION: u16 = 45;
/// Bit 3: the sizes follow the data, in a data descriptor.
const FLAG_DESCRIPTOR: u16 = 1 << 3;
const METHOD_STORED: u16 = 0;
const METHOD_DEFLATE: u16 = 8;
const SIG_LOCAL: u32 = 0x0403_4b50;
const SIG_DESCRIPTOR: u32 = 0x0807_4b50;
const SIG_CENTRAL: u32 = 0x0201_4b50;
const SIG_ZIP64_END: u32 = 0x0606_4b50;
const SIG_ZIP64_LOCATOR: u32 = 0x0706_4b50;
const SIG_END: u32 = 0x0605_4b50;

/// One block of a member: its compressed size, unless the member is stored, and the base64 SHA-256 of its
/// uncompressed bytes.
pub struct Block {
    pub compressed: Option<u64>,
    pub hash: String,
}

/// A member that was written: what the block map needs about it.
pub struct Member {
    /// The package-relative name, before the percent-encoding of the zip name.
    pub name: String,
    /// The uncompressed size.
    pub size: u64,
    /// The size of the local header: 30 bytes and the zip name.
    pub lfh_size: u64,
    pub blocks: Vec<Block>,
}

/// What the central directory says about one member.
struct Central {
    name: Vec<u8>,
    method: u16,
    crc: u32,
    csize: u32,
    usize: u32,
    offset: u32,
}

/// Writes a zip file, one member at a time, with `add`, and ends it with `finish`.
pub struct Writer<W: Write> {
    out: W,
    offset: u64,
    time: u16,
    date: u16,
    central: Vec<Central>,
}

impl<W: Write> Writer<W> {
    /// A writer whose members all carry the time `mtime` (seconds since the epoch).
    pub fn new(out: W, mtime: u64) -> Self {
        let (time, date) = dos_time(mtime);
        Writer {
            out,
            offset: 0,
            time,
            date,
            central: Vec::new(),
        }
    }

    /// Writes the member `name` with the bytes of `source`. `stored` keeps the bytes uncompressed.
    pub fn add<R: Read>(&mut self, name: &str, mut source: R, stored: bool) -> Result<Member> {
        let encoded = opc::zip_name(name).into_bytes();
        let name_len = u16::try_from(encoded.len())
            .with_context(|| format!("the name {name:?} is too long for a zip member"))?;
        let method = if stored {
            METHOD_STORED
        } else {
            METHOD_DEFLATE
        };
        let offset = self.checked_u32(self.offset, name)?;
        let mut header = Vec::with_capacity(30 + encoded.len());
        header.extend_from_slice(&SIG_LOCAL.to_le_bytes());
        header.extend_from_slice(&VERSION.to_le_bytes());
        header.extend_from_slice(&FLAG_DESCRIPTOR.to_le_bytes());
        header.extend_from_slice(&method.to_le_bytes());
        header.extend_from_slice(&self.time.to_le_bytes());
        header.extend_from_slice(&self.date.to_le_bytes());
        header.extend_from_slice(&[0u8; 12]); // crc, compressed size and size: in the descriptor
        header.extend_from_slice(&name_len.to_le_bytes());
        header.extend_from_slice(&0u16.to_le_bytes()); // no extra field
        header.extend_from_slice(&encoded);
        self.put(&header)?;

        let mut crc = Crc::new();
        let mut size = 0u64;
        let mut csize = 0u64;
        let mut blocks = Vec::new();
        let mut chunk = vec![0u8; BLOCK];
        let mut deflater = (!stored).then(|| Compress::new(Compression::default(), false));
        let mut packed = Vec::new();
        loop {
            let read =
                fill(&mut source, &mut chunk).with_context(|| format!("cannot read {name:?}"))?;
            if read == 0 {
                break;
            }
            let data = &chunk[..read];
            crc.update(data);
            size += read as u64;
            let hash = STANDARD.encode(Sha256::digest(data));
            match deflater.as_mut() {
                None => {
                    self.put(data)?;
                    csize += read as u64;
                    blocks.push(Block {
                        compressed: None,
                        hash,
                    });
                }
                Some(compressor) => {
                    packed.clear();
                    deflate(compressor, data, FlushCompress::Full, &mut packed)?;
                    self.put(&packed)?;
                    csize += packed.len() as u64;
                    blocks.push(Block {
                        compressed: Some(packed.len() as u64),
                        hash,
                    });
                }
            }
        }
        if let Some(compressor) = deflater.as_mut() {
            // The closing bytes are not part of any block: the blocks sum to the member size minus these 2 bytes, as Windows expects.
            packed.clear();
            deflate(compressor, &[], FlushCompress::Finish, &mut packed)?;
            self.put(&packed)?;
            csize += packed.len() as u64;
        }
        let crc = crc.sum();

        let mut descriptor = Vec::with_capacity(24);
        descriptor.extend_from_slice(&SIG_DESCRIPTOR.to_le_bytes());
        descriptor.extend_from_slice(&crc.to_le_bytes());
        descriptor.extend_from_slice(&csize.to_le_bytes());
        descriptor.extend_from_slice(&size.to_le_bytes());
        self.put(&descriptor)?;

        self.central.push(Central {
            name: encoded.clone(),
            method,
            crc,
            csize: self.checked_u32(csize, name)?,
            usize: self.checked_u32(size, name)?,
            offset,
        });
        Ok(Member {
            name: name.to_owned(),
            size,
            lfh_size: 30 + u64::from(name_len),
            blocks,
        })
    }

    /// Writes the central directory and the end records, and flushes the file.
    pub fn finish(mut self) -> Result<()> {
        let cd_offset = self.offset;
        let count = self.central.len() as u64;
        for entry in std::mem::take(&mut self.central) {
            let mut record = Vec::with_capacity(46 + entry.name.len());
            record.extend_from_slice(&SIG_CENTRAL.to_le_bytes());
            record.extend_from_slice(&VERSION.to_le_bytes()); // made by
            record.extend_from_slice(&VERSION.to_le_bytes()); // needed to extract
            record.extend_from_slice(&FLAG_DESCRIPTOR.to_le_bytes());
            record.extend_from_slice(&entry.method.to_le_bytes());
            record.extend_from_slice(&self.time.to_le_bytes());
            record.extend_from_slice(&self.date.to_le_bytes());
            record.extend_from_slice(&entry.crc.to_le_bytes());
            record.extend_from_slice(&entry.csize.to_le_bytes());
            record.extend_from_slice(&entry.usize.to_le_bytes());
            record.extend_from_slice(&u16::try_from(entry.name.len())?.to_le_bytes());
            record.extend_from_slice(&0u16.to_le_bytes()); // extra field
            record.extend_from_slice(&0u16.to_le_bytes()); // comment
            record.extend_from_slice(&0u16.to_le_bytes()); // disk
            record.extend_from_slice(&0u16.to_le_bytes()); // internal attributes
            record.extend_from_slice(&0u32.to_le_bytes()); // external attributes
            record.extend_from_slice(&entry.offset.to_le_bytes());
            record.extend_from_slice(&entry.name);
            self.put(&record)?;
        }
        let cd_size = self.offset - cd_offset;
        let eocd64_offset = self.offset;

        let mut end = Vec::with_capacity(56 + 20 + 22);
        end.extend_from_slice(&SIG_ZIP64_END.to_le_bytes());
        end.extend_from_slice(&44u64.to_le_bytes()); // the size of the rest of this record
        end.extend_from_slice(&VERSION.to_le_bytes());
        end.extend_from_slice(&VERSION.to_le_bytes());
        end.extend_from_slice(&0u32.to_le_bytes()); // this disk
        end.extend_from_slice(&0u32.to_le_bytes()); // disk of the directory
        end.extend_from_slice(&count.to_le_bytes()); // entries on this disk
        end.extend_from_slice(&count.to_le_bytes()); // entries
        end.extend_from_slice(&cd_size.to_le_bytes());
        end.extend_from_slice(&cd_offset.to_le_bytes());
        end.extend_from_slice(&SIG_ZIP64_LOCATOR.to_le_bytes());
        end.extend_from_slice(&0u32.to_le_bytes()); // disk of the zip64 end record
        end.extend_from_slice(&eocd64_offset.to_le_bytes());
        end.extend_from_slice(&1u32.to_le_bytes()); // disks
        end.extend_from_slice(&SIG_END.to_le_bytes());
        end.extend_from_slice(&0u16.to_le_bytes()); // this disk
        end.extend_from_slice(&0u16.to_le_bytes()); // disk of the directory
        end.extend_from_slice(&u16::MAX.to_le_bytes()); // entries: see the zip64 record
        end.extend_from_slice(&u16::MAX.to_le_bytes());
        end.extend_from_slice(&u32::MAX.to_le_bytes()); // directory size: see the zip64 record
        end.extend_from_slice(&u32::MAX.to_le_bytes()); // directory offset: see the zip64 record
        end.extend_from_slice(&0u16.to_le_bytes()); // comment
        self.put(&end)?;
        self.out.flush()?;
        Ok(())
    }

    fn put(&mut self, bytes: &[u8]) -> io::Result<()> {
        self.out.write_all(bytes)?;
        self.offset += bytes.len() as u64;
        Ok(())
    }

    /// The classic record fields are 32 bits. A larger package needs zip64 fields in every record, which this writer
    /// does not write, so it refuses instead.
    fn checked_u32<T>(&self, value: T, name: &str) -> Result<u32>
    where
        T: TryInto<u32> + Copy + std::fmt::Display,
    {
        match value.try_into() {
            Ok(value) => Ok(value),
            Err(_) => {
                bail!("{name:?} or the package is over 4 GiB, which this writer does not support")
            }
        }
    }
}

/// Reads until `buf` is full or the source ends. Returns the number of bytes read.
fn fill<R: Read>(source: &mut R, buf: &mut [u8]) -> io::Result<usize> {
    let mut filled = 0;
    while filled < buf.len() {
        match source.read(&mut buf[filled..]) {
            Ok(0) => break,
            Ok(n) => filled += n,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) => return Err(error),
        }
    }
    Ok(filled)
}

/// Feeds `input` to a raw deflate stream and appends the output to `out`. `Full` flushes the output so far and ends the
/// block: the output ends on a byte boundary and the bytes can be cut there. `Finish` ends the stream.
fn deflate(
    compressor: &mut Compress,
    mut input: &[u8],
    flush: FlushCompress,
    out: &mut Vec<u8>,
) -> Result<()> {
    let mut buf = [0u8; 16 * 1024];
    loop {
        let (in_before, out_before) = (compressor.total_in(), compressor.total_out());
        let status = compressor
            .compress(input, &mut buf, flush)
            .context("cannot compress")?;
        let used = (compressor.total_in() - in_before) as usize;
        let produced = (compressor.total_out() - out_before) as usize;
        input = &input[used..];
        out.extend_from_slice(&buf[..produced]);
        if status == Status::StreamEnd {
            return Ok(());
        }
        // A full flush is complete once the output has room left over: zlib has nothing more to write.
        if flush == FlushCompress::Full && input.is_empty() && produced < buf.len() {
            return Ok(());
        }
    }
}

/// The DOS time and date of `secs` (seconds since the epoch) in UTC, for zip headers. A year outside 1980 to 2107 is
/// clamped, because the DOS date has no other range.
pub fn dos_time(secs: u64) -> (u16, u16) {
    let secs = i64::try_from(secs).unwrap_or(i64::MAX);
    let days = secs.div_euclid(86_400);
    let rest = secs.rem_euclid(86_400);
    let (hour, minute, second) = (rest / 3600, rest % 3600 / 60, rest % 60);
    // Howard Hinnant's civil-from-days, http://howardhinnant.github.io/date_algorithms.html
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = (yoe + era * 400 + i64::from(month <= 2)).clamp(1980, 2107);
    let time = (hour << 11) | (minute << 5) | (second / 2);
    let date = ((year - 1980) << 9) | (month << 5) | day;
    (time as u16, date as u16)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dos_time_of_known_instants() {
        assert_eq!(dos_time(0), (0, 0x21)); // 1980-01-01 00:00:00
        // 2001-09-09 01:46:40 UTC
        assert_eq!(dos_time(1_000_000_000), (3540, 11049));
    }

    #[test]
    fn a_stored_member_is_its_bytes_and_a_block_per_chunk() {
        let mut writer = Writer::new(Vec::new(), 0);
        let data = vec![7u8; BLOCK + 10];
        let member = writer.add("a.bin", &data[..], true).unwrap();
        assert_eq!(member.size, data.len() as u64);
        assert_eq!(member.blocks.len(), 2);
        assert!(member.blocks.iter().all(|b| b.compressed.is_none()));
        assert_eq!(member.lfh_size, 30 + 5);
    }

    #[test]
    fn a_deflated_member_records_each_block_compressed_size() {
        let mut writer = Writer::new(Vec::new(), 0);
        let data = vec![9u8; BLOCK * 2 + 1];
        let member = writer.add("b.txt", &data[..], false).unwrap();
        assert_eq!(member.blocks.len(), 3);
        assert!(member.blocks.iter().all(|b| b.compressed.is_some()));
    }
}
