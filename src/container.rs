//! The build environment: a container that runs the build and shows which files the build installed.
//!
//! The manifest names an image, an optional `setup` script and a `build` script. `setup` runs once and is cached as an
//! image of its own, because installing build dependencies touches thousands of files that do not belong in the
//! package. `build` runs on top of that image with the source folder mounted at `/src` as a throw-away overlay, so
//! builds may write into it without changing the checkout. When `build` ends, the container's changes against its
//! image are exactly the files the build installed. They are exported into a stage folder, and the formats package that.

use crate::ui::{LineSplitter, Step, Ui};
use anyhow::{Context, Result, bail};
use futures_util::StreamExt;
use pkgsmith_plugin::hash::hex;
use pkgsmith_plugin::{Data, Entry};
use podman_api::Podman;
use podman_api::conn::TtyChunk;
use podman_api::models::{ContainerMount, ContainerStatus, OverlayVolume};
use podman_api::opts::{
    ContainerCommitOpts, ContainerCreateOpts, ContainerDeleteOpts, ContainerLogsOpts,
    ContainerWaitOpts, ImageListOpts, PullOpts,
};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::io::Write;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

/// Folders and files that builds and package managers touch but no package should carry.
pub const DEFAULT_EXCLUDES: &[&str] = &[
    "/tmp",
    "/var/tmp",
    "/root",
    "/run",
    "/proc",
    "/sys",
    "/dev",
    "/var/cache",
    "/var/log",
    "/var/lib/dnf",
    "/var/lib/rpm",
    "/var/lib/pacman",
    "/var/lib/dpkg",
    "/var/lib/apk",
    "/var/lib/zypp",
    "/usr/lib/sysimage",
    "/etc/ld.so.cache",
    "/etc/mtab",
    "/etc/resolv.conf",
    "/etc/hosts",
    "/etc/hostname",
    "/lib/apk",
    "/pkgsmith-script",
    "/src",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Change {
    Added,
    Changed,
}

/// Parse `podman diff` output (`A /path`, `C /path`, `D /path`). Deleted paths are dropped.
pub fn parse_diff(output: &str) -> Vec<(Change, String)> {
    output
        .lines()
        .filter_map(|line| {
            let (kind, path) = line.split_once(' ')?;
            let change = match kind {
                "A" => Change::Added,
                "C" => Change::Changed,
                _ => return None,
            };
            Some((change, path.trim().to_string()))
        })
        .collect()
}

/// Is `path` inside one of the excluded locations? An exclude is a path prefix or a glob.
pub fn excluded(path: &str, excludes: &[String]) -> bool {
    let defaults = DEFAULT_EXCLUDES.iter().map(|s| s.to_string());
    defaults.chain(excludes.iter().cloned()).any(|rule| {
        let rule = rule.trim_end_matches('/');
        if path == rule
            || path
                .strip_prefix(rule)
                .is_some_and(|rest| rest.starts_with('/'))
        {
            return true;
        }
        glob::Pattern::new(rule).is_ok_and(|pattern| pattern.matches(path))
    })
}

/// What one tracked build needs.
pub struct Tracking<'a> {
    /// The lane of the build in the progress output, for example `fedora/amd64`.
    pub label: &'a str,
    pub image: &'a str,
    pub build: &'a str,
    pub source: &'a Path,
    pub env: &'a BTreeMap<String, String>,
    pub arch: &'a str,
    pub excludes: &'a [String],
    pub stage: &'a Path,
    /// Keeps the container name unique among builds that run at the same time.
    pub id: &'a str,
}

/// What a tracked build produced.
pub struct Built {
    /// The folder the installed files were exported to.
    pub stage: PathBuf,
    pub entries: Vec<Entry>,
}

/// A script on disk the container can run: a file with a `#!` line runs as written, any other text runs with `sh -e`.
struct Script {
    _directory: tempfile::TempDir,
    path: PathBuf,
    shebang: bool,
}

impl Script {
    fn new(text: &str) -> Result<Script> {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("script");
        let mut file = std::fs::File::create(&path)?;
        file.write_all(text.as_bytes())?;
        #[cfg(unix)]
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))?;
        Ok(Script {
            _directory: directory,
            path,
            shebang: text.starts_with("#!"),
        })
    }

    fn entrypoint(&self) -> Vec<String> {
        if self.shebang {
            vec!["/pkgsmith-script".into()]
        } else {
            vec!["sh".into(), "-e".into(), "/pkgsmith-script".into()]
        }
    }
}

/// The first line of a script for a step title, with an ellipsis when the script goes on.
fn title(script: &str) -> String {
    let mut lines = script
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty());
    let first = lines.next().unwrap_or("");
    if lines.next().is_some() {
        format!("{first} …")
    } else {
        first.to_string()
    }
}

fn architecture(arch: &str) -> Option<&'static str> {
    match arch {
        "arm64" => Some("arm64"),
        "amd64" => Some("amd64"),
        _ => None,
    }
}

/// What to run in a container.
struct Run<'a> {
    name: String,
    image: &'a str,
    script: &'a Script,
    arch: &'a str,
    env: &'a BTreeMap<String, String>,
    /// Mount this folder at `/src` as a throw-away overlay and start there.
    source: Option<&'a Path>,
}

/// The connection to Podman. It talks to the Podman API socket. When no socket answers, it starts
/// `podman system service` for the length of the run.
pub struct Engine {
    podman: Podman,
    runtime: tokio::runtime::Runtime,
    service: Option<std::process::Child>,
    _socket_directory: Option<tempfile::TempDir>,
    /// One lock per setup image, so builds that need the same image wait for the first one instead of racing it.
    setups: std::sync::Mutex<BTreeMap<String, std::sync::Arc<std::sync::Mutex<()>>>>,
}

impl Drop for Engine {
    fn drop(&mut self) {
        if let Some(service) = &mut self.service {
            let _ = service.kill();
            let _ = service.wait();
        }
    }
}

impl Engine {
    fn new(
        podman: Podman,
        runtime: tokio::runtime::Runtime,
        service: Option<std::process::Child>,
        socket_directory: Option<tempfile::TempDir>,
    ) -> Engine {
        Engine {
            podman,
            runtime,
            service,
            _socket_directory: socket_directory,
            setups: Default::default(),
        }
    }

    /// Remove the cached setup images. Returns how many there were.
    pub fn prune(&self) -> Result<usize> {
        self.runtime.block_on(async {
            let images = self.podman.images();
            let list = images
                .list(&ImageListOpts::builder().all(true).build())
                .await?;
            let mut removed = 0;
            for image in list {
                let tagged = image
                    .repo_tags
                    .iter()
                    .flatten()
                    .any(|tag| tag.starts_with("localhost/pkgsmith-setup:"));
                if let (true, Some(id)) = (tagged, &image.id) {
                    self.podman.images().get(id).remove().await?;
                    removed += 1;
                }
            }
            Ok(removed)
        })
    }

    /// Connect to the container engine.
    ///
    /// 1. `CONTAINER_HOST`, the variable of Podman itself: `unix:///path/to/podman.sock` or `tcp://host:port`. This is the
    ///    way on Windows, and on any system where the socket is somewhere else.
    /// 2. On Linux, the socket of the user (`$XDG_RUNTIME_DIR/podman/podman.sock`), else `podman system service` for the
    ///    length of the run.
    /// 3. On macOS, the socket of the running `podman machine`.
    pub fn connect() -> Result<Engine> {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()?;
        if let Some(host) = std::env::var("CONTAINER_HOST")
            .ok()
            .filter(|h| !h.is_empty())
        {
            let podman = Podman::new(&host)
                .with_context(|| format!("CONTAINER_HOST={host} is not an address that pkgsmith understands: use unix:///PATH or tcp://HOST:PORT"))?;
            runtime
                .block_on(podman.ping())
                .with_context(|| format!("podman does not answer at CONTAINER_HOST={host}"))?;
            return Ok(Engine::new(podman, runtime, None, None));
        }
        #[cfg(unix)]
        return Self::connect_unix(runtime);
        #[cfg(not(unix))]
        {
            let _ = runtime;
            bail!(
                "pkgsmith needs the address of the Podman API to run container builds on this system. Start Podman (`podman machine start`) and set CONTAINER_HOST to a tcp:// address that it serves. Builds without an `image` do not need a container"
            )
        }
    }

    #[cfg(unix)]
    fn connect_unix(runtime: tokio::runtime::Runtime) -> Result<Engine> {
        let default = std::env::var("XDG_RUNTIME_DIR")
            .map(|dir| format!("{dir}/podman/podman.sock"))
            .ok();
        if let Some(path) = &default {
            let podman = Podman::unix(path);
            if runtime.block_on(podman.ping()).is_ok() {
                return Ok(Engine::new(podman, runtime, None, None));
            }
        }
        if cfg!(target_os = "macos") {
            // Podman runs in a virtual machine here. Its socket is on the Mac, and `podman machine inspect` names it.
            let socket = std::process::Command::new("podman")
                .args([
                    "machine",
                    "inspect",
                    "--format",
                    "{{.ConnectionInfo.PodmanSocket.Path}}",
                ])
                .output()
                .ok()
                .filter(|o| o.status.success())
                .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
                .filter(|path| !path.is_empty());
            if let Some(socket) = socket {
                let podman = Podman::unix(&socket);
                if runtime.block_on(podman.ping()).is_ok() {
                    return Ok(Engine::new(podman, runtime, None, None));
                }
            }
            bail!(
                "podman does not answer. On macOS Podman runs in a virtual machine: run `podman machine init` once and `podman machine start`, or set CONTAINER_HOST"
            );
        }
        // The socket lives in the runtime folder of the user, which no cleanup of temporary files touches.
        let base = std::env::var("XDG_RUNTIME_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|_| std::env::temp_dir());
        let directory = tempfile::Builder::new()
            .prefix("pkgsmith-")
            .tempdir_in(base)?;
        let socket = directory.path().join("podman.sock");
        let service = std::process::Command::new("podman")
            .args(["system", "service", "--time=0"])
            .arg(format!("unix://{}", socket.display()))
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .context("cannot start podman; is podman installed?")?;
        let podman = Podman::unix(&socket);
        for _ in 0..100 {
            if runtime.block_on(podman.ping()).is_ok() {
                return Ok(Engine::new(podman, runtime, Some(service), Some(directory)));
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        let mut service = service;
        let _ = service.kill();
        bail!("podman did not answer on its API socket within 10 seconds")
    }

    async fn pull_if_missing(&self, image: &str, step: &Step) -> Result<()> {
        if self.podman.images().get(image).exists().await? {
            return Ok(());
        }
        step.log(&format!("pulling {image}"));
        let options = PullOpts::builder().reference(image).build();
        let images = self.podman.images();
        let mut stream = images.pull(&options);
        while let Some(report) = stream.next().await {
            let report = report.with_context(|| format!("cannot pull {image}"))?;
            if let Some(line) = report.stream {
                step.log(line.trim_end());
            }
        }
        Ok(())
    }

    /// Create, start and follow a container. Its output goes to `step`. Fails when it exits with a status other than
    /// 0. The container stays, so the caller can inspect or remove it.
    async fn run(&self, run: &Run<'_>, step: &Step) -> Result<()> {
        self.pull_if_missing(run.image, step).await?;
        let mut options = ContainerCreateOpts::builder()
            .name(&run.name)
            .image(run.image)
            .entrypoint(run.script.entrypoint())
            .selinux_opts(["disable"])
            .env(run.env.iter().map(|(k, v)| (k.as_str(), v.as_str())))
            .mounts([ContainerMount {
                destination: Some("/pkgsmith-script".into()),
                source: Some(run.script.path.display().to_string()),
                _type: Some("bind".into()),
                options: Some(vec!["ro".into()]),
                uid_mappings: None,
                gid_mappings: None,
            }]);
        if let Some(arch) = architecture(run.arch) {
            options = options.image_arch(arch);
        }
        if let Some(source) = run.source {
            options = options.work_dir("/src").overlay_volumes([OverlayVolume {
                destination: Some("/src".into()),
                source: Some(source.display().to_string()),
                options: None,
            }]);
        }
        let container = self.podman.containers().create(&options.build()).await?;
        let container = self.podman.containers().get(container.id);
        container.start(None).await?;
        let follow = ContainerLogsOpts::builder()
            .stdout(true)
            .stderr(true)
            .follow(true)
            .build();
        let mut logs = container.logs(&follow);
        let print = async {
            let mut lines = LineSplitter::new(step);
            while let Some(chunk) = logs.next().await {
                match chunk? {
                    TtyChunk::StdOut(bytes) | TtyChunk::StdErr(bytes) | TtyChunk::StdIn(bytes) => {
                        lines.push(&bytes)
                    }
                }
            }
            lines.flush();
            anyhow::Ok(())
        };
        let wait_options = ContainerWaitOpts::builder()
            .conditions([ContainerStatus::Exited])
            .build();
        let wait = container.wait(&wait_options);
        let (printed, waited) = tokio::join!(print, wait);
        printed?;
        waited?;
        let code = container
            .inspect()
            .await?
            .state
            .and_then(|state| state.exit_code)
            .unwrap_or(-1);
        if code != 0 {
            bail!("the script exited with status {code}");
        }
        Ok(())
    }

    async fn remove(&self, name: &str) {
        let _ = self
            .podman
            .containers()
            .get(name)
            .delete(
                &ContainerDeleteOpts::builder()
                    .force(true)
                    .volumes(true)
                    .build(),
            )
            .await;
    }

    /// The image `build` runs on: the base image, or the base image after `setup`, cached under a tag that names its
    /// inputs. `cache` false rebuilds the setup image.
    pub fn prepare(
        &self,
        ui: &Ui,
        label: &str,
        image: &str,
        setup: Option<&str>,
        arch: &str,
        cache: bool,
    ) -> Result<String> {
        let Some(setup) = setup else {
            let step = ui.step(label, format!("setup  pull {image}"));
            let result = self.runtime.block_on(self.pull_if_missing(image, &step));
            return match result {
                Ok(()) => {
                    step.done();
                    Ok(image.to_string())
                }
                Err(error) => {
                    step.fail();
                    Err(error)
                }
            };
        };
        let key = hex(&Sha256::digest(format!("{image}\0{arch}\0{setup}")))[..16].to_string();
        let tag = format!("localhost/pkgsmith-setup:{key}");
        let lock = self
            .setups
            .lock()
            .unwrap()
            .entry(key.clone())
            .or_default()
            .clone();
        let _held = lock.lock().unwrap();
        let first = title(setup);
        let name = format!("setup  {first}");
        if cache
            && self
                .runtime
                .block_on(async { anyhow::Ok(self.podman.images().get(&tag).exists().await?) })?
        {
            ui.step(label, name).cached();
            return Ok(tag);
        }
        let step = ui.step(label, name);
        let result: Result<()> = self.runtime.block_on(async {
            let container = format!("pkgsmith-setup-{key}");
            self.remove(&container).await;
            let script = Script::new(setup)?;
            let env = BTreeMap::new();
            let ran = self
                .run(
                    &Run {
                        name: container.clone(),
                        image,
                        script: &script,
                        arch,
                        env: &env,
                        source: None,
                    },
                    &step,
                )
                .await;
            if ran.is_ok() {
                let commit = ContainerCommitOpts::builder()
                    .repo("localhost/pkgsmith-setup")
                    .tag(&key)
                    .build();
                self.podman
                    .containers()
                    .get(&container)
                    .commit(&commit)
                    .await?;
            }
            self.remove(&container).await;
            ran.context("the setup script failed")
        });
        match result {
            Ok(()) => {
                step.done();
                Ok(tag)
            }
            Err(error) => {
                step.fail();
                Err(error)
            }
        }
    }

    /// Run `build` on `image` with `source` at `/src`, then export every added or changed file outside the excluded
    /// locations into `stage`. `id` keeps the container name unique among builds that run at the same time.
    pub fn track(&self, ui: &Ui, request: &Tracking) -> Result<Built> {
        let Tracking {
            label,
            image,
            build,
            source,
            env,
            arch,
            excludes,
            stage,
            id,
        } = *request;
        let first = title(build);
        let step = ui.step(label, format!("build  {first}"));
        let name = format!("pkgsmith-build-{}-{id}", std::process::id());
        let ran: Result<()> = self.runtime.block_on(async {
            self.remove(&name).await;
            let script = Script::new(build)?;
            self.run(
                &Run {
                    name: name.clone(),
                    image,
                    script: &script,
                    arch,
                    env,
                    source: Some(source),
                },
                &step,
            )
            .await
            .context("the build script failed")
        });
        if let Err(error) = ran {
            self.runtime.block_on(self.remove(&name));
            step.fail();
            return Err(error);
        }
        step.done();
        let collect = ui.step(label, "collect the installed files");
        let outcome = self.runtime.block_on(self.export(&name, excludes, stage));
        self.runtime.block_on(self.remove(&name));
        match outcome {
            Ok(entries) => {
                collect.log(&format!("{} files and folders", entries.len()));
                collect.done();
                Ok(Built {
                    stage: stage.to_path_buf(),
                    entries,
                })
            }
            Err(error) => {
                collect.fail();
                Err(error)
            }
        }
    }

    async fn export(&self, name: &str, excludes: &[String], stage: &Path) -> Result<Vec<Entry>> {
        let container = self.podman.containers().get(name);
        let changes = container
            .changes(&Default::default())
            .await
            .context("cannot list the installed files")?;
        let diff: String = changes
            .iter()
            .map(|item| {
                format!(
                    "{} {}\n",
                    match item.kind {
                        0 => "C",
                        1 => "A",
                        _ => "D",
                    },
                    item.path
                )
            })
            .collect();
        let wanted: BTreeMap<String, Change> = parse_diff(&diff)
            .into_iter()
            .filter(|(_, path)| !excluded(path, excludes))
            .map(|(change, path)| (path, change))
            .collect();
        if wanted.is_empty() {
            bail!(
                "the build installed no files outside /src; install into the image, for example `make install PREFIX=/usr`"
            );
        }
        if stage.exists() {
            std::fs::remove_dir_all(stage)?;
        }
        std::fs::create_dir_all(stage)?;

        // The tar of the whole container streams in; a thread keeps only the wanted members.
        let (sender, receiver) = std::sync::mpsc::sync_channel::<Vec<u8>>(16);
        let stage_path = stage.to_path_buf();
        let reader =
            std::thread::spawn(move || unpack(ChannelReader::new(receiver), &wanted, &stage_path));
        let mut stream = container.export();
        while let Some(chunk) = stream.next().await {
            if sender.send(chunk?).is_err() {
                break;
            }
        }
        drop(sender);
        reader
            .join()
            .map_err(|_| anyhow::anyhow!("the export reader panicked"))?
    }
}

/// A `Read` over chunks that arrive on a channel.
struct ChannelReader {
    receiver: std::sync::mpsc::Receiver<Vec<u8>>,
    current: std::io::Cursor<Vec<u8>>,
}

impl ChannelReader {
    fn new(receiver: std::sync::mpsc::Receiver<Vec<u8>>) -> ChannelReader {
        ChannelReader {
            receiver,
            current: std::io::Cursor::new(Vec::new()),
        }
    }
}

impl std::io::Read for ChannelReader {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        loop {
            let read = self.current.read(buffer)?;
            if read > 0 {
                return Ok(read);
            }
            match self.receiver.recv() {
                Ok(chunk) => self.current = std::io::Cursor::new(chunk),
                Err(_) => return Ok(0),
            }
        }
    }
}

/// Keep the members of the container tar that the diff named, in `stage`, and describe them as package entries.
fn unpack(
    source: impl std::io::Read,
    wanted: &BTreeMap<String, Change>,
    stage: &Path,
) -> Result<Vec<Entry>> {
    let mut archive = tar::Archive::new(source);
    archive.set_preserve_permissions(true);
    archive.set_unpack_xattrs(false);
    let mut entries = Vec::new();
    for entry in archive.entries()? {
        let mut entry = entry?;
        let raw = entry.path()?.to_string_lossy().into_owned();
        let path = format!("/{}", raw.trim_start_matches("./").trim_matches('/'));
        let Some(change) = wanted.get(&path) else {
            continue;
        };
        let kind = entry.header().entry_type();
        if kind.is_dir() && *change == Change::Changed {
            continue; // A folder that already existed. The package does not own it.
        }
        entry.unpack_in(stage)?;
        let mode = entry.header().mode()? & 0o7777;
        let data = if kind.is_dir() {
            Data::Dir
        } else if kind.is_symlink() {
            Data::Symlink {
                target: entry
                    .link_name()?
                    .map(|t| t.to_string_lossy().into_owned())
                    .unwrap_or_default(),
            }
        } else {
            let on_disk = stage.join(path.trim_start_matches('/'));
            Data::File {
                src: on_disk.clone(),
                size: std::fs::metadata(&on_disk)?.len(),
            }
        };
        entries.push(Entry {
            config: path.starts_with("/etc/") && matches!(data, Data::File { .. }),
            path,
            data,
            mode,
            owner: "root".into(),
            group: "root".into(),
        });
    }
    entries.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(entries)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_diff_keeps_added_and_changed_paths_and_drops_deletions() {
        let diff = "C /usr\nA /usr/bin/tool\nD /usr/bin/old\nC /etc/ld.so.cache\nA /opt/app\n";
        assert_eq!(
            parse_diff(diff),
            [
                (Change::Changed, "/usr".to_string()),
                (Change::Added, "/usr/bin/tool".to_string()),
                (Change::Changed, "/etc/ld.so.cache".to_string()),
                (Change::Added, "/opt/app".to_string()),
            ]
        );
    }

    #[test]
    fn default_and_user_excludes_match_prefixes_and_globs() {
        let user = vec![
            "/usr/share/doc/**".to_string(),
            "/usr/lib/debug".to_string(),
        ];
        assert!(excluded("/tmp/build/x", &user));
        assert!(excluded("/var/cache/dnf/repo", &user));
        assert!(excluded("/etc/ld.so.cache", &user));
        assert!(excluded("/usr/share/doc/pkg/README", &user));
        assert!(excluded("/usr/lib/debug/x.debug", &user));
        assert!(!excluded("/usr/bin/tool", &user));
        assert!(!excluded("/usr/lib/gnoblin/lib64/libmutter.so", &user));
        assert!(!excluded("/tmpfile-is-not-tmp", &user));
    }

    #[test]
    fn a_script_with_a_shebang_runs_as_written_and_other_text_runs_with_sh() {
        assert_eq!(
            Script::new("make install").unwrap().entrypoint(),
            ["sh", "-e", "/pkgsmith-script"]
        );
        assert_eq!(
            Script::new("#!/bin/bash\nset -e\n").unwrap().entrypoint(),
            ["/pkgsmith-script"]
        );
    }
}
