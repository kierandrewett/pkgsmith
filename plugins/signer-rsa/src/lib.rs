//! The `rsa-pem` signer: an RSA PKCS#1 v1.5 signature from a PEM private key file.
//!
//! Alpine's `apk` wants this kind of signature. The key is PKCS#1 (`BEGIN RSA PRIVATE KEY`), PKCS#8
//! (`BEGIN PRIVATE KEY`) or encrypted PKCS#8 (`BEGIN ENCRYPTED PRIVATE KEY`). The input is hashed as a stream.

use anyhow::{Context, Result, anyhow, bail};
use pkcs8::DecodePrivateKey as _;
use pkgsmith_plugin::{Plugin, PluginInfo, Scheme, SignRequest, SignerInfo};
use rsa::RsaPrivateKey;
use rsa::pkcs1::DecodeRsaPrivateKey as _;
use rsa::pkcs1v15::SigningKey;
use rsa::signature::SignatureEncoding;
use rsa::signature::hazmat::PrehashSigner;
use sha2::Digest;
use std::io::Read;
use std::path::PathBuf;

struct RsaPem;

const NAME: &str = "rsa-pem";

/// The text of the key file, or `None` when the key is not a PEM RSA key file.
fn read_pem(path: &str) -> Option<String> {
    let text = std::fs::read_to_string(path).ok()?;
    [
        "BEGIN RSA PRIVATE KEY",
        "BEGIN PRIVATE KEY",
        "BEGIN ENCRYPTED PRIVATE KEY",
    ]
    .iter()
    .any(|marker| text.contains(marker))
    .then_some(text)
}

fn passphrase(file: &Option<PathBuf>) -> Result<Option<String>> {
    Ok(match file {
        Some(file) => Some(
            std::fs::read_to_string(file)
                .with_context(|| format!("cannot read {}", file.display()))?
                .trim_end_matches(['\r', '\n'])
                .to_string(),
        ),
        None => None,
    })
}

fn load(text: &str, passphrase: &Option<String>, path: &str) -> Result<RsaPrivateKey> {
    if text.contains("BEGIN RSA PRIVATE KEY") {
        RsaPrivateKey::from_pkcs1_pem(text)
            .with_context(|| format!("{path} is not a valid RSA key"))
    } else if text.contains("BEGIN ENCRYPTED PRIVATE KEY") {
        let Some(passphrase) = passphrase else {
            bail!("{path} is encrypted. Give the passphrase with --sign-passphrase-file");
        };
        RsaPrivateKey::from_pkcs8_encrypted_pem(text, passphrase.as_bytes())
            .with_context(|| format!("cannot decrypt {path}: wrong passphrase?"))
    } else {
        RsaPrivateKey::from_pkcs8_pem(text)
            .with_context(|| format!("{path} is not a valid RSA key"))
    }
}

/// Hash a file in blocks.
fn digest<D: Digest>(path: &std::path::Path) -> Result<Vec<u8>> {
    let mut file =
        std::fs::File::open(path).with_context(|| format!("cannot read {}", path.display()))?;
    let mut hasher = D::new();
    let mut block = vec![0u8; 1 << 16];
    loop {
        let n = file.read(&mut block)?;
        if n == 0 {
            break;
        }
        hasher.update(&block[..n]);
    }
    Ok(hasher.finalize().to_vec())
}

impl Plugin for RsaPem {
    fn info(&self) -> PluginInfo {
        PluginInfo {
            name: "signer-rsa".into(),
            version: env!("CARGO_PKG_VERSION").into(),
            signers: vec![SignerInfo {
                name: NAME.into(),
                description: "An RSA PKCS#1 v1.5 signature from a PEM private key file".into(),
                schemes: vec![Scheme::RsaSha256, Scheme::RsaSha1],
            }],
            ..Default::default()
        }
    }

    fn accepts(&self, request: &SignRequest) -> Result<bool> {
        if request.signer != NAME || !matches!(request.scheme, Scheme::RsaSha256 | Scheme::RsaSha1)
        {
            return Ok(false);
        }
        let Some(text) = read_pem(&request.key) else {
            return Ok(false);
        };
        // Load it now: a locked key or a wrong passphrase is reported before any build starts.
        load(&text, &passphrase(&request.passphrase_file)?, &request.key)?;
        Ok(true)
    }

    fn sign(&self, request: &SignRequest) -> Result<()> {
        let text = read_pem(&request.key)
            .ok_or_else(|| anyhow!("{} is not a PEM private key", request.key))?;
        let key = load(&text, &passphrase(&request.passphrase_file)?, &request.key)?;
        let signature = match request.scheme {
            Scheme::RsaSha1 => {
                SigningKey::<sha1::Sha1>::new(key)
                    .sign_prehash(&digest::<sha1::Sha1>(&request.input)?)
            }
            Scheme::RsaSha256 => {
                SigningKey::<sha2::Sha256>::new(key)
                    .sign_prehash(&digest::<sha2::Sha256>(&request.input)?)
            }
            other => bail!("the rsa-pem signer cannot make {other:?}"),
        }
        .map_err(|e| anyhow!("cannot sign: {e}"))?;
        std::fs::write(&request.output, signature.to_vec())
            .with_context(|| format!("cannot write {}", request.output.display()))?;
        Ok(())
    }
}

pkgsmith_plugin::export_plugin!(RsaPem);
