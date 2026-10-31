//! What a build will do: the matrix of a manifest crossed with the formats that were asked for.

use crate::manifest::{Job, Manifest};
use anyhow::{Result, bail};

/// One build: a matrix combination and one format.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    pub job: Job,
    pub format: String,
}

impl Item {
    pub fn label(&self) -> String {
        format!("{} [{}]", self.format, self.job.label())
    }
}

/// Every build the manifest describes. A job that names its `format` runs that format only. A job without one runs each
/// format in `wanted` (all formats when `wanted` is empty). `wanted` also drops jobs whose format it does not list.
/// `known` are the formats the loaded plugins write. `filters` are `key=value` pairs that a job must match, like the matrix values of `--job arch=arm64`.
pub fn items(
    manifest: &Manifest,
    known: &[String],
    wanted: &[String],
    filters: &[(String, String)],
    default_distro: Option<&str>,
) -> Result<Vec<Item>> {
    if let Some(unknown) = wanted.iter().find(|f| !known.contains(f)) {
        bail!(
            "unknown format {unknown:?}; installed: {}",
            known.join(", ")
        );
    }
    let everything = known.to_vec();
    let mut out = Vec::new();
    for mut job in manifest.jobs() {
        if let Some(distro) = default_distro {
            job.values
                .entry("distro".into())
                .or_insert_with(|| distro.to_string());
        }
        if !filters
            .iter()
            .all(|(key, value)| job.values.get(key) == Some(value))
        {
            continue;
        }
        let formats: Vec<String> = match job.format() {
            Some(format) if wanted.is_empty() || wanted.iter().any(|w| w == format) => {
                vec![format.to_string()]
            }
            Some(_) => continue,
            None if wanted.is_empty() => everything.clone(),
            None => wanted.to_vec(),
        };
        for format in formats {
            if !known.contains(&format) {
                bail!(
                    "the matrix asks for the format {format:?}; installed: {}",
                    known.join(", ")
                );
            }
            out.push(Item {
                job: job.clone(),
                format,
            });
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest(matrix: &str) -> Manifest {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("pkg.yaml");
        std::fs::write(
            &path,
            format!("name: demo\nversion: 1.0.0\ndescription: d\nmatrix:\n{matrix}"),
        )
        .unwrap();
        Manifest::parse(&path).unwrap()
    }

    fn known() -> Vec<String> {
        ["tar", "deb", "rpm"].map(String::from).to_vec()
    }

    fn labels(items: Vec<Item>) -> Vec<String> {
        items.iter().map(Item::label).collect()
    }

    #[test]
    fn a_format_axis_fixes_the_format_and_the_command_line_narrows_it() {
        let manifest = manifest("  arch: [amd64, arm64]\n  format: [deb, tar]\n");
        assert_eq!(items(&manifest, &known(), &[], &[], None).unwrap().len(), 4);
        let only_deb = items(&manifest, &known(), &["deb".into()], &[], None).unwrap();
        assert_eq!(
            labels(only_deb),
            [
                "deb [arch=amd64, format=deb]",
                "deb [arch=arm64, format=deb]"
            ]
        );
    }

    #[test]
    fn without_a_format_axis_each_requested_format_runs_for_every_job() {
        let manifest = manifest("  arch: [amd64, arm64]\n");
        let all = items(
            &manifest,
            &known(),
            &["deb".into(), "tar".into()],
            &[],
            None,
        )
        .unwrap();
        assert_eq!(all.len(), 4);
    }

    #[test]
    fn job_filters_select_by_value_and_an_unknown_format_is_refused() {
        let manifest = manifest("  arch: [amd64, arm64]\n");
        let filtered = items(
            &manifest,
            &known(),
            &["tar".into()],
            &[("arch".into(), "arm64".into())],
            None,
        )
        .unwrap();
        assert_eq!(labels(filtered), ["tar [arch=arm64]"]);
        assert!(items(&manifest, &known(), &["msi".into()], &[], None).is_err());
    }

    #[test]
    fn a_default_distro_fills_in_jobs_that_name_none() {
        let manifest =
            manifest("  arch: [amd64]\n  include:\n    - {arch: amd64, distro: opensuse}\n");
        let jobs = items(&manifest, &known(), &["tar".into()], &[], Some("fedora")).unwrap();
        assert_eq!(jobs[0].job.distro(), Some("opensuse"));
    }
}
