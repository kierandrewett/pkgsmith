//! What a build will do: the targets of a script, narrowed by what was asked for.

use crate::manifest::{Job, Manifest, TargetSpec};
use anyhow::{Result, bail};
use std::path::Path;

/// One package to write.
#[derive(Debug, Clone)]
pub struct Item {
    pub job: Job,
    pub format: String,
    pub manifest: Manifest,
}

impl Item {
    pub fn label(&self) -> String {
        format!("{} [{}]", self.format, self.job.label())
    }
}

/// What the command line selects.
#[derive(Debug, Default)]
pub struct Selection {
    /// Formats to write. Empty means every format the script declares.
    pub formats: Vec<String>,
    /// `KEY=VALUE` pairs that a target must match: `format`, `distro`, `arch`, `name` or a key of its `tags`.
    pub filters: Vec<(String, String)>,
}

/// Every package the targets describe. `known` are the formats the loaded plugins write. `base` is the folder of the
/// script and `context` the folder that a tracked build mounts at `/src`.
pub fn items(
    specs: Vec<TargetSpec>,
    base: &Path,
    context: &Path,
    known: &[String],
    selection: &Selection,
) -> Result<Vec<Item>> {
    if let Some(unknown) = selection.formats.iter().find(|f| !known.contains(f)) {
        bail!(
            "unknown format {unknown:?}; installed: {}",
            known.join(", ")
        );
    }
    let mut out = Vec::new();
    for (n, spec) in specs.into_iter().enumerate() {
        if !known.contains(&spec.format) {
            bail!(
                "target {} ({}) asks for the format {:?}; installed: {}",
                n + 1,
                spec.name,
                spec.format,
                known.join(", ")
            );
        }
        let format = spec.format.clone();
        let mut values = spec.tags.clone();
        values.insert("format".into(), format.clone());
        values.insert("name".into(), spec.name.clone());
        if let Some(distro) = &spec.distro {
            values.insert("distro".into(), distro.clone());
        }
        if let Some(file) = &spec.file {
            values.insert("file".into(), file.clone());
        }
        let label = format!("target {} ({} as {format})", n + 1, spec.name);
        let manifest = Manifest::from_spec(spec, base.to_path_buf(), context.to_path_buf())
            .map_err(|error| anyhow::anyhow!("{label}: {error:#}"))?;
        values.insert("arch".into(), manifest.arch.clone());
        let job = Job { values };
        let wanted = selection.formats.is_empty() || selection.formats.contains(&format);
        let matches = selection
            .filters
            .iter()
            .all(|(key, value)| job.values.get(key) == Some(value));
        if wanted && matches {
            out.push(Item {
                job,
                format,
                manifest,
            });
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn specs() -> Vec<TargetSpec> {
        let target = |format: &str, distro: &str, arch: &str| {
            Manifest::parse_spec(json!({
                "name": "demo", "version": "1", "description": "d", "format": format,
                "distro": distro, "arch": arch, "tags": {"stage": "one"},
            }))
            .unwrap()
        };
        vec![
            target("deb", "debian", "amd64"),
            target("deb", "debian", "arm64"),
            target("rpm", "fedora", "amd64"),
            target("tar", "any", "all"),
        ]
    }

    fn known() -> Vec<String> {
        ["tar", "deb", "rpm"].map(String::from).to_vec()
    }

    fn labels(selection: Selection) -> Vec<String> {
        items(
            specs(),
            Path::new("."),
            Path::new("."),
            &known(),
            &selection,
        )
        .unwrap()
        .iter()
        .map(Item::label)
        .collect()
    }

    #[test]
    fn formats_filters_and_architectures_narrow_the_targets() {
        assert_eq!(labels(Selection::default()).len(), 4);
        let deb = labels(Selection {
            formats: vec!["deb".into()],
            ..Selection::default()
        });
        assert_eq!(
            deb,
            [
                "deb [arch=amd64, distro=debian, format=deb, name=demo, stage=one]",
                "deb [arch=arm64, distro=debian, format=deb, name=demo, stage=one]"
            ]
        );
        let arm = labels(Selection {
            filters: vec![
                ("distro".into(), "debian".into()),
                ("arch".into(), "arm64".into()),
            ],
            ..Selection::default()
        });
        assert_eq!(
            arm,
            ["deb [arch=arm64, distro=debian, format=deb, name=demo, stage=one]"]
        );
    }

    #[test]
    fn a_format_that_no_plugin_writes_is_refused_by_name() {
        let error = items(
            specs(),
            Path::new("."),
            Path::new("."),
            &["tar".to_string()],
            &Selection::default(),
        )
        .unwrap_err();
        assert!(format!("{error:#}").contains("\"deb\""), "{error:#}");
        let selection = Selection {
            formats: vec!["msi".into()],
            ..Selection::default()
        };
        assert!(
            items(
                specs(),
                Path::new("."),
                Path::new("."),
                &known(),
                &selection
            )
            .is_err()
        );
    }
}
