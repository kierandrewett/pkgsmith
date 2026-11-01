//! Run `pkgsmith.star` and collect the targets it declares.
//!
//! The language is Starlark, in the implementation of Meta (`starlark-rust`). A script has no files, network or processes
//! of its own: it can only compute, `load` other `.star` files, and read the build arguments with `arg`. What it declares
//! is a list of targets, so `pkgsmith plan` can show everything without running a build.

use crate::manifest::{Manifest, TargetSpec};
use anyhow::{Context, Result, anyhow, bail};
use starlark::any::ProvidesStaticType;
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

/// What the script reads and what it declares.
#[derive(Debug, ProvidesStaticType)]
struct Collector {
    /// `--build-arg NAME=VALUE`. They win over the environment.
    args: BTreeMap<String, String>,
    /// The targets, in the order the script declared them.
    specs: RefCell<Vec<serde_json::Value>>,
}

#[starlark_module]
fn builtins(builder: &mut GlobalsBuilder) {
    /// The build argument `name` (`--build-arg NAME=VALUE`), else the environment variable of that name, else `default`.
    fn arg<'v>(
        name: &str,
        default: Option<Value<'v>>,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<Value<'v>> {
        let collector = eval
            .extra
            .and_then(|extra| extra.downcast_ref::<Collector>())
            .ok_or_else(|| anyhow!("arg is not available here"))?;
        if let Some(value) = collector.args.get(name) {
            return Ok(eval.heap().alloc_str(value).to_value());
        }
        if let Ok(value) = std::env::var(name) {
            return Ok(eval.heap().alloc_str(&value).to_value());
        }
        Ok(default.unwrap_or_else(Value::new_none))
    }

    /// Used by `target`. Takes the dict that describes one package.
    fn _register<'v>(
        spec: Value<'v>,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<NoneType> {
        let collector = eval
            .extra
            .and_then(|extra| extra.downcast_ref::<Collector>())
            .ok_or_else(|| anyhow!("target is not available here"))?;
        collector.specs.borrow_mut().push(spec.to_json_value()?);
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

/// Run the script and give back its targets. `args` are the `--build-arg`s.
pub fn evaluate(file: &Path, args: BTreeMap<String, String>) -> Result<Vec<TargetSpec>> {
    let globals = globals();
    let collector = Collector {
        args,
        specs: RefCell::new(Vec::new()),
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
    let values = collector.specs.into_inner();
    if values.is_empty() {
        bail!("{} declares no target; call target(...)", file.display());
    }
    values
        .into_iter()
        .enumerate()
        .map(|(n, value)| {
            Manifest::parse_spec(value)
                .with_context(|| format!("target {} of {}", n + 1, file.display()))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(source: &str, args: &[(&str, &str)]) -> Result<Vec<TargetSpec>> {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("pkgsmith.star");
        std::fs::write(&file, source).unwrap();
        // Keep the folder alive until the script has run.
        let result = evaluate(
            &file,
            args.iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
        );
        drop(dir);
        result
    }

    const PKG: &str =
        "pkg = package(name = 'hi', version = arg('VERSION', '1.0.0'), description = 'Says hi')\n";

    #[test]
    fn a_script_declares_targets_with_functions_and_loops() {
        let specs = run(
            &format!(
                "{PKG}build = job('debian:13', [step('make', name = 'Build')])\nfor distro, fmt, deps in [('fedora', 'rpm', ['glibc']), ('debian', 'deb', ['libc6'])]:\n    target(pkg, fmt, distro = distro, job = build, depends = deps)\n"
            ),
            &[],
        )
        .unwrap();
        assert_eq!(specs.len(), 2);
        assert_eq!(
            (specs[1].format.as_str(), specs[1].depends.clone()),
            ("deb", vec!["libc6".to_string()])
        );
        assert_eq!(specs[0].job.as_ref().unwrap().runs_on, "debian:13");
        assert_eq!(specs[0].version, "1.0.0");
    }

    #[test]
    fn build_arguments_win_over_defaults_and_the_environment() {
        let specs = run(
            &format!("{PKG}target(pkg, 'tar')\n"),
            &[("VERSION", "2.0.0")],
        )
        .unwrap();
        assert_eq!(specs[0].version, "2.0.0");
    }

    #[test]
    fn a_misspelled_field_and_an_empty_script_are_errors_that_say_what_is_wrong() {
        let error = run(&format!("{PKG}target(pkg, 'tar', dependz = [])\n"), &[]).unwrap_err();
        assert!(
            format!("{error:#}").contains("unknown field 'dependz'"),
            "{error:#}"
        );
        let error = run("x = 1\n", &[]).unwrap_err();
        assert!(
            format!("{error:#}").contains("declares no target"),
            "{error:#}"
        );
        let error = run(
            &format!("{PKG}target(pkg, 'tar', job = {{'runs_on': 'a', 'steps': [], 'oops': 1}})\n"),
            &[],
        )
        .unwrap_err();
        assert!(format!("{error:#}").contains("oops"), "{error:#}");
    }

    #[test]
    fn a_script_loads_functions_from_a_file_next_to_it_and_cannot_read_other_files() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("common.star"),
            "def base(name):\n    return package(name = name, version = '1.0.0', description = 'd')\n",
        )
        .unwrap();
        std::fs::write(
            dir.path().join("pkgsmith.star"),
            "load('common.star', 'base')\ntarget(base('shared'), 'tar')\n",
        )
        .unwrap();
        let specs = evaluate(&dir.path().join("pkgsmith.star"), BTreeMap::new()).unwrap();
        assert_eq!(specs[0].name, "shared");
        std::fs::write(dir.path().join("bad.star"), "open('/etc/passwd')\n").unwrap();
        assert!(evaluate(&dir.path().join("bad.star"), BTreeMap::new()).is_err());
    }
}
