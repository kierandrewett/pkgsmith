//! MD5 and SHA-256 of a stream, read once.

use anyhow::Result;
use md5::Md5;
use sha2::{Digest, Sha256};
use std::fmt::Write as _;
use std::io::Read;

pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().fold(String::new(), |mut out, byte| {
        let _ = write!(out, "{byte:02x}");
        out
    })
}

/// `(md5, sha256)` as lowercase hex.
pub fn digests(data: &mut impl Read) -> Result<(String, String)> {
    let (mut md5, mut sha) = (Md5::new(), Sha256::new());
    let mut buffer = [0u8; 65536];
    loop {
        let read = data.read(&mut buffer)?;
        if read == 0 {
            return Ok((hex(&md5.finalize()), hex(&sha.finalize())));
        }
        md5.update(&buffer[..read]);
        sha.update(&buffer[..read]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_digests() {
        let (md5, sha) = digests(&mut "abc".as_bytes()).unwrap();
        assert_eq!(md5, "900150983cd24fb0d6963f7d28e17f72");
        assert_eq!(
            sha,
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }
}
