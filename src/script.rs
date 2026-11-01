//! Run `pkgsmith.star`. A script is a program: pkgsmith runs it from top to bottom.
//!
//! * `sh("make")` adds a command to the build. The commands run on this system, in the folder of the script, like makepkg.
//! * `expose(file("target/release/hello", "/usr/bin/hello"))` makes a file of the build part of what the script offers.
//! * `package("rpm", name = ..., version = ...)` starts a job that packs the commands and files so far into an rpm.
//!
//! Every `package(...)` call that the run reaches writes one package, so `package("rpm")` and `package("deb")` make two
//! packages from one build, and the branch of an `if` that is false on this system makes none. `host()` tells the script
//! which system it runs on. The language is Starlark, in the implementation of Meta (`starlark-rust`), so a script has no
//! files, network or processes of its own.

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
    /// The commands of the build so far: `{run, name, env}`.
    steps: RefCell<Vec<Json>>,
    /// The files that the script has exposed so far.
    exposed: RefCell<Vec<Json>>,
    /// The packages, in the order the script declared them.
    packages: RefCell<Vec<TargetSpec>>,
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

fn is_function(value: Value) -> bool {
    matches!(value.get_type(), "function" | "native_function")
}

/// The commands a function records when pkgsmith calls it. They do not join the build.
fn record<'v>(function: Value<'v>, eval: &mut Evaluator<'v, '_, '_>) -> anyhow::Result<Vec<Json>> {
    let build = collector(eval)?.steps.take();
    let result = eval.eval_function(function, &[], &[]);
    let recorded = collector(eval)?.steps.replace(build);
    result.map_err(|error| anyhow!("{error:#}"))?;
    Ok(recorded)
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

    /// Add a command to the build. It runs in a fresh `sh -e` in the folder of the script, after the commands before it.
    fn sh<'v>(
        run: &str,
        name: Option<&str>,
        env: Option<Value<'v>>,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<NoneType> {
        let env = match env {
            Some(env) => env.to_json_value()?,
            None => json!({}),
        };
        collector(eval)?
            .steps
            .borrow_mut()
            .push(json!({"run": run, "name": name, "env": env}));
        Ok(NoneType)
    }

    /// Make the files that the build produces part of what the script offers. Takes content entries (`file`, `config`,
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

    /// Pack the commands and the exposed files so far into one package of type `format` (`rpm`, `deb`, see `pkgsmith
    /// formats`). The fields are name, version and description (required), then release, epoch, arch, maintainer,
    /// homepage, license, depends, recommends, suggests, conflicts, replaces, provides, contents (more files than the
    /// exposed ones), options, distro, file, tags, exclude (paths the build installs that the package leaves out) and
    /// scripts (a dict of pre_install, post_install, pre_remove and post_remove: text, or a function that calls sh()).
    fn package<'v>(
        format: &str,
        #[starlark(kwargs)] fields: SmallMap<String, Value<'v>>,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<NoneType> {
        let (host, steps, exposed) = {
            let c = collector(eval)?;
            (
                c.host.clone(),
                c.steps.borrow().clone(),
                c.exposed.borrow().clone(),
            )
        };
        let mut spec = serde_json::Map::new();
        let mut exclude = json!([]);
        let mut scripts = Vec::new();
        let mut contents = exposed;
        for (key, value) in fields {
            match key.as_str() {
                "exclude" => exclude = value.to_json_value()?,
                "contents" => match value
                    .to_json_value()
                    .with_context(|| format!("package({format:?})"))?
                {
                    Json::Array(list) => contents.extend(list),
                    _ => bail!("package({format:?}): contents must be a list"),
                },
                "scripts" => {
                    let Some(dict) = starlark::values::dict::DictRef::from_value(value) else {
                        bail!("package({format:?}): scripts must be a dict")
                    };
                    for (name, script) in dict.iter() {
                        let Some(name) = name.unpack_str() else {
                            bail!("package({format:?}): the keys of scripts are text")
                        };
                        scripts.push((name.to_string(), script));
                    }
                }
                _ => {
                    spec.insert(
                        key,
                        value
                            .to_json_value()
                            .with_context(|| format!("package({format:?})"))?,
                    );
                }
            }
        }
        spec.insert("format".into(), json!(format));
        spec.insert("contents".into(), Json::Array(contents));
        let arch = match spec.get("arch") {
            Some(Json::String(arch)) => arch.clone(),
            Some(_) => bail!("package({format:?}): arch must be text"),
            None => host.arch.clone(),
        };
        spec.insert("arch".into(), json!(arch));
        spec.entry("distro").or_insert_with(|| json!(host.distro));
        let mut texts = serde_json::Map::new();
        for (name, script) in scripts {
            let text = if script.is_none() {
                continue;
            } else if let Some(text) = script.unpack_str() {
                text.to_string()
            } else if is_function(script) {
                record(script, eval)?
                    .iter()
                    .filter_map(|step| step["run"].as_str())
                    .collect::<Vec<_>>()
                    .join("\n")
            } else {
                bail!(
                    "package({format:?}): scripts.{name} must be text or a function that calls sh()"
                );
            };
            texts.insert(name, Json::String(text));
        }
        spec.insert("scripts".into(), Json::Object(texts));
        if !steps.is_empty() {
            if arch != host.arch && arch != "all" {
                bail!(
                    "package({format:?}): a build on this system (arch {}) cannot make arch {arch}; build on a system of that arch",
                    host.arch
                );
            }
            spec.insert("job".into(), json!({"steps": steps, "exclude": exclude}));
        }
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
pub fn evaluate(file: &Path, args: BTreeMap<String, String>) -> Result<Vec<TargetSpec>> {
    evaluate_on(file, args, Host::detect())
}

/// Like `evaluate`, for a given system. Tests use it so that they do not depend on the machine.
pub fn evaluate_on(
    file: &Path,
    args: BTreeMap<String, String>,
    host: Host,
) -> Result<Vec<TargetSpec>> {
    let globals = globals();
    let collector = Collector {
        host,
        args,
        steps: RefCell::new(Vec::new()),
        exposed: RefCell::new(Vec::new()),
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

    fn run(source: &str, args: &[(&str, &str)]) -> Result<Vec<TargetSpec>> {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("pkgsmith.star");
        std::fs::write(&file, source).unwrap();
        evaluate_on(
            &file,
            args.iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
            fedora(),
        )
    }

    const COMMON: &str =
        "common = {'name': 'hi', 'version': arg('VERSION', '1.0.0'), 'description': 'Says hi'}\n";

    #[test]
    fn package_calls_share_the_build_and_the_exposed_files() {
        let specs = run(
            &format!(
                "{COMMON}def post_install():\n    sh('echo hi')\n\nsh('make')\nsh('make install DESTDIR=\"$pkgdir\"')\nexpose(file('hi', '/usr/bin/hi'), config(None, '/etc/hi.conf'))\npackage('deb', depends = ['libc6'], scripts = {{'post_install': post_install}}, **common)\npackage('rpm', distro = 'rocky', **common)\n"
            ),
            &[],
        )
        .unwrap();
        assert_eq!(specs.len(), 2);
        assert_eq!(
            (specs[0].name.as_str(), specs[0].version.as_str()),
            ("hi", "1.0.0")
        );
        assert_eq!(specs[0].depends, ["libc6"]);
        assert_eq!(
            specs[0].distro.as_deref(),
            Some("fedora"),
            "the distro of the host by default"
        );
        assert_eq!(specs[1].distro.as_deref(), Some("rocky"));
        for spec in &specs {
            assert_eq!(
                spec.job.as_ref().unwrap().steps.len(),
                2,
                "a script function does not join the build"
            );
            assert_eq!(spec.contents.len(), 2);
        }
        assert_eq!(specs[0].scripts.post_install.as_deref(), Some("echo hi"));
        assert_eq!(specs[1].scripts.post_install, None);
    }

    #[test]
    fn a_package_sees_only_what_came_before_it_and_only_reached_code_declares_packages() {
        let specs = run(
            &format!(
                "{COMMON}h = host()\nsh('make')\nexpose(file('a', '/a'))\nif h['distro'] == 'fedora':\n    package('rpm', **common)\nelif h['distro'] == 'debian':\n    package('deb', **common)\nsh('make more')\nexpose(file('b', '/b'))\npackage('tar', **common)\n"
            ),
            &[],
        )
        .unwrap();
        let formats: Vec<_> = specs.iter().map(|s| s.format.as_str()).collect();
        assert_eq!(formats, ["rpm", "tar"]);
        assert_eq!(
            (
                specs[0].job.as_ref().unwrap().steps.len(),
                specs[0].contents.len()
            ),
            (1, 1)
        );
        assert_eq!(
            (
                specs[1].job.as_ref().unwrap().steps.len(),
                specs[1].contents.len()
            ),
            (2, 2)
        );
    }

    #[test]
    fn a_package_without_a_build_holds_contents_only_and_a_wrong_arch_cannot_be_built() {
        let specs = run(
            &format!("{COMMON}package('msi', contents = [tree('stage', '/hi')], options = {{'prefix': '/hi'}}, **common)\n"),
            &[],
        )
        .unwrap();
        assert!(specs[0].job.is_none());
        assert_eq!(specs[0].options["prefix"], "/hi");
        let error = run(
            &format!("{COMMON}sh('x')\npackage('deb', arch = 'arm64', **common)\n"),
            &[],
        )
        .unwrap_err();
        assert!(
            format!("{error:#}").contains("cannot make arch arm64"),
            "{error:#}"
        );
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
        let error = run(
            &format!("{COMMON}package('tar', scripts = {{'post_install': 3}}, **common)\n"),
            &[],
        )
        .unwrap_err();
        assert!(
            format!("{error:#}").contains("must be text or a function"),
            "{error:#}"
        );
        let error = run("x = 1\n", &[]).unwrap_err();
        assert!(
            format!("{error:#}").contains("declares no package"),
            "{error:#}"
        );
    }

    #[test]
    fn a_script_loads_shared_code_and_cannot_read_other_files() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("common.star"), "DEPENDS = ['a']\n").unwrap();
        std::fs::write(
            dir.path().join("pkgsmith.star"),
            "load('common.star', 'DEPENDS')\npackage('tar', name = 'shared', version = '1', description = 'd', depends = DEPENDS)\n",
        )
        .unwrap();
        let specs =
            evaluate_on(&dir.path().join("pkgsmith.star"), BTreeMap::new(), fedora()).unwrap();
        assert_eq!(specs[0].depends, ["a"]);
        std::fs::write(dir.path().join("bad.star"), "open('/etc/passwd')\n").unwrap();
        assert!(evaluate_on(&dir.path().join("bad.star"), BTreeMap::new(), fedora()).is_err());
    }
}
