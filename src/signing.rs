//! Package signing: which key signs which format, and the call to the signer plugin that does it.
//!
//! The host holds no key and no signing code. A signer is a plugin (see `pkgsmith plugins`) that offers one or more
//! schemes. The user gives `--sign-key [FORMAT=][SIGNER:]KEY`:
//!
//! * `FORMAT=` limits the key to one format. A key without it is tried for every format.
//! * `SIGNER:` names the signer plugin. Without it, the signers are asked one by one whether they take the key.
//! * `KEY` is whatever the signer understands: a key file, or a key id for `gpg`.
//!
//! A package format states how it is signed: not at all, with a detached OpenPGP signature that this module writes next
//! to the file, or with a signature inside the package that the format plugin asks for while it builds.

use crate::plugins::{Registry, Signer};
use anyhow::{Context, Result, anyhow, bail};
use pkgsmith_plugin::{Scheme, SignRequest, Signing as FormatSigning};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// One `--sign-key` value, split into its parts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Spec {
    pub format: Option<String>,
    pub signer: Option<String>,
    pub key: String,
}

/// Split `[FORMAT=][SIGNER:]KEY`. A prefix counts only when it is a name that exists, so a key file such as
/// `C:\keys\a.pem` or `./a=b.pem` stays whole.
pub fn parse_spec(text: &str, formats: &[String], signers: &[String]) -> Spec {
    let mut rest = text;
    let mut format = None;
    if let Some((head, tail)) = rest.split_once('=')
        && formats.iter().any(|f| f == head)
    {
        format = Some(head.to_string());
        rest = tail;
    }
    let mut signer = None;
    if let Some((head, tail)) = rest.split_once(':')
        && signers.iter().any(|s| s == head)
    {
        signer = Some(head.to_string());
        rest = tail;
    }
    Spec {
        format,
        signer,
        key: rest.to_string(),
    }
}

/// A signer name and the key it will use.
type Chosen = (String, String);

/// The signing setup of one run.
pub struct Signing<'a> {
    registry: &'a Registry,
    specs: Vec<Spec>,
    passphrase_file: Option<PathBuf>,
    /// The result of asking the signers for a format and scheme, so each question is asked once.
    chosen: Mutex<HashMap<(String, Scheme), Option<Chosen>>>,
}

impl<'a> Signing<'a> {
    pub fn new(
        registry: &'a Registry,
        texts: &[String],
        passphrase_file: Option<PathBuf>,
    ) -> Result<Self> {
        let formats = registry.format_names();
        let signers: Vec<String> = registry
            .signers()
            .iter()
            .map(|(s, _)| s.name.clone())
            .collect();
        let specs: Vec<Spec> = texts
            .iter()
            .map(|t| parse_spec(t, &formats, &signers))
            .collect();
        if let Some(spec) = specs.iter().find(|s| s.key.is_empty()) {
            bail!("--sign-key {spec:?} has no key");
        }
        if let Some(file) = &passphrase_file
            && !file.is_file()
        {
            bail!("the passphrase file {} does not exist", file.display());
        }
        Ok(Signing {
            registry,
            specs,
            passphrase_file,
            chosen: Mutex::new(HashMap::new()),
        })
    }

    /// No key was given, so nothing is signed.
    pub fn is_off(&self) -> bool {
        self.specs.is_empty()
    }

    fn request(&self, spec: &Spec, scheme: Scheme, armor: bool) -> SignRequest {
        SignRequest {
            signer: spec.signer.clone().unwrap_or_default(),
            scheme,
            key: spec.key.clone(),
            passphrase_file: self.passphrase_file.clone(),
            input: PathBuf::new(),
            output: PathBuf::new(),
            armor,
        }
    }

    /// The signer and key for a format and scheme. Keys named for the format go first, then the general ones.
    fn choose(&self, format: &str, scheme: Scheme) -> Result<Option<Chosen>> {
        let cache_key = (format.to_string(), scheme);
        if let Some(found) = self.chosen.lock().unwrap().get(&cache_key) {
            return Ok(found.clone());
        }
        let named = self
            .specs
            .iter()
            .filter(|s| s.format.as_deref() == Some(format));
        let general = self.specs.iter().filter(|s| s.format.is_none());
        let mut found = None;
        for spec in named.chain(general) {
            let request = self.request(spec, scheme, false);
            if let Some(signer) = self
                .registry
                .choose_signer(&request, spec.signer.is_some())?
            {
                found = Some((signer, spec.key.clone()));
                break;
            }
        }
        self.chosen.lock().unwrap().insert(cache_key, found.clone());
        Ok(found)
    }

    /// The scheme a format needs, or `None` when pkgsmith does not sign it.
    pub fn scheme_for(&self, format: &str) -> Option<Scheme> {
        match self.registry.signing(format)? {
            FormatSigning::Detached => Some(Scheme::OpenPgp),
            FormatSigning::Embedded(scheme) => Some(scheme),
            FormatSigning::None => None,
        }
    }

    /// Check before any build starts that a key and a signer exist for `format`. The signer has opened the key by now, so
    /// a missing file, a wrong key type or a wrong passphrase stops the run here.
    pub fn require(&self, format: &str) -> Result<()> {
        let Some(scheme) = self.scheme_for(format) else {
            return Ok(());
        };
        if self.choose(format, scheme)?.is_some() {
            return Ok(());
        }
        let installed: Vec<String> = self
            .registry
            .signers()
            .iter()
            .filter(|(s, _)| s.schemes.contains(&scheme))
            .map(|(s, _)| s.name.clone())
            .collect();
        bail!(
            "{format} needs a {scheme:?} signature, but no signer takes the key you gave. Signers for it: {}. Give a key with --sign-key [{format}=][SIGNER:]KEY",
            if installed.is_empty() {
                "none installed".to_string()
            } else {
                installed.join(", ")
            }
        )
    }

    fn sign_paths(
        &self,
        format: &str,
        scheme: Scheme,
        input: &Path,
        output: &Path,
        armor: bool,
    ) -> Result<()> {
        let (signer, key) = self
            .choose(format, scheme)?
            .ok_or_else(|| anyhow!("no signer takes the key for {format}"))?;
        self.registry.run_signer(&SignRequest {
            signer,
            scheme,
            key,
            passphrase_file: self.passphrase_file.clone(),
            input: input.to_path_buf(),
            output: output.to_path_buf(),
            armor,
        })
    }

    /// Write the detached OpenPGP signature of `file` next to it, as `FILE.sig` or `FILE.asc`. Returns what was done,
    /// for the summary line.
    pub fn sign_detached(&self, format: &str, file: &Path, armor: bool) -> Result<&'static str> {
        match self.registry.signing(format) {
            Some(FormatSigning::Detached) => {
                let mut output = file.as_os_str().to_owned();
                output.push(if armor { ".asc" } else { ".sig" });
                self.sign_paths(format, Scheme::OpenPgp, file, Path::new(&output), armor)?;
                Ok(if armor {
                    "signed, .asc"
                } else {
                    "signed, .sig"
                })
            }
            Some(FormatSigning::Embedded(_)) => Ok("signed"),
            _ => Ok("not signed: this format cannot be signed"),
        }
    }
}

impl Signer for Signing<'_> {
    fn can_sign(&self, format: &str, scheme: Scheme) -> bool {
        matches!(self.choose(format, scheme), Ok(Some(_)))
    }

    fn sign(&self, format: &str, scheme: Scheme, data: &[u8]) -> Result<Vec<u8>> {
        // The format plugin hands over bytes, the signer plugins read and write files. Keep the files in a private
        // folder that is removed at once.
        let dir = tempfile::tempdir().context("cannot make a folder for the signature")?;
        let input = dir.path().join("data");
        let output = dir.path().join("signature");
        std::fs::write(&input, data)?;
        self.sign_paths(format, scheme, &input, &output, false)?;
        Ok(std::fs::read(&output)?)
    }

    fn key_name(&self, format: &str, scheme: Scheme) -> Option<String> {
        let (_, key) = self.choose(format, scheme).ok()??;
        Some(
            Path::new(&key)
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or(key),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn a_prefix_counts_only_when_it_names_a_format_or_a_signer() {
        let formats = names(&["rpm", "apk"]);
        let signers = names(&["gpg", "rsa-pem"]);
        let parse = |t| parse_spec(t, &formats, &signers);
        assert_eq!(
            parse("rpm=gpg:ABCD"),
            Spec {
                format: Some("rpm".into()),
                signer: Some("gpg".into()),
                key: "ABCD".into()
            }
        );
        assert_eq!(parse("apk=key.pem").format.as_deref(), Some("apk"));
        assert_eq!(parse("gpg:me@example.org").signer.as_deref(), Some("gpg"));
        // Not names: the text is the key.
        assert_eq!(parse("./a=b.pem").key, "./a=b.pem");
        assert_eq!(parse("C:\\keys\\a.pem").key, "C:\\keys\\a.pem");
        assert_eq!(parse("key.asc").signer, None);
    }
}
