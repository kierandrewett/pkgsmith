//! The example manifests are documentation. They must keep parsing and binding, for every job they describe.

use pkgsmith::expand::Scope;
use pkgsmith::manifest::Manifest;
use pkgsmith::plan;
use std::path::Path;

#[test]
fn every_example_manifest_binds_for_every_job() {
    let folder = Path::new(env!("CARGO_MANIFEST_DIR")).join("examples");
    let mut scope = Scope::default();
    scope.set("VERSION", "1.2.3");
    scope.set("STAGE", "/tmp/stage");
    // The formats of the plugins in this repository.
    let known: Vec<String> = ["tar", "zip", "deb", "ipk", "archlinux", "rpm", "apk"]
        .map(String::from)
        .to_vec();
    let mut seen = 0;
    for entry in std::fs::read_dir(&folder).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_none_or(|e| e != "yaml") {
            continue;
        }
        let manifest =
            Manifest::parse(&path).unwrap_or_else(|e| panic!("{}: {e:#}", path.display()));
        manifest
            .check_options(&known)
            .unwrap_or_else(|e| panic!("{}: {e:#}", path.display()));
        let items = plan::items(&manifest, &known, &[], &[], None).unwrap();
        assert!(!items.is_empty(), "{} describes no job", path.display());
        for item in items {
            manifest
                .bind(&scope, &item.job, &item.format)
                .unwrap_or_else(|e| panic!("{} {}: {e:#}", path.display(), item.label()));
        }
        seen += 1;
    }
    assert!(
        seen >= 2,
        "expected the example manifests in {}",
        folder.display()
    );
}
