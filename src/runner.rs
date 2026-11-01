//! Runs the tracked builds of a manifest and keeps their results, so a build that several formats share runs once,
//! also when those formats are built at the same time on different threads.

use crate::container::{Engine, Tracking};
use crate::manifest::{Job, Manifest};
use crate::ui::Ui;
use anyhow::{Result, anyhow};
use pkgsmith_plugin::Entry;
use pkgsmith_plugin::hash::hex;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};

type Slot = Arc<OnceLock<std::result::Result<Vec<Entry>, String>>>;

pub struct Runner {
    work: PathBuf,
    ui: Ui,
    cache: bool,
    engine: OnceLock<std::result::Result<Engine, String>>,
    /// One slot per distinct build. The thread that gets there first runs it. The others wait on the slot.
    builds: Mutex<HashMap<String, Slot>>,
}

/// What a build script can read. The format is left out on purpose: a build must not depend on it, so the formats of one
/// distro and architecture share one run. With `build_axes`, only those matrix values are exposed, so jobs of different
/// distros that are equal in them share one run as well.
pub fn build_env(manifest: &Manifest, job: &Job) -> BTreeMap<String, String> {
    let mut env = BTreeMap::new();
    env.insert("PKGSMITH_NAME".into(), manifest.name.clone());
    env.insert("PKGSMITH_VERSION".into(), manifest.version.clone());
    env.insert("PKGSMITH_RELEASE".into(), manifest.release.clone());
    env.insert("PKGSMITH_ARCH".into(), manifest.arch.clone());
    if manifest.build_axes.is_none() {
        env.insert(
            "PKGSMITH_DISTRO".into(),
            job.distro().unwrap_or_default().to_string(),
        );
    }
    for (key, value) in &job.values {
        let visible = match &manifest.build_axes {
            Some(axes) => axes.iter().any(|axis| axis == key),
            None => true,
        };
        if visible && !["format", "setup", "build", "file"].contains(&key.as_str()) {
            let key: String = key
                .chars()
                .map(|c| {
                    if c.is_ascii_alphanumeric() {
                        c.to_ascii_uppercase()
                    } else {
                        '_'
                    }
                })
                .collect();
            env.insert(format!("MATRIX_{key}"), value.clone());
        }
    }
    env
}

/// Identifies a tracked build. Jobs with the same key share one run. `None` for a manifest without an image.
pub fn build_key(manifest: &Manifest, job: &Job) -> Option<String> {
    let image = manifest.image.as_ref()?;
    let build = manifest.build.as_ref()?;
    let source =
        std::fs::canonicalize(&manifest.context).unwrap_or_else(|_| manifest.context.clone());
    Some(hex(&Sha256::digest(format!(
        "{image}\0{:?}\0{build}\0{:?}\0{}\0{:?}",
        manifest.setup,
        manifest.exclude,
        source.display(),
        build_env(manifest, job)
    ))))
}

/// The lane a job runs in: its distro and architecture, or the image name.
pub fn lane(manifest: &Manifest, job: &Job) -> String {
    let name = job.distro().map(String::from).unwrap_or_else(|| {
        manifest
            .image
            .as_deref()
            .map(|image| image.rsplit('/').next().unwrap_or(image).to_string())
            .unwrap_or_else(|| manifest.name.clone())
    });
    format!("{name}/{}", manifest.arch)
}

impl Runner {
    /// `cache` false rebuilds the setup images instead of reusing them.
    pub fn new(work: PathBuf, ui: Ui, cache: bool) -> Runner {
        Runner {
            work,
            ui,
            cache,
            engine: OnceLock::new(),
            builds: Mutex::new(HashMap::new()),
        }
    }

    fn engine(&self) -> Result<&Engine> {
        self.engine
            .get_or_init(|| Engine::connect().map_err(|error| format!("{error:#}")))
            .as_ref()
            .map_err(|error| anyhow!("{error}"))
    }

    /// The files the manifest's build installs, for this job. Empty when the manifest has no `image`.
    pub fn tracked(&self, manifest: &Manifest, job: &Job) -> Result<Vec<Entry>> {
        let Some(image) = &manifest.image else {
            return Ok(Vec::new());
        };
        let Some(build) = &manifest.build else {
            return Err(anyhow!(
                "the manifest names an image but no build script to run in it"
            ));
        };
        let env = build_env(manifest, job);
        let source = std::fs::canonicalize(&manifest.context)?;
        let key = build_key(manifest, job).expect("image and build are set above");
        let slot = self
            .builds
            .lock()
            .unwrap()
            .entry(key.clone())
            .or_default()
            .clone();
        let label = lane(manifest, job);
        slot.get_or_init(|| {
            (|| -> Result<Vec<Entry>> {
                let engine = self.engine()?;
                let prepared = engine.prepare(
                    &self.ui,
                    &label,
                    image,
                    manifest.setup.as_deref(),
                    &manifest.arch,
                    self.cache,
                )?;
                let stage = self
                    .work
                    .join(format!("{}-{}", manifest.name, &key[..10]))
                    .join("stage");
                std::fs::create_dir_all(&self.work)?;
                let built = engine.track(
                    &self.ui,
                    &Tracking {
                        label: &label,
                        image: &prepared,
                        build,
                        source: &source,
                        env: &env,
                        arch: &manifest.arch,
                        excludes: &manifest.exclude,
                        stage: &stage,
                        id: &key[..10],
                    },
                )?;
                Ok(built.entries)
            })()
            .map_err(|error| format!("{error:#}"))
        })
        .clone()
        .map_err(|error| anyhow!("{error}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::expand::Scope;

    fn keys(extra: &str) -> Vec<String> {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("pkg.yaml");
        std::fs::write(
            &path,
            format!(
                "name: demo\nversion: 1.0.0\ndescription: d\nimage: debian:13\nbuild: make\n{extra}matrix:\n  arch: [amd64, arm64]\n  distro: [fedora, debian]\n"
            ),
        )
        .unwrap();
        let manifest = Manifest::parse(&path).unwrap();
        let mut keys = Vec::new();
        for job in manifest.jobs() {
            let bound = manifest.bind(&Scope::default(), &job, "rpm").unwrap();
            keys.push(build_key(&bound, &job).unwrap());
        }
        keys
    }

    #[test]
    fn a_build_sees_every_matrix_value_by_default_so_each_distro_builds_alone() {
        let keys = keys("");
        assert_eq!(keys.len(), 4);
        assert_eq!(
            keys.iter().collect::<std::collections::HashSet<_>>().len(),
            4
        );
    }

    #[test]
    fn build_axes_lets_jobs_of_different_distros_share_a_build_but_never_architectures() {
        let keys = keys("build_axes: [arch]\n");
        assert_eq!(
            keys.iter().collect::<std::collections::HashSet<_>>().len(),
            2
        );
    }
}
