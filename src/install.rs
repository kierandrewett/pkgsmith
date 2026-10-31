//! `pkgsmith plugins install`: put the plugins of a release where pkgsmith looks for them.
//!
//! `cargo install` and `cargo binstall` put one binary on the machine. The formats are shared libraries, so a second
//! step fetches them. A release has one archive per architecture, `pkgsmith-TARGET.tar.gz`, with the binary and a
//! `plugins/` folder, and a `SHA256SUMS` file. This module downloads the archive of the running version, checks its
//! digest, and copies the plugins out.

use anyhow::{Context, Result, anyhow, bail};
use pkgsmith_plugin::hash::hex;
use sha2::{Digest, Sha256};
use std::io::Read;
use std::path::{Path, PathBuf};

/// Where the assets of a release live. `{version}` is replaced by the version.
pub const DEFAULT_RELEASE_URL: &str =
    "https://github.com/kierandrewett/pkgsmith/releases/download/v{version}";

/// The target of the release archive that fits this machine.
pub fn target() -> Result<&'static str> {
    if cfg!(target_env = "musl") {
        bail!(
            "the plugin bundle is built for glibc and cannot load into this musl binary. Install the apk package of pkgsmith instead"
        );
    }
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("linux", "x86_64") => Ok("x86_64-unknown-linux-gnu"),
        ("linux", "aarch64") => Ok("aarch64-unknown-linux-gnu"),
        ("macos", "x86_64") => Ok("x86_64-apple-darwin"),
        ("macos", "aarch64") => Ok("aarch64-apple-darwin"),
        ("windows", "x86_64") => Ok("x86_64-pc-windows-msvc"),
        (os, arch) => bail!("there is no plugin bundle for {arch} {os}"),
    }
}

/// The folder `install` writes to when the user names none: the one under the user's data folder that pkgsmith searches.
pub fn default_dir() -> Result<PathBuf> {
    let data = crate::plugins::user_data_dir().ok_or_else(|| {
        anyhow!("the data folder of the user is not known (HOME, XDG_DATA_HOME or LOCALAPPDATA); name a folder with --dir")
    })?;
    Ok(data.join("pkgsmith/plugins"))
}

/// The digest file comes from the same server as the archive, so it only guards against a damaged download. The
/// connection must authenticate the server: HTTPS, or a loopback address for tests.
fn require_secure(url: &str) -> Result<()> {
    if url.starts_with("https://") {
        return Ok(());
    }
    let host = url
        .strip_prefix("http://")
        .map(|rest| rest.split(['/', ':']).next().unwrap_or(""))
        .unwrap_or("");
    if ["localhost", "127.0.0.1", "[::1]"].contains(&host) {
        return Ok(());
    }
    bail!("{url} is not an https URL. Plugins are code, so pkgsmith downloads them over https only")
}

fn download(url: &str) -> Result<Vec<u8>> {
    require_secure(url)?;
    let mut response = ureq::get(url)
        .call()
        .with_context(|| format!("cannot download {url}"))?;
    response
        .body_mut()
        .with_config()
        .limit(512 * 1024 * 1024)
        .read_to_vec()
        .with_context(|| format!("cannot read {url}"))
}

/// The digest `SHA256SUMS` lists for `file`.
fn listed_digest(sums: &str, file: &str) -> Option<String> {
    sums.lines().find_map(|line| {
        let (digest, name) = line.split_once(char::is_whitespace)?;
        (name.trim().trim_start_matches('*') == file).then(|| digest.to_string())
    })
}

/// Copy `pkgsmith-plugin-*` libraries from the `plugins/` folder of the archive into `dir`. Returns the files written.
fn extract(archive: &[u8], dir: &Path) -> Result<Vec<PathBuf>> {
    std::fs::create_dir_all(dir).with_context(|| format!("cannot create {}", dir.display()))?;
    let mut written = Vec::new();
    let mut tar = tar::Archive::new(flate2::read::GzDecoder::new(archive));
    for entry in tar.entries().context("the archive is not a gzip tar")? {
        let mut entry = entry?;
        let path = entry.path()?.into_owned();
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default()
            .to_string();
        let in_plugins = path
            .parent()
            .and_then(|p| p.file_name())
            .is_some_and(|d| d == "plugins");
        if !entry.header().entry_type().is_file()
            || !in_plugins
            || !name.starts_with("pkgsmith-plugin-")
            || !name.ends_with(crate::plugins::LIBRARY_SUFFIX)
        {
            continue;
        }
        let mut bytes = Vec::new();
        entry.read_to_end(&mut bytes)?;
        // Write to a temporary name and rename, so a running pkgsmith never maps a half-written library.
        let target = dir.join(&name);
        let temporary = dir.join(format!(".{name}.part"));
        std::fs::write(&temporary, &bytes)
            .with_context(|| format!("cannot write {}", temporary.display()))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&temporary, std::fs::Permissions::from_mode(0o755))?;
        }
        std::fs::rename(&temporary, &target)?;
        written.push(target);
    }
    if written.is_empty() {
        bail!(
            "the archive holds no plugins/pkgsmith-plugin-*{}",
            crate::plugins::LIBRARY_SUFFIX
        );
    }
    Ok(written)
}

/// Install the plugins of `version`. With `from`, read that archive instead of downloading one.
pub fn install(
    version: &str,
    dir: &Path,
    from: Option<&Path>,
    release_url: &str,
) -> Result<Vec<PathBuf>> {
    let archive = match from {
        Some(path) => {
            std::fs::read(path).with_context(|| format!("cannot read {}", path.display()))?
        }
        None => {
            let base = release_url.replace("{version}", version);
            let file = format!("pkgsmith-{}.tar.gz", target()?);
            let archive = download(&format!("{base}/{file}"))?;
            let sums = String::from_utf8(download(&format!("{base}/SHA256SUMS"))?)?;
            let want = listed_digest(&sums, &file)
                .ok_or_else(|| anyhow!("SHA256SUMS does not list {file}"))?;
            let got = hex(&Sha256::digest(&archive));
            if got != want {
                bail!("{file} has the digest {got}, but SHA256SUMS lists {want}");
            }
            archive
        }
    };
    extract(&archive, dir)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_https_and_loopback_urls_are_downloaded() {
        assert!(require_secure("https://github.com/x/releases/download/v1/a.tar.gz").is_ok());
        assert!(require_secure("http://127.0.0.1:8799/v1/a").is_ok());
        assert!(require_secure("http://localhost/v1/a").is_ok());
        assert!(require_secure("http://github.com/x").is_err());
        assert!(require_secure("http://127.0.0.1.evil.example/x").is_err());
        assert!(require_secure("ftp://127.0.0.1/x").is_err());
    }

    #[test]
    fn the_digest_comes_from_the_line_of_the_file_and_ignores_the_binary_marker() {
        let sums = "aa  other.tar.gz\nbb *pkgsmith-x.tar.gz\n";
        assert_eq!(
            listed_digest(sums, "pkgsmith-x.tar.gz").as_deref(),
            Some("bb")
        );
        assert_eq!(listed_digest(sums, "missing"), None);
    }
}
