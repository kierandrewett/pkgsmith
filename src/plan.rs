//! What a build will do: the matrix of a manifest crossed with the formats that were asked for.

use crate::expand::Scope;
use crate::expr;
use crate::manifest::{Job, Manifest};
use anyhow::{Context, Result, bail};

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
///
/// A matrix `include` entry may carry `if:`, an expression like the `if:` of a `contents` entry. It sees `env` (the
/// environment and `--build-arg`), the matrix values, `format`, `distro` and `arch`. A job whose condition is false is
/// dropped, and `if` is not a matrix value.
pub fn items(
    manifest: &Manifest,
    known: &[String],
    wanted: &[String],
    filters: &[(String, String)],
    default_distro: Option<&str>,
    scope: &Scope,
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
        let condition = job.values.remove("if");
        if !filters
            .iter()
            .all(|(key, value)| job.values.get(key) == Some(value))
        {
            continue;
        }
        if let Some(condition) = condition {
            let mut context = scope.clone();
            context.matrix = job.values.clone();
            context.distro = job.distro().unwrap_or_default().to_string();
            context.arch = job.arch().unwrap_or_default().to_string();
            context.format = job.format().unwrap_or_default().to_string();
            let keep = expr::eval(&condition, &context)
                .with_context(|| format!("the matrix entry has the condition if: {condition}"))?;
            if !keep.truthy() {
                continue;
            }
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
        assert_eq!(
            items(&manifest, &known(), &[], &[], None, &Scope::default())
                .unwrap()
                .len(),
            4
        );
        let only_deb = items(
            &manifest,
            &known(),
            &["deb".into()],
            &[],
            None,
            &Scope::default(),
        )
        .unwrap();
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
            &Scope::default(),
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
            &Scope::default(),
        )
        .unwrap();
        assert_eq!(labels(filtered), ["tar [arch=arm64]"]);
        assert!(
            items(
                &manifest,
                &known(),
                &["msi".into()],
                &[],
                None,
                &Scope::default()
            )
            .is_err()
        );
    }

    #[test]
    fn a_default_distro_fills_in_jobs_that_name_none() {
        let manifest =
            manifest("  arch: [amd64]\n  include:\n    - {arch: amd64, distro: opensuse}\n");
        let jobs = items(
            &manifest,
            &known(),
            &["tar".into()],
            &[],
            Some("fedora"),
            &Scope::default(),
        )
        .unwrap();
        assert_eq!(jobs[0].job.distro(), Some("opensuse"));
    }

    #[test]
    fn an_include_entry_with_a_false_condition_is_dropped_and_if_is_not_a_matrix_value() {
        let manifest = manifest(
            "  arch: [amd64]\n  include:\n    - { format: dmg, distro: macos, arch: arm64, if: env.STAGE }\n",
        );
        let without = items(
            &manifest,
            &known_with_dmg(),
            &[],
            &[],
            None,
            &Scope::default(),
        )
        .unwrap();
        assert!(
            without.iter().all(|i| i.job.distro() != Some("macos")),
            "the gated entry is dropped"
        );
        let mut staged = Scope::default();
        staged.set("STAGE", "stage");
        let with = items(&manifest, &known_with_dmg(), &[], &[], None, &staged).unwrap();
        let dmg: Vec<_> = with
            .iter()
            .filter(|i| i.job.distro() == Some("macos"))
            .collect();
        assert_eq!(dmg.len(), 1);
        assert!(!dmg[0].job.values.contains_key("if"));
        assert_eq!(dmg[0].label(), "dmg [arch=arm64, distro=macos, format=dmg]");
    }

    #[test]
    fn a_condition_that_cannot_be_evaluated_is_an_error() {
        let manifest = manifest("  include:\n    - { format: tar, if: \"(\" }\n");
        assert!(items(&manifest, &known(), &[], &[], None, &Scope::default()).is_err());
    }

    fn known_with_dmg() -> Vec<String> {
        ["tar", "deb", "rpm", "dmg"].map(String::from).to_vec()
    }
}
