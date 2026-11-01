//! Run `pkgsmith.star`. A script is a program that describes packages. pkgsmith runs it from top to bottom, and every
//! `package(...)` call that the run reaches declares one package to write.
//!
//! pkgsmith builds nothing. The files come from a prefix that already exists: a folder laid out like an installed system
//! (`usr/bin/hello`, `etc/hello.conf`), for example the result of `make install DESTDIR=prefix`. The script may read it,
//! with `read_file`, `exists`, `is_dir`, `listdir` and `glob`, but it cannot change anything, and it cannot read outside the
//! prefix and its own folder. It has no network or processes either.
//!
//! * `package("rpm", name = ..., version = ...)` writes a package of one format. It holds the whole prefix, narrowed by
//!   `include()` and `exclude()`, or only the files that `expose()` offered.
//!
//! Every `package(...)` call that the run reaches writes one package, so `package("rpm")` and `package("deb")` make two
//! packages from the same files, and the branch of an `if` that is false on this system makes none. `host()` tells the
//! script which system it runs on. The language is Starlark, in the implementation of Meta (`starlark-rust`).

use crate::host::Host;
use crate::manifest::{Manifest, TargetSpec};
use anyhow::{Context, Result, anyhow, bail};
use serde_json::{Value as Json, json};
use starlark::any::ProvidesStaticType;
use starlark::collections::SmallMap;
use starlark::environment::{FrozenModule, Globals, GlobalsBuilder, LibraryExtension, Module};
use starlark::eval::{Evaluator, FileLoader};
use starlark::starlark_module;
use starlark::syntax::{AstModule, Dialect};
use starlark::values::Value;
use starlark::values::none::NoneType;
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

const PRELUDE: &str = include_str!("prelude.star");

/// What the script reads, and what it declares.
#[derive(Debug, ProvidesStaticType)]
struct Collector {
    /// The system the script runs on.
    host: Host,
    /// `--build-arg NAME=VALUE`. They win over the environment.
    args: BTreeMap<String, String>,
    /// The prefix: the read-only folder that holds the files to package.
    prefix: PathBuf,
    /// The folder of the script.
    here: PathBuf,
    /// The files that the script has exposed so far. Empty means the whole prefix.
    exposed: RefCell<Vec<Json>>,
    /// Path globs that `include()` and `exclude()` have collected so far. They apply to the packages that come after.
    include: RefCell<Vec<String>>,
    exclude: RefCell<Vec<String>>,
    /// The packages, in the order the script declared them.
    packages: RefCell<Vec<TargetSpec>>,
}

/// The globs of `include(...)` and `exclude(...)`: text, or lists of text.
fn patterns_of(args: &starlark::values::tuple::TupleRef) -> anyhow::Result<Vec<String>> {
    let mut out = Vec::new();
    for arg in args.iter() {
        match arg.to_json_value()? {
            Json::String(text) => out.push(text),
            Json::Array(list) => {
                for item in list {
                    match item {
                        Json::String(text) => out.push(text),
                        _ => bail!("a pattern must be text"),
                    }
                }
            }
            _ => bail!("a pattern must be text or a list of text"),
        }
    }
    Ok(out)
}

fn collector<'x>(eval: &'x Evaluator<'_, '_, '_>) -> anyhow::Result<&'x Collector> {
    eval.extra
        .and_then(|extra| extra.downcast_ref::<Collector>())
        .ok_or_else(|| anyhow!("this function is not available here"))
}

fn json_value<'v>(heap: starlark::values::Heap<'v>, value: &Json) -> Value<'v> {
    match value {
        Json::String(text) => heap.alloc_str(text).to_value(),
        Json::Array(items) => heap.alloc(
            items
                .iter()
                .map(|item| json_value(heap, item))
                .collect::<Vec<_>>(),
        ),
        other => heap.alloc_str(&other.to_string()).to_value(),
    }
}

/// A path that a script asks to read. A relative path is inside the prefix. The path must resolve, symbolic links
/// followed, to a place inside the prefix or the folder of the script.
fn readable(collector: &Collector, text: &str) -> anyhow::Result<PathBuf> {
    let path = Path::new(text);
    let native = path.is_absolute()
        && (path.starts_with(&collector.prefix) || path.starts_with(&collector.here));
    let path = if native {
        path.to_path_buf()
    } else if path.is_absolute() || text.starts_with('/') {
        // `/usr/bin/x` names a file of the prefix, as it does in a package. On Windows a path with no drive is not
        // absolute, so the text decides.
        collector.prefix.join(text.trim_start_matches(['/', '\\']))
    } else {
        collector.prefix.join(path)
    };
    let resolved =
        std::fs::canonicalize(&path).with_context(|| format!("cannot read {}", path.display()))?;
    let inside =
        |root: &Path| std::fs::canonicalize(root).is_ok_and(|root| resolved.starts_with(root));
    if inside(&collector.prefix) || inside(&collector.here) {
        Ok(resolved)
    } else {
        bail!("{text} is outside the prefix and the folder of the script")
    }
}

#[starlark_module]
fn builtins(builder: &mut GlobalsBuilder) {
    /// The build argument `name` (`--build-arg NAME=VALUE`), else the environment variable of that name, else `default`.
    fn arg<'v>(
        name: &str,
        default: Option<Value<'v>>,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<Value<'v>> {
        let collector = collector(eval)?;
        if let Some(value) = collector.args.get(name) {
            return Ok(eval.heap().alloc_str(value).to_value());
        }
        if let Ok(value) = std::env::var(name) {
            return Ok(eval.heap().alloc_str(&value).to_value());
        }
        Ok(default.unwrap_or_else(Value::new_none))
    }

    /// The system this script runs on: a dict with os, arch, distro, like, version, libc and format.
    fn host<'v>(eval: &mut Evaluator<'v, '_, '_>) -> anyhow::Result<Value<'v>> {
        let host = serde_json::to_value(&collector(eval)?.host)?;
        Ok(eval.heap().alloc(starlark::values::dict::AllocDict(
            host.as_object()
                .into_iter()
                .flatten()
                .map(|(key, value)| (key.clone(), json_value(eval.heap(), value)))
                .collect::<Vec<_>>(),
        )))
    }

    /// The prefix as a path: the folder that holds the files to package.
    fn prefix<'v>(eval: &mut Evaluator<'v, '_, '_>) -> anyhow::Result<Value<'v>> {
        let text = collector(eval)?.prefix.to_string_lossy().into_owned();
        Ok(eval.heap().alloc_str(&text).to_value())
    }

    /// The folder of the script as a path. Files next to the script, such as a license, are read from there.
    fn here<'v>(eval: &mut Evaluator<'v, '_, '_>) -> anyhow::Result<Value<'v>> {
        let text = collector(eval)?.here.to_string_lossy().into_owned();
        Ok(eval.heap().alloc_str(&text).to_value())
    }

    /// The text of a file of the prefix (a relative or `/`-rooted path) or of the folder of the script.
    fn read_file<'v>(path: &str, eval: &mut Evaluator<'v, '_, '_>) -> anyhow::Result<Value<'v>> {
        let file = readable(collector(eval)?, path)?;
        let text = std::fs::read_to_string(&file)
            .with_context(|| format!("cannot read {}", file.display()))?;
        Ok(eval.heap().alloc_str(&text).to_value())
    }

    /// Whether a path exists in the prefix or the folder of the script. A path outside both does not exist.
    fn exists(path: &str, eval: &mut Evaluator<'_, '_, '_>) -> anyhow::Result<bool> {
        Ok(readable(collector(eval)?, path).is_ok())
    }

    /// Whether a path in the prefix or the folder of the script is a folder.
    fn is_dir(path: &str, eval: &mut Evaluator<'_, '_, '_>) -> anyhow::Result<bool> {
        Ok(readable(collector(eval)?, path).is_ok_and(|path| path.is_dir()))
    }

    /// The sorted names in a folder of the prefix or of the script.
    fn listdir<'v>(path: &str, eval: &mut Evaluator<'v, '_, '_>) -> anyhow::Result<Value<'v>> {
        let folder = readable(collector(eval)?, path)?;
        let mut names: Vec<String> = std::fs::read_dir(&folder)
            .with_context(|| format!("cannot list {}", folder.display()))?
            .map(|e| e.map(|e| e.file_name().to_string_lossy().into_owned()))
            .collect::<Result<_, _>>()?;
        names.sort();
        let heap = eval.heap();
        Ok(heap.alloc(
            names
                .iter()
                .map(|n| heap.alloc_str(n).to_value())
                .collect::<Vec<_>>(),
        ))
    }

    /// The paths that match a pattern in the prefix, sorted, as paths from the root of the prefix: `glob("usr/bin/*")`
    /// gives `["/usr/bin/a", "/usr/bin/b"]`.
    fn glob<'v>(pattern: &str, eval: &mut Evaluator<'v, '_, '_>) -> anyhow::Result<Value<'v>> {
        let root = std::fs::canonicalize(&collector(eval)?.prefix)?;
        let full = root.join(pattern.trim_start_matches('/'));
        let mut found: Vec<String> = ::glob::glob(&full.to_string_lossy())
            .with_context(|| format!("bad pattern {pattern:?}"))?
            .filter_map(Result::ok)
            .filter_map(|path| {
                let resolved = std::fs::canonicalize(path.parent()?)
                    .ok()?
                    .join(path.file_name()?);
                let relative = resolved.strip_prefix(&root).ok()?;
                Some(format!("/{}", relative.to_string_lossy()))
            })
            .collect();
        found.sort();
        let heap = eval.heap();
        Ok(heap.alloc(
            found
                .iter()
                .map(|n| heap.alloc_str(n).to_value())
                .collect::<Vec<_>>(),
        ))
    }

    /// Make files of the prefix part of what the script offers. Takes content entries (`file`, `config`,
    /// `tree`, `symlink`, `directory`, `ghost`) or lists of them.
    fn expose<'v>(
        #[starlark(args)] entries: &'v starlark::values::tuple::TupleRef<'v>,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<NoneType> {
        let mut exposed = Vec::new();
        for entry in entries.iter() {
            match entry.to_json_value().context("expose")? {
                Json::Array(list) => exposed.extend(list),
                one => exposed.push(one),
            }
        }
        collector(eval)?.exposed.borrow_mut().extend(exposed);
        Ok(NoneType)
    }

    /// Keep only the paths of the package that match these globs (`/usr/bin/**`, `/etc/hello.conf`), for the packages
    /// declared after this call. Without any `include`, a package keeps everything in the prefix.
    fn include<'v>(
        #[starlark(args)] patterns: &'v starlark::values::tuple::TupleRef<'v>,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<NoneType> {
        let list = patterns_of(patterns)?;
        collector(eval)?.include.borrow_mut().extend(list);
        Ok(NoneType)
    }

    /// Leave out the paths of the package that match these globs, for the packages declared after this call.
    fn exclude<'v>(
        #[starlark(args)] patterns: &'v starlark::values::tuple::TupleRef<'v>,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<NoneType> {
        let list = patterns_of(patterns)?;
        collector(eval)?.exclude.borrow_mut().extend(list);
        Ok(NoneType)
    }

    /// Forget the files exposed, and the `include` and `exclude` globs collected, so far. The next package starts from the
    /// whole prefix again. Use it between split packages.
    fn reset(eval: &mut Evaluator<'_, '_, '_>) -> anyhow::Result<NoneType> {
        let c = collector(eval)?;
        c.exposed.borrow_mut().clear();
        c.include.borrow_mut().clear();
        c.exclude.borrow_mut().clear();
        Ok(NoneType)
    }

    /// Write one package of type `format` (`rpm`, `deb`, see `pkgsmith formats`) from the files exposed so far. The fields
    /// are name, version and description (required), then release, epoch, arch, maintainer, homepage, license, depends,
    /// recommends, suggests, conflicts, replaces, provides, contents (more files, added to the exposed ones or to the whole prefix), options, distro,
    /// file, tags, include, exclude and config_files (path globs of the package; `include()` and `exclude()` calls add to them) and scripts (a dict of pre_install,
    /// post_install, pre_remove and post_remove: text, or the name of a file next to the script).
    fn package<'v>(
        format: &str,
        #[starlark(kwargs)] fields: SmallMap<String, Value<'v>>,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<NoneType> {
        // The files the script exposed, or the whole prefix when it exposed none. `contents` adds to them.
        let mut contents = collector(eval)?.exposed.borrow().clone();
        if contents.is_empty() {
            contents.push(json!({"type": "tree", "src": "/", "dst": "/"}));
        }
        let mut spec = serde_json::Map::new();
        spec.insert(
            "include".into(),
            json!(collector(eval)?.include.borrow().clone()),
        );
        spec.insert(
            "exclude".into(),
            json!(collector(eval)?.exclude.borrow().clone()),
        );
        for (key, value) in fields {
            let value = value
                .to_json_value()
                .with_context(|| format!("package({format:?})"))?;
            if key == "contents" {
                match value {
                    Json::Array(list) => contents.extend(list),
                    _ => bail!("package({format:?}): contents must be a list"),
                }
            } else if (key == "include" || key == "exclude") && spec.contains_key(&key) {
                // The globs of the package add to the ones that include() and exclude() collected.
                let (Some(Json::Array(list)), Json::Array(more)) = (spec.get_mut(&key), value)
                else {
                    bail!("package({format:?}): {key} must be a list")
                };
                list.extend(more);
            } else {
                spec.insert(key, value);
            }
        }
        spec.insert("format".into(), json!(format));
        spec.insert("contents".into(), Json::Array(contents));
        let arch = match spec.get("arch") {
            Some(Json::String(arch)) => arch.clone(),
            Some(_) => bail!("package({format:?}): arch must be text"),
            None => collector(eval)?.host.arch.clone(),
        };
        spec.insert("arch".into(), json!(arch));
        let package = Manifest::parse_spec(Json::Object(spec))
            .with_context(|| format!("package({format:?})"))?;
        collector(eval)?.packages.borrow_mut().push(package);
        Ok(NoneType)
    }
}

fn dialect() -> Dialect {
    Dialect {
        enable_f_strings: true,
        ..Dialect::Extended
    }
}

fn globals() -> Globals {
    // `json` lets a script read and build data. Nothing here reaches the machine.
    GlobalsBuilder::extended_by(&[LibraryExtension::Json])
        .with(builtins)
        .build()
}

/// Loads the files a script names with `load("file.star", "name")`. A path is relative to the file that names it.
struct Loader<'a> {
    dir: PathBuf,
    prelude: &'a FrozenModule,
    globals: &'a Globals,
    collector: &'a Collector,
}

impl Loader<'_> {
    fn evaluate(&self, file: &Path) -> Result<FrozenModule> {
        let source = std::fs::read_to_string(file)
            .with_context(|| format!("cannot read {}", file.display()))?;
        let ast = AstModule::parse(&file.to_string_lossy(), source, &dialect())
            .map_err(|error| anyhow!("{error:#}"))?;
        let nested = Loader {
            dir: file.parent().map(Path::to_path_buf).unwrap_or_default(),
            prelude: self.prelude,
            globals: self.globals,
            collector: self.collector,
        };
        Module::with_temp_heap(|module| {
            module.import_public_symbols(self.prelude);
            {
                let mut eval = Evaluator::new(&module);
                eval.set_loader(&nested);
                eval.extra = Some(self.collector);
                eval.eval_module(ast, self.globals)
                    .map_err(|error| anyhow!("{error:#}"))?;
            }
            module.freeze().map_err(|error| anyhow!("{error:?}"))
        })
    }
}

impl FileLoader for Loader<'_> {
    fn load(&self, path: &str) -> starlark::Result<FrozenModule> {
        let file = self.dir.join(path);
        self.evaluate(&file)
            .map_err(|error| starlark::Error::new_other(anyhow!("{error:#}")))
    }
}

/// Run the script and give back the packages it declares. `args` are the `--build-arg`s.
pub fn evaluate(
    file: &Path,
    args: BTreeMap<String, String>,
    prefix: &Path,
) -> Result<Vec<TargetSpec>> {
    evaluate_on(file, args, Host::detect(), prefix)
}

/// Like `evaluate`, for a given system. Tests use it so that they do not depend on the machine.
pub fn evaluate_on(
    file: &Path,
    args: BTreeMap<String, String>,
    host: Host,
    prefix: &Path,
) -> Result<Vec<TargetSpec>> {
    let globals = globals();
    let folder = file
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let here = std::fs::canonicalize(folder)
        .with_context(|| format!("cannot read the folder of the script, {}", folder.display()))?;
    let collector = Collector {
        host,
        args,
        prefix: prefix.to_path_buf(),
        here,
        exposed: RefCell::new(Vec::new()),
        include: RefCell::new(Vec::new()),
        exclude: RefCell::new(Vec::new()),
        packages: RefCell::new(Vec::new()),
    };
    // The prelude is a module of its own, so the script sees its functions without a `load`.
    let prelude = {
        let ast = AstModule::parse("prelude.star", PRELUDE.to_string(), &dialect())
            .map_err(|error| anyhow!("{error:#}"))?;
        Module::with_temp_heap(|module| {
            {
                let mut eval = Evaluator::new(&module);
                eval.eval_module(ast, &globals)
                    .map_err(|error| anyhow!("{error:#}"))?;
            }
            module.freeze().map_err(|error| anyhow!("{error:?}"))
        })?
    };
    let loader = Loader {
        dir: PathBuf::new(),
        prelude: &prelude,
        globals: &globals,
        collector: &collector,
    };
    loader
        .evaluate(file)
        .with_context(|| format!("{} failed", file.display()))?;
    let packages = collector.packages.into_inner();
    if packages.is_empty() {
        bail!(
            "{} declares no package. Call package(\"deb\", name = ..., version = ..., description = ...)",
            file.display()
        );
    }
    Ok(packages)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fedora() -> Host {
        Host::from_os_release("ID=fedora\nVERSION_ID=43\n", "amd64", "glibc")
    }

    /// A prefix with a program, a config file and a manual, and a script next to it.
    fn project(script: &str) -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        for file in [
            "prefix/usr/bin/hi",
            "prefix/etc/hi.conf",
            "prefix/usr/share/doc/hi/README",
        ] {
            let path = dir.path().join(file);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, "x").unwrap();
        }
        std::fs::write(dir.path().join("pkgsmith.star"), script).unwrap();
        (dir, PathBuf::new())
    }

    fn run(source: &str, args: &[(&str, &str)]) -> Result<Vec<TargetSpec>> {
        let (dir, _) = project(source);
        evaluate_on(
            &dir.path().join("pkgsmith.star"),
            args.iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
            fedora(),
            &dir.path().join("prefix"),
        )
    }

    const COMMON: &str =
        "common = {'name': 'hi', 'version': arg('VERSION', '1.0.0'), 'description': 'Says hi'}\n";

    #[test]
    fn package_calls_share_what_was_exposed_before_them() {
        let specs = run(
            &format!(
                "{COMMON}expose(file('usr/bin/hi', '/usr/bin/hi'))\npackage('deb', depends = ['libc6'], **common)\nexpose(config('etc/hi.conf', '/etc/hi.conf'))\npackage('rpm', distro = 'rocky', scripts = {{'post_install': 'echo hi'}}, **common)\n"
            ),
            &[],
        )
        .unwrap();
        assert_eq!(specs.len(), 2);
        assert_eq!(
            (specs[0].name.as_str(), specs[0].version.as_str()),
            ("hi", "1.0.0")
        );
        assert_eq!(
            (specs[0].depends.clone(), specs[0].distro.clone()),
            (vec!["libc6".to_string()], None)
        );
        assert_eq!((specs[0].contents.len(), specs[1].contents.len()), (1, 2));
        assert_eq!(specs[1].distro.as_deref(), Some("rocky"));
        assert_eq!(specs[1].scripts.post_install.as_deref(), Some("echo hi"));
    }

    #[test]
    fn a_package_holds_the_whole_prefix_unless_the_script_narrows_it() {
        let specs = run(
            &format!(
                "{COMMON}package('tar', **common)\nexclude('/usr/share/doc/**')\nconfig_files = ['/etc/**']\npackage('deb', config_files = config_files, **common)\nreset()\ninclude('/usr/share/doc/**')\npackage('zip', **common)\n"
            ),
            &[],
        )
        .unwrap();
        assert_eq!(
            (
                specs[0].contents.len(),
                specs[0].exclude.len(),
                specs[0].include.len()
            ),
            (1, 0, 0),
            "the whole prefix"
        );
        assert_eq!(specs[1].exclude, ["/usr/share/doc/**"]);
        assert_eq!(specs[1].config_files, ["/etc/**"]);
        assert_eq!(
            (specs[2].exclude.len(), specs[2].include.clone()),
            (0, vec!["/usr/share/doc/**".to_string()])
        );
    }

    #[test]
    fn only_the_code_that_runs_declares_packages_and_the_script_can_adapt_to_the_host() {
        let specs = run(
            &format!(
                "{COMMON}h = host()\nexpose(tree('/', '/'))\nif h['distro'] == 'fedora':\n    package('rpm', **common)\nelif h['distro'] == 'debian':\n    package('deb', **common)\nfor f in ['tar', 'zip']:\n    package(f, **common)\n"
            ),
            &[],
        )
        .unwrap();
        let formats: Vec<_> = specs.iter().map(|s| s.format.as_str()).collect();
        assert_eq!(formats, ["rpm", "tar", "zip"]);
    }

    #[test]
    fn a_script_reads_the_prefix_and_nothing_else() {
        let specs = run(
            &format!(
                "{COMMON}text = read_file('etc/hi.conf')\nnames = listdir('/usr/bin')\nbins = glob('usr/bin/*')\nassert_eq = [exists('usr/bin/hi'), exists('usr/bin/nope'), is_dir('usr'), exists('/etc/passwd'), exists('../outside')]\npackage('tar', description = 'x' + text + ','.join(names + bins) + str(assert_eq), name = 'hi', version = '1')\n"
            ),
            &[],
        )
        .unwrap();
        assert_eq!(
            specs[0].description,
            "xxhi,/usr/bin/hi[True, False, True, False, False]"
        );
        let error = run(
            &format!("{COMMON}read_file('../..')\npackage('tar', **common)\n"),
            &[],
        )
        .unwrap_err();
        assert!(
            format!("{error:#}").contains("outside the prefix"),
            "{error:#}"
        );
        let error = run(
            &format!("{COMMON}read_file('/etc/passwd')\npackage('tar', **common)\n"),
            &[],
        )
        .unwrap_err();
        assert!(format!("{error:#}").contains("cannot read"), "{error:#}");
    }

    #[test]
    fn build_arguments_win_and_mistakes_name_the_problem() {
        let specs = run(
            &format!("{COMMON}package('tar', **common)\n"),
            &[("VERSION", "2.0.0")],
        )
        .unwrap();
        assert_eq!(specs[0].version, "2.0.0");
        let error = run(
            &format!("{COMMON}package('tar', dependz = [], **common)\n"),
            &[],
        )
        .unwrap_err();
        assert!(format!("{error:#}").contains("dependz"), "{error:#}");
        let error = run("x = 1\n", &[]).unwrap_err();
        assert!(
            format!("{error:#}").contains("declares no package"),
            "{error:#}"
        );
        let error = run(&format!("{COMMON}sh('make')\n"), &[]).unwrap_err();
        assert!(format!("{error:#}").contains("sh"), "{error:#}");
    }

    #[test]
    fn a_script_loads_shared_code_next_to_it() {
        let (dir, _) = project(
            "load('common.star', 'DEPENDS')\npackage('tar', name = 'shared', version = '1', description = 'd', depends = DEPENDS)\n",
        );
        std::fs::write(dir.path().join("common.star"), "DEPENDS = ['a']\n").unwrap();
        let specs = evaluate_on(
            &dir.path().join("pkgsmith.star"),
            BTreeMap::new(),
            fedora(),
            &dir.path().join("prefix"),
        )
        .unwrap();
        assert_eq!(specs[0].depends, ["a"]);
    }
}
