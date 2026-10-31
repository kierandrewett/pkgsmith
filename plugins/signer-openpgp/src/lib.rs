//! The `openpgp-file` signer: signs with an armored OpenPGP secret key that is a file.
//!
//! This needs no `gpg` program, which suits a CI job that has a key in a secret. The file is read by pkgsmith, so
//! prefer the `gpg` signer on a machine where the key lives in `gpg-agent` or on a smartcard. A key that has a signing
//! subkey signs with that subkey. Otherwise the primary key signs, if it may.

use anyhow::{Context, Result, anyhow, bail};
use pgp::composed::{Deserializable, DetachedSignature, SignedSecretKey};
use pgp::crypto::hash::HashAlgorithm;
use pgp::ser::Serialize as _;
use pgp::types::{Password, SigningKey};
use pkgsmith_plugin::{Plugin, PluginInfo, Scheme, SignRequest, SignerInfo};
use std::fs::File;
use std::path::Path;

struct OpenPgpFile;

const NAME: &str = "openpgp-file";

fn read_key(path: &str) -> Result<Option<SignedSecretKey>> {
    let Ok(text) = std::fs::read_to_string(path) else {
        return Ok(None);
    };
    if !text.contains("BEGIN PGP PRIVATE KEY BLOCK") {
        return Ok(None);
    }
    let (key, _) = SignedSecretKey::from_string(&text)
        .with_context(|| format!("{path} is not a valid OpenPGP secret key"))?;
    Ok(Some(key))
}

fn passphrase(file: &Option<std::path::PathBuf>) -> Result<Password> {
    match file {
        Some(file) => {
            let text = std::fs::read_to_string(file)
                .with_context(|| format!("cannot read {}", file.display()))?;
            Ok(Password::from(text.trim_end_matches(['\r', '\n'])))
        }
        None => Ok(Password::empty()),
    }
}

/// Which key signs: a subkey that is allowed to sign, else the primary key if it is allowed to.
enum Signer<'a> {
    Subkey(&'a pgp::packet::SecretSubkey),
    Primary(&'a pgp::packet::SecretKey),
}

fn choose(key: &SignedSecretKey) -> Result<Signer<'_>> {
    for subkey in &key.secret_subkeys {
        if subkey.signatures.iter().any(|s| s.key_flags().sign()) {
            return Ok(Signer::Subkey(&subkey.key));
        }
    }
    let primary_may_sign = key
        .details
        .users
        .iter()
        .flat_map(|u| u.signatures.iter())
        .chain(key.details.direct_signatures.iter())
        .any(|s| s.key_flags().sign());
    if primary_may_sign {
        return Ok(Signer::Primary(&key.primary_key));
    }
    bail!("the key has no key that is allowed to sign. Add a signing subkey")
}

fn locked(signer: &Signer<'_>) -> bool {
    match signer {
        Signer::Subkey(key) => key.secret_params().is_encrypted(),
        Signer::Primary(key) => key.secret_params().is_encrypted(),
    }
}

fn make(key: &impl SigningKey, password: &Password, input: &Path) -> Result<DetachedSignature> {
    let file = File::open(input).with_context(|| format!("cannot read {}", input.display()))?;
    DetachedSignature::sign_binary_data(
        rand::rngs::OsRng,
        key,
        password,
        HashAlgorithm::Sha256,
        std::io::BufReader::new(file),
    )
    .map_err(|e| anyhow!("cannot sign: {e}"))
}

impl Plugin for OpenPgpFile {
    fn info(&self) -> PluginInfo {
        PluginInfo {
            name: "signer-openpgp".into(),
            version: env!("CARGO_PKG_VERSION").into(),
            signers: vec![SignerInfo {
                name: NAME.into(),
                description: "Signs with an armored OpenPGP secret key file, without gpg".into(),
                schemes: vec![Scheme::OpenPgp],
            }],
            ..Default::default()
        }
    }

    fn accepts(&self, request: &SignRequest) -> Result<bool> {
        if request.signer != NAME || request.scheme != Scheme::OpenPgp {
            return Ok(false);
        }
        let Some(key) = read_key(&request.key)? else {
            return Ok(false);
        };
        let signer = choose(&key)?;
        if locked(&signer) && request.passphrase_file.is_none() {
            bail!(
                "{} is protected by a passphrase. Give it with --sign-passphrase-file",
                request.key
            );
        }
        Ok(true)
    }

    fn sign(&self, request: &SignRequest) -> Result<()> {
        let key = read_key(&request.key)?
            .ok_or_else(|| anyhow!("{} is not an armored OpenPGP secret key", request.key))?;
        let password = passphrase(&request.passphrase_file)?;
        let signature = match choose(&key)? {
            Signer::Subkey(subkey) => make(subkey, &password, &request.input),
            Signer::Primary(primary) => make(primary, &password, &request.input),
        }
        .with_context(|| format!("with the key {}", request.key))?;
        let bytes = if request.armor {
            signature.to_armored_bytes(Default::default())?
        } else {
            signature.to_bytes()?
        };
        std::fs::write(&request.output, bytes)
            .with_context(|| format!("cannot write {}", request.output.display()))?;
        Ok(())
    }
}

pkgsmith_plugin::export_plugin!(OpenPgpFile);
