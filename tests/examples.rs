//! The example scripts are documentation. They must keep evaluating and planning, for every target they declare.

use pkgsmith::plan::{self, Selection};
use pkgsmith::script;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Every `.star` file below `folder`, except the ones that other scripts load.
fn scripts(folder: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(folder).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            scripts(&path, out);
        } else if path.extension().is_some_and(|e| e == "star") {
            out.push(path);
        }
    }
}

#[test]
fn every_example_script_plans() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut files = vec![root.join("pkgsmith.star")];
    scripts(&root.join("examples"), &mut files);
    // The formats of the plugins in this repository, and of the packers that need no job.
    let known: Vec<String> = [
        "tar",
        "zip",
        "deb",
        "ipk",
        "archlinux",
        "rpm",
        "apk",
        "dmg",
        "msi",
        "nsis",
    ]
    .map(String::from)
    .to_vec();
    let args: BTreeMap<String, String> = [("VERSION", "1.2.3"), ("STAGE", "/tmp/stage")]
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .into();
    let mut seen = 0;
    for path in files {
        // A file that only defines values for others to load declares no target.
        if path.file_name().is_some_and(|n| n == "common.star") {
            continue;
        }
        let specs = script::evaluate(&path, args.clone())
            .unwrap_or_else(|e| panic!("{}: {e:#}", path.display()));
        let base = path.parent().unwrap();
        let items = plan::items(specs, base, base, &known, &Selection::default())
            .unwrap_or_else(|e| panic!("{}: {e:#}", path.display()));
        assert!(!items.is_empty(), "{} declares no target", path.display());
        seen += 1;
    }
    assert!(seen >= 10, "expected the example scripts, saw {seen}");
}
