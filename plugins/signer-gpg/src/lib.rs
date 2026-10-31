//! The `gpg` signer: signs with the `gpg` program, so the keys stay where gpg keeps them.
//!
//! The key is not a file. It is whatever `gpg --local-user` takes: a key id, a fingerprint or an e-mail address.
//! `gpg-agent` asks for the passphrase and does the signing, so a smartcard or a hardware key works, and the secret key
//! never reaches pkgsmith. A passphrase file is read by gpg itself with `--pinentry-mode loopback`.

use anyhow::{Context, Result, bail};
use pkgsmith_plugin::{Plugin, PluginInfo, Scheme, SignRequest, SignerInfo};
use std::path::Path;
use std::process::{Command, Stdio};

struct Gpg;

const NAME: &str = "gpg";

/// A key that is an existing file belongs to a signer that reads key files.
fn is_key_id(key: &str) -> bool {
    !key.is_empty() && !Path::new(key).is_file()
}

fn gpg_available() -> bool {
    Command::new("gpg")
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

impl Plugin for Gpg {
    fn info(&self) -> PluginInfo {
        PluginInfo {
            name: "signer-gpg".into(),
            version: env!("CARGO_PKG_VERSION").into(),
            signers: vec![SignerInfo {
                name: NAME.into(),
                description: "Signs with gpg and its agent. The key is a key id, a fingerprint or an e-mail address".into(),
                schemes: vec![Scheme::OpenPgp],
            }],
            ..Default::default()
        }
    }

    fn accepts(&self, request: &SignRequest) -> Result<bool> {
        if request.signer != NAME || request.scheme != Scheme::OpenPgp || !is_key_id(&request.key) {
            return Ok(false);
        }
        if !gpg_available() {
            bail!("gpg is not installed, or it is not on PATH");
        }
        // Check that gpg knows the key, so a typo stops the build before it starts.
        let known = Command::new("gpg")
            .args(["--batch", "--list-secret-keys", "--with-colons", "--"])
            .arg(&request.key)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .context("cannot run gpg")?
            .success();
        if !known {
            bail!(
                "gpg has no secret key for {:?}. See `gpg --list-secret-keys`",
                request.key
            );
        }
        Ok(true)
    }

    fn sign(&self, request: &SignRequest) -> Result<()> {
        if !is_key_id(&request.key) {
            bail!(
                "the gpg signer takes a key id, not a file. Use the openpgp-file signer for a key file"
            );
        }
        let mut command = Command::new("gpg");
        command
            .args(["--batch", "--yes", "--detach-sign", "--local-user"])
            .arg(&request.key);
        if request.armor {
            command.arg("--armor");
        }
        if let Some(file) = &request.passphrase_file {
            command
                .args(["--pinentry-mode", "loopback", "--passphrase-file"])
                .arg(file);
        }
        let output = command
            .arg("--output")
            .arg(&request.output)
            .arg(&request.input)
            .stdin(Stdio::null())
            .output()
            .context("cannot run gpg")?;
        if !output.status.success() {
            bail!(
                "gpg failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            );
        }
        Ok(())
    }
}

pkgsmith_plugin::export_plugin!(Gpg);
