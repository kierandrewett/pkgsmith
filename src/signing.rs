//! Package signing: the keys that the user gave, and the signatures made with them.
//!
//! The host holds the keys. A plugin that embeds a signature asks the host through [`crate::plugins::Signer`], so the
//! key is not part of the package description. A plugin is code in this process, so that is a convenience and not
//! isolation: install only plugins that you trust.
//!
//! Two kinds of key are read, both from a file:
//!
//! * an armored OpenPGP secret key (`gpg --export-secret-keys --armor KEYID`), which signs with [`Scheme::OpenPgp`];
//! * an RSA private key in PEM form, PKCS#1 (`BEGIN RSA PRIVATE KEY`) or PKCS#8 (`BEGIN PRIVATE KEY`, or
//!   `BEGIN ENCRYPTED PRIVATE KEY`), which signs with [`Scheme::RsaSha256`] and [`Scheme::RsaSha1`].

use crate::plugins::Signer;
use anyhow::{Context, Result, anyhow, bail};
use pgp::composed::{Deserializable, DetachedSignature, SignedSecretKey};
use pgp::crypto::hash::HashAlgorithm;
use pgp::ser::Serialize as _;
use pgp::types::Password;
use pkcs8::DecodePrivateKey as _;
use pkgsmith_plugin::Scheme;
use rsa::RsaPrivateKey;
use rsa::pkcs1::DecodeRsaPrivateKey as _;
use rsa::pkcs1v15::SigningKey;
use rsa::signature::{SignatureEncoding, Signer as _};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

enum Key {
    OpenPgp(Box<SignedSecretKey>),
    Rsa(Box<RsaPrivateKey>),
}

struct Entry {
    /// The format this key is for, or `None` for every format.
    format: Option<String>,
    path: PathBuf,
}

/// The signing keys of a run.
pub struct Keys {
    entries: Vec<Entry>,
    passphrase: Option<String>,
    loaded: Mutex<HashMap<PathBuf, Arc<Key>>>,
}

impl Keys {
    /// `specs` are `PATH` (a key for every format) or `FORMAT=PATH`. `passphrase` unlocks encrypted keys.
    pub fn new(specs: &[String], passphrase: Option<String>) -> Result<Keys> {
        let mut entries = Vec::new();
        for spec in specs {
            let (format, path) = match spec.split_once('=') {
                Some((format, path)) if !format.is_empty() && !format.contains('/') => {
                    (Some(format.to_string()), path)
                }
                _ => (None, spec.as_str()),
            };
            let path = PathBuf::from(path);
            if !path.is_file() {
                bail!("the signing key {} does not exist", path.display());
            }
            entries.push(Entry { format, path });
        }
        Ok(Keys {
            entries,
            passphrase,
            loaded: Mutex::new(HashMap::new()),
        })
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    fn load(&self, path: &Path) -> Result<Arc<Key>> {
        if let Some(key) = self.loaded.lock().unwrap().get(path) {
            return Ok(key.clone());
        }
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("cannot read the signing key {}", path.display()))?;
        let password = self.passphrase.clone().unwrap_or_default();
        let key = if text.contains("BEGIN PGP PRIVATE KEY BLOCK") {
            let (key, _) = SignedSecretKey::from_string(&text)
                .with_context(|| format!("{} is not a valid OpenPGP secret key", path.display()))?;
            Key::OpenPgp(Box::new(key))
        } else if text.contains("BEGIN RSA PRIVATE KEY") {
            Key::Rsa(Box::new(
                RsaPrivateKey::from_pkcs1_pem(&text)
                    .with_context(|| format!("{} is not a valid RSA key", path.display()))?,
            ))
        } else if text.contains("BEGIN ENCRYPTED PRIVATE KEY") {
            if self.passphrase.is_none() {
                bail!(
                    "{} is encrypted. Give the passphrase with --sign-passphrase-file or PKGSMITH_SIGN_PASSPHRASE",
                    path.display()
                );
            }
            Key::Rsa(Box::new(
                RsaPrivateKey::from_pkcs8_encrypted_pem(&text, password.as_bytes()).with_context(
                    || format!("cannot decrypt {}: wrong passphrase?", path.display()),
                )?,
            ))
        } else if text.contains("BEGIN PRIVATE KEY") {
            Key::Rsa(Box::new(
                RsaPrivateKey::from_pkcs8_pem(&text)
                    .with_context(|| format!("{} is not a valid RSA key", path.display()))?,
            ))
        } else {
            bail!(
                "{} is not an armored OpenPGP secret key or a PEM RSA private key",
                path.display()
            );
        };
        let key = Arc::new(key);
        self.loaded
            .lock()
            .unwrap()
            .insert(path.to_path_buf(), key.clone());
        Ok(key)
    }

    /// The key for `format` that can sign with `scheme`: a key named for the format first, then the default keys.
    fn find(&self, format: &str, scheme: Scheme) -> Result<Option<(Arc<Key>, &Path)>> {
        let named = self
            .entries
            .iter()
            .filter(|e| e.format.as_deref() == Some(format));
        let general = self.entries.iter().filter(|e| e.format.is_none());
        for entry in named.chain(general) {
            let key = self.load(&entry.path)?;
            let fits = matches!(
                (&*key, scheme),
                (Key::OpenPgp(_), Scheme::OpenPgp)
                    | (Key::Rsa(_), Scheme::RsaSha256 | Scheme::RsaSha1)
            );
            if fits {
                return Ok(Some((key, &entry.path)));
            }
        }
        Ok(None)
    }

    /// Check that a key exists for `format` and `scheme`, and say why not: no key, a key of another type, or a key that
    /// cannot be read, for example because the passphrase is missing or wrong.
    pub fn require(&self, format: &str, scheme: Scheme) -> Result<()> {
        if self.find(format, scheme)?.is_some() {
            return Ok(());
        }
        let wanted = match scheme {
            Scheme::OpenPgp => "an armored OpenPGP secret key",
            _ => "an RSA private key in PEM form",
        };
        let other = if self.has_key_for(format) {
            " The key you gave has another type."
        } else {
            ""
        };
        bail!("{format} is signed with {scheme:?}. Give {wanted} with --sign-key.{other}")
    }

    /// Whether a key for this format exists at all, whatever its type.
    pub fn has_key_for(&self, format: &str) -> bool {
        self.entries
            .iter()
            .any(|e| e.format.is_none() || e.format.as_deref() == Some(format))
    }

    /// An OpenPGP detached signature of `data`, as a binary packet or as ASCII armor.
    pub fn detached(&self, format: &str, data: &[u8], armor: bool) -> Result<Vec<u8>> {
        let signature = self.make(format, Scheme::OpenPgp, data)?;
        if !armor {
            return Ok(signature);
        }
        let parsed = DetachedSignature::from_bytes(signature.as_slice())?;
        Ok(parsed.to_armored_bytes(Default::default())?)
    }

    fn make(&self, format: &str, scheme: Scheme, data: &[u8]) -> Result<Vec<u8>> {
        let Some((key, path)) = self.find(format, scheme)? else {
            bail!(
                "no signing key for {format} can sign with {scheme:?}. {}",
                match scheme {
                    Scheme::OpenPgp => "Give an armored OpenPGP secret key",
                    _ => "Give an RSA private key in PEM form",
                }
            );
        };
        match &*key {
            Key::OpenPgp(key) => {
                let password = Password::from(self.passphrase.clone().unwrap_or_default().as_str());
                let signature = DetachedSignature::sign_binary_data(
                    rand::rngs::OsRng,
                    &key.primary_key,
                    &password,
                    HashAlgorithm::Sha256,
                    data,
                )
                .map_err(|e| {
                    let hint = if self.passphrase.is_none() {
                        " If the key has a passphrase, give it with --sign-passphrase-file or PKGSMITH_SIGN_PASSPHRASE."
                    } else {
                        ""
                    };
                    anyhow!("cannot sign with {}: {e}.{hint}", path.display())
                })?;
                Ok(signature.to_bytes()?)
            }
            Key::Rsa(key) => Ok(match scheme {
                Scheme::RsaSha1 => SigningKey::<sha1_10::Sha1>::new((**key).clone())
                    .sign(data)
                    .to_vec(),
                _ => SigningKey::<sha2_10::Sha256>::new((**key).clone())
                    .sign(data)
                    .to_vec(),
            }),
        }
    }
}

impl Signer for Keys {
    fn can_sign(&self, format: &str, scheme: Scheme) -> bool {
        matches!(self.find(format, scheme), Ok(Some(_)))
    }

    fn sign(&self, format: &str, scheme: Scheme, data: &[u8]) -> Result<Vec<u8>> {
        self.make(format, scheme, data)
    }

    fn key_name(&self, format: &str, scheme: Scheme) -> Option<String> {
        let (_, path) = self.find(format, scheme).ok()??;
        path.file_name().map(|n| n.to_string_lossy().into_owned())
    }
}
