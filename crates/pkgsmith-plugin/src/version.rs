//! One semantic version, written the way each package format orders it.
//!
//! A release is `MAJOR.MINOR.PATCH`. A pre-release such as the nightly `0.1.13-nightly.20261031.142.gabc1234` must sort
//! after the previous release and before the release it leads up to. Each format spells that differently, so the tool
//! converts the one version in the manifest instead of making the author write several.

use anyhow::{Result, bail};

/// The tokens of a pre-release without the commit identifier (`g` and 7 to 40 hex digits), which does not order builds.
fn without_commit(pre: &str) -> Vec<&str> {
    let is_commit = |token: &str| {
        (8..=41).contains(&token.len())
            && token.starts_with('g')
            && token[1..].chars().all(|c| c.is_ascii_hexdigit())
    };
    pre.split(['.', '-']).filter(|t| !is_commit(t)).collect()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Version {
    pub core: String,
    pub pre: Option<String>,
}

impl Version {
    pub fn parse(text: &str) -> Result<Version> {
        let (core, pre) = match text.split_once('-') {
            Some((core, pre)) => (core, Some(pre)),
            None => (text, None),
        };
        let numbers: Vec<&str> = core.split('.').collect();
        let numeric = numbers.len() == 3
            && numbers
                .iter()
                .all(|n| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()));
        let identifier = |c: char| c.is_ascii_alphanumeric() || c == '.' || c == '-';
        if !numeric || pre.is_some_and(|p| p.is_empty() || !p.chars().all(identifier)) {
            bail!("version {text:?} must be MAJOR.MINOR.PATCH or MAJOR.MINOR.PATCH-prerelease");
        }
        Ok(Version {
            core: core.into(),
            pre: pre.map(String::from),
        })
    }

    /// rpm and deb: `0.1.13~nightly.20261031.142.gabc1234`. The tilde sorts before the empty string.
    pub fn tilde(&self) -> String {
        match &self.pre {
            Some(pre) => format!("{}~{pre}", self.core),
            None => self.core.clone(),
        }
    }

    /// Arch: `0.1.13nightly20261031.142`. pacman compares letters and digits in runs, so the commit identifier goes
    /// (the date and the commit count already order builds) and a name joins the number after it. The pkgver may not
    /// hold a hyphen.
    pub fn arch(&self) -> String {
        let Some(pre) = &self.pre else {
            return self.core.clone();
        };
        let tokens = without_commit(pre);
        let mut text = String::from(tokens[0]);
        for pair in tokens.windows(2) {
            let (previous, token) = (pair[0], pair[1]);
            let joins = previous.chars().all(|c| c.is_ascii_alphabetic())
                && token.chars().all(|c| c.is_ascii_digit());
            if !joins {
                text.push('.');
            }
            text.push_str(token);
        }
        format!("{}{text}", self.core)
    }

    /// Alpine: `0.1.13_pre20261031142`. The suffix `_pre` sorts before the release, and the digits order the builds.
    pub fn apk(&self) -> String {
        let Some(pre) = &self.pre else {
            return self.core.clone();
        };
        let digits: String = without_commit(pre)
            .concat()
            .chars()
            .filter(char::is_ascii_digit)
            .collect();
        if digits.is_empty() {
            format!("{}_pre", self.core)
        } else {
            format!("{}_pre{digits}", self.core)
        }
    }

    /// Formats that take any text (snap, flatpak, nix, tar): the version as written.
    pub fn plain(&self) -> String {
        match &self.pre {
            Some(pre) => format!("{}-{pre}", self.core),
            None => self.core.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NIGHTLY: &str = "0.1.13-nightly.20261031.142.gabc1234";

    #[test]
    fn a_nightly_is_spelled_per_format() {
        let version = Version::parse(NIGHTLY).unwrap();
        assert_eq!(version.tilde(), "0.1.13~nightly.20261031.142.gabc1234");
        assert_eq!(version.arch(), "0.1.13nightly20261031.142");
        assert_eq!(version.apk(), "0.1.13_pre20261031142");
        assert_eq!(version.plain(), NIGHTLY);
    }

    #[test]
    fn a_release_is_the_same_everywhere() {
        let version = Version::parse("0.1.12").unwrap();
        assert_eq!(
            (
                version.tilde(),
                version.arch(),
                version.apk(),
                version.plain()
            ),
            (
                "0.1.12".into(),
                "0.1.12".into(),
                "0.1.12".into(),
                "0.1.12".into()
            )
        );
    }

    #[test]
    fn text_that_is_not_a_version_is_refused() {
        for bad in ["1.2", "v1.2.3", "1.2.3-", "1.2.x", "1.2.3-a b"] {
            assert!(Version::parse(bad).is_err(), "{bad}");
        }
    }
}
