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

/// What a build script can read: `PKGSMITH_*` and the `MATRIX_*` of the cell of its own job, never the format or the
/// distro. That is what lets the targets of several distros share one run. `bind` fills it in.
pub fn build_env(manifest: &Manifest, _job: &Job) -> BTreeMap<String, String> {
    manifest.build_env.clone()
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

    /// The build key of every package of a manifest with the given text after the header.
    fn keys(text: &str) -> Vec<String> {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("pkg.yaml");
        std::fs::write(
            &path,
            format!("name: demo\nversion: 1.0.0\ndescription: d\n{text}"),
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

    fn distinct(keys: &[String]) -> usize {
        keys.iter().collect::<std::collections::HashSet<_>>().len()
    }

    const TARGETS: &str = "targets:\n  - {format: rpm, distro: fedora, needs: glibc}\n  - {format: deb, distro: debian, needs: glibc}\n  - {format: apk, distro: alpine, needs: musl}\nmatrix:\n  arch: [amd64, arm64]\n";

    #[test]
    fn targets_that_need_one_job_share_a_run_per_architecture() {
        let keys = keys(&format!(
            "jobs:\n  glibc:\n    runs-on: debian:13\n    steps:\n      - run: make\n  musl:\n    runs-on: alpine:3.22\n    steps:\n      - run: make\n{TARGETS}"
        ));
        assert_eq!(keys.len(), 6);
        assert_eq!(distinct(&keys), 4, "linux x 2 architectures, alpine x 2");
    }

    #[test]
    fn two_jobs_with_the_same_image_and_steps_still_share_nothing_by_name_alone() {
        let keys = keys(&format!(
            "jobs:\n  glibc:\n    runs-on: debian:13\n    steps:\n      - run: make\n  musl:\n    runs-on: debian:13\n    steps:\n      - run: make\n{TARGETS}"
        ));
        assert_eq!(
            distinct(&keys),
            2,
            "equal image and steps are one build whatever the job is called"
        );
    }

    #[test]
    fn a_job_with_its_own_matrix_makes_a_run_per_cell() {
        let keys = keys(
            "jobs:\n  glibc:\n    runs-on: debian:13\n    matrix:\n      libc: [gnu, musl]\n    steps:\n      - run: make $MATRIX_LIBC\ntargets:\n  - {format: rpm, needs: glibc, libc: gnu}\n  - {format: apk, needs: glibc, libc: musl}\n",
        );
        assert_eq!(distinct(&keys), 2);
    }
}
