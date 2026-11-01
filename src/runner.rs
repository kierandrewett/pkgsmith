//! Runs the builds of a script and keeps their results, so a build that several formats share runs once, also when those
//! formats are built at the same time on different threads.
//!
//! A build runs on this system, like makepkg: in the build context (`$srcdir`), with `$pkgdir` an empty folder that the
//! steps install into. The files in `$pkgdir` are the package.

use crate::manifest::{Job, Manifest};
use crate::tree;
use crate::ui::Ui;
use anyhow::{Context, Result, anyhow, bail};
use pkgsmith_plugin::Entry;
use pkgsmith_plugin::hash::hex;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

type Slot = Arc<OnceLock<std::result::Result<Vec<Entry>, String>>>;

pub struct Runner {
    work: PathBuf,
    ui: Ui,
    /// One slot per distinct build. The thread that gets there first runs it. The others wait on the slot.
    builds: Mutex<HashMap<String, Slot>>,
}

/// Identifies a build. Targets with the same key share one run. `None` for a target without a build.
pub fn build_key(manifest: &Manifest) -> Option<String> {
    let build = manifest.build.as_ref()?;
    let source =
        std::fs::canonicalize(&manifest.context).unwrap_or_else(|_| manifest.context.clone());
    Some(hex(&Sha256::digest(format!(
        "{build}\0{:?}\0{}\0{:?}",
        manifest.exclude,
        source.display(),
        manifest.build_env
    ))))
}

/// The lane a target is built in, in the progress output: its distro and architecture.
pub fn lane(manifest: &Manifest, job: &Job) -> String {
    format!("{}/{}", job.distro().unwrap_or("host"), manifest.arch)
}

impl Runner {
    pub fn new(work: PathBuf, ui: Ui) -> Runner {
        Runner {
            work,
            ui,
            builds: Mutex::new(HashMap::new()),
        }
    }

    /// The files the build of the target installs. Empty when the target has no build.
    pub fn tracked(&self, manifest: &Manifest, job: &Job) -> Result<Vec<Entry>> {
        let Some(build) = &manifest.build else {
            return Ok(Vec::new());
        };
        let key = build_key(manifest).expect("build is set above");
        let slot = self
            .builds
            .lock()
            .unwrap()
            .entry(key.clone())
            .or_default()
            .clone();
        let label = lane(manifest, job);
        slot.get_or_init(|| {
            let folder = self.work.join(format!("{}-{}", manifest.name, &key[..10]));
            self.run(manifest, build, &label, &folder)
                .map_err(|error| format!("{error:#}"))
        })
        .clone()
        .map_err(|error| anyhow!("{error}"))
    }

    /// Run the build. `folder` holds `pkg`, the `$pkgdir`, and `state`, where the steps pass variables to each other.
    fn run(
        &self,
        manifest: &Manifest,
        build: &str,
        label: &str,
        folder: &Path,
    ) -> Result<Vec<Entry>> {
        use std::io::{BufRead, BufReader};
        use std::process::{Command, Stdio};
        let (pkgdir, state) = (folder.join("pkg"), folder.join("state"));
        if folder.exists() {
            std::fs::remove_dir_all(folder)?;
        }
        std::fs::create_dir_all(&pkgdir)?;
        let source = std::fs::canonicalize(&manifest.context)?;
        let step = self.ui.step(label, "build");
        let mut child = Command::new("sh")
            .arg("-c")
            .arg(format!("exec 2>&1\n{build}"))
            .current_dir(&source)
            .envs(&manifest.build_env)
            .env("srcdir", &source)
            .env("pkgdir", std::fs::canonicalize(&pkgdir)?)
            .env("PKGSMITH_STATE", &state)
            .stdout(Stdio::piped())
            .spawn()
            .context("cannot start sh")?;
        for line in BufReader::new(child.stdout.take().expect("piped")).lines() {
            step.log(&line?);
        }
        let status = child.wait()?;
        let _ = std::fs::remove_dir_all(&state);
        if !status.success() {
            step.fail();
            bail!("the build failed ({status})");
        }
        step.done();
        let mut entries = tree::stage_entries(&pkgdir)?;
        entries.retain(|entry| !tree::excluded(&entry.path, &manifest.exclude));
        Ok(entries)
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
        let host = crate::host::Host::from_os_release("ID=fedora\n", "amd64", "glibc");
        script::evaluate_on(&file, BTreeMap::new(), host)
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

    const HEAD: &str = "c = {'name': 'demo', 'version': '1', 'description': 'd'}\n";

    #[test]
    fn packages_after_one_build_share_a_run_and_a_longer_build_is_another_run() {
        let keys = keys(&format!(
            "{HEAD}sh('make')\nfor f in ['rpm', 'deb', 'tar']:\n    package(f, **c)\nsh('make docs')\npackage('zip', **c)\n"
        ));
        assert_eq!(keys.len(), 4);
        assert_eq!(distinct(&keys[..3]), 1);
        assert_eq!(distinct(&keys), 2);
    }

    #[test]
    fn a_different_command_or_environment_is_a_different_run() {
        let one = keys(&format!("{HEAD}sh('make')\npackage('rpm', **c)\n"));
        let other_step = keys(&format!("{HEAD}sh('make CC=clang')\npackage('rpm', **c)\n"));
        let other_env = keys(&format!(
            "{HEAD}sh('make', env = {{'A': '1'}})\npackage('rpm', **c)\n"
        ));
        assert_eq!(
            distinct(&[one[0].clone(), other_step[0].clone(), other_env[0].clone()]),
            3
        );
    }
}
