//! The system pkgsmith runs on. A script reads it with `host()` and adapts: which formats to write, which packages to
//! depend on, which commands to run.

use serde::Serialize;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Host {
    /// `linux`, `macos` or `windows`.
    pub os: String,
    /// `amd64`, `arm64`, or the name the system gives for another processor.
    pub arch: String,
    /// The `ID` of /etc/os-release on Linux, for example `fedora`, `debian`, `ubuntu`, `arch`, `alpine`. `macos` and
    /// `windows` elsewhere.
    pub distro: String,
    /// The `ID_LIKE` of /etc/os-release: the distros this one derives from. `ubuntu` lists `debian`.
    pub like: Vec<String>,
    /// The `VERSION_ID` of /etc/os-release, or an empty text.
    pub version: String,
    /// `glibc` or `musl` on Linux, an empty text elsewhere.
    pub libc: String,
    /// The package format this system installs, as it is named in `pkgsmith formats`. Empty when there is none.
    pub format: String,
}

/// The fields of an os-release file: `KEY=value` or `KEY="value"` lines.
pub fn os_release(text: &str, key: &str) -> Option<String> {
    text.lines().find_map(|line| {
        let (name, value) = line.split_once('=')?;
        (name.trim() == key).then(|| value.trim().trim_matches(['"', '\'']).to_string())
    })
}

/// The package format of a distro and the distros it derives from.
pub fn native_format(distro: &str, like: &[String]) -> &'static str {
    let is = |names: &[&str]| {
        names
            .iter()
            .any(|n| distro == *n || like.iter().any(|l| l == n))
    };
    if is(&["debian", "ubuntu"]) {
        "deb"
    } else if is(&["fedora", "rhel", "centos", "suse", "opensuse"]) {
        "rpm"
    } else if is(&["arch"]) {
        "archlinux"
    } else if is(&["alpine"]) {
        "apk"
    } else {
        ""
    }
}

impl Host {
    pub fn from_os_release(text: &str, arch: &str, libc: &str) -> Host {
        let distro = os_release(text, "ID").unwrap_or_else(|| "linux".into());
        let like: Vec<String> = os_release(text, "ID_LIKE")
            .unwrap_or_default()
            .split_whitespace()
            .map(String::from)
            .collect();
        Host {
            os: "linux".into(),
            arch: arch.into(),
            format: native_format(&distro, &like).into(),
            version: os_release(text, "VERSION_ID").unwrap_or_default(),
            libc: libc.into(),
            distro,
            like,
        }
    }

    pub fn detect() -> Host {
        let arch = match std::env::consts::ARCH {
            "x86_64" => "amd64",
            "aarch64" => "arm64",
            other => other,
        };
        match std::env::consts::OS {
            "linux" => {
                let text = std::fs::read_to_string("/etc/os-release")
                    .or_else(|_| std::fs::read_to_string("/usr/lib/os-release"))
                    .unwrap_or_default();
                let musl = std::fs::read_dir("/lib")
                    .into_iter()
                    .flatten()
                    .flatten()
                    .any(|e| e.file_name().to_string_lossy().starts_with("ld-musl-"));
                Host::from_os_release(&text, arch, if musl { "musl" } else { "glibc" })
            }
            os => {
                let (os, distro, format) = match os {
                    "macos" => ("macos", "macos", "dmg"),
                    "windows" => ("windows", "windows", "msi"),
                    other => (other, other, ""),
                };
                Host {
                    os: os.into(),
                    arch: arch.into(),
                    distro: distro.into(),
                    like: Vec::new(),
                    version: String::new(),
                    libc: String::new(),
                    format: format.into(),
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn os_release_gives_the_distro_its_family_and_native_format() {
        let ubuntu = "NAME=\"Ubuntu\"\nID=ubuntu\nID_LIKE=debian\nVERSION_ID=\"24.04\"\n";
        let host = Host::from_os_release(ubuntu, "amd64", "glibc");
        assert_eq!(
            (
                host.distro.as_str(),
                host.format.as_str(),
                host.version.as_str()
            ),
            ("ubuntu", "deb", "24.04")
        );
        assert_eq!(host.like, ["debian"]);
        let rocky = Host::from_os_release(
            "ID=\"rocky\"\nID_LIKE=\"rhel centos fedora\"\n",
            "arm64",
            "glibc",
        );
        assert_eq!(rocky.format, "rpm");
        assert_eq!(
            Host::from_os_release("ID=alpine\n", "amd64", "musl").format,
            "apk"
        );
        assert_eq!(
            Host::from_os_release("ID=cachyos\nID_LIKE=arch\n", "amd64", "glibc").format,
            "archlinux"
        );
        assert_eq!(
            Host::from_os_release("ID=nixos\n", "amd64", "glibc").format,
            ""
        );
    }
}
