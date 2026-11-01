//! Runs the tracked builds of a manifest and keeps their results, so a build that several formats share runs once,
//! also when those formats are built at the same time on different threads.

use crate::container::{Engine, Tracking};
use crate::manifest::{Job, Manifest};
use crate::ui::Ui;
use anyhow::{Result, anyhow};
use pkgsmith_plugin::Entry;
use pkgsmith_plugin::hash::hex;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
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

/// Identifies a tracked build. Jobs with the same key share one run. `None` for a manifest without an image.
pub fn build_key(manifest: &Manifest) -> Option<String> {
    let image = manifest.image.as_ref()?;
    let build = manifest.build.as_ref()?;
    let source =
        std::fs::canonicalize(&manifest.context).unwrap_or_else(|_| manifest.context.clone());
    Some(hex(&Sha256::digest(format!(
        "{image}\0{:?}\0{build}\0{:?}\0{}\0{:?}",
        manifest.setup,
        manifest.exclude,
        source.display(),
        manifest.build_env
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

    /// The files the manifest's build installs, for this target. Empty when it has no job.
    pub fn tracked(&self, manifest: &Manifest, job: &Job) -> Result<Vec<Entry>> {
        let Some(image) = &manifest.image else {
            return Ok(Vec::new());
        };
        let Some(build) = &manifest.build else {
            return Err(anyhow!(
                "the target names an image but no build script to run in it"
            ));
        };
        let env = &manifest.build_env;
        let source = std::fs::canonicalize(&manifest.context)?;
        let key = build_key(manifest).expect("image and build are set above");
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
                        env,
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
    use crate::script;
    use std::collections::{BTreeMap, HashSet};

    /// The build key of every target of a script.
    fn keys(source: &str) -> Vec<String> {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("pkgsmith.star");
        std::fs::write(&file, source).unwrap();
        script::evaluate(&file, BTreeMap::new())
            .unwrap()
            .into_iter()
            .map(|spec| {
                let manifest =
                    Manifest::from_spec(spec, dir.path().into(), dir.path().into()).unwrap();
                build_key(&manifest).unwrap()
            })
            .collect()
    }

    fn distinct(keys: &[String]) -> usize {
        keys.iter().collect::<HashSet<_>>().len()
    }

    const HEAD: &str = "pkg = package(name = 'demo', version = '1', description = 'd')
";

    #[test]
    fn targets_with_equal_jobs_share_a_run_per_architecture() {
        let keys = keys(&format!(
            "{HEAD}glibc = job('debian:13', [step('make')])
musl = job('alpine:3.22', [step('make')])
for arch in ['amd64', 'arm64']:
    target(pkg, 'rpm', distro = 'fedora', job = glibc, arch = arch)
    target(pkg, 'deb', distro = 'debian', job = glibc, arch = arch)
    target(pkg, 'apk', distro = 'alpine', job = musl, arch = arch)
"
        ));
        assert_eq!(keys.len(), 6);
        assert_eq!(
            distinct(&keys),
            4,
            "glibc and musl, each for two architectures"
        );
    }

    #[test]
    fn a_job_is_equal_by_content_and_not_by_name() {
        let keys = keys(&format!(
            "{HEAD}one = job('debian:13', [step('make')])
two = job('debian:13', [step('make')])
three = job('debian:13', [step('make CC=clang')])
target(pkg, 'rpm', job = one)
target(pkg, 'deb', job = two)
target(pkg, 'tar', job = three)
"
        ));
        assert_eq!(distinct(&keys), 2);
    }
}
