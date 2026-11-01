//! The example scripts are documentation. They must keep evaluating and planning, on every kind of system.

use pkgsmith::host::Host;
#[cfg(unix)]
use pkgsmith::manifest::Content;
use pkgsmith::plan::{self, Selection};
use pkgsmith::script;
#[cfg(unix)]
use pkgsmith::tree;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Every `.star` file below `folder`.
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

/// The formats of the plugins in this repository, and of the packers that need no other tool.
fn known() -> Vec<String> {
    [
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
    .to_vec()
}

fn args() -> BTreeMap<String, String> {
    [("VERSION", "1.2.3"), ("STAGE", "/tmp/stage")]
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .into()
}

/// A glibc Linux system, a musl one and a Mac. A script must give packages on each.
fn hosts() -> Vec<(&'static str, Host)> {
    vec![
        (
            "fedora",
            Host::from_os_release("ID=fedora\nVERSION_ID=43\n", "amd64", "glibc"),
        ),
        (
            "alpine",
            Host::from_os_release("ID=alpine\nVERSION_ID=3.22\n", "amd64", "musl"),
        ),
        (
            "macos",
            Host {
                os: "macos".into(),
                arch: "arm64".into(),
                distro: "macos".into(),
                like: Vec::new(),
                version: String::new(),
                libc: String::new(),
                format: "dmg".into(),
            },
        ),
    ]
}

#[test]
fn every_example_script_plans_on_every_host() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut files = vec![root.join("pkgsmith.star")];
    scripts(&root.join("examples"), &mut files);
    let known = known();
    let mut seen = 0;
    for path in files {
        // A file that only defines values for others to load declares no package.
        if path.file_name().is_some_and(|n| n == "common.star") {
            continue;
        }
        // The prefix is empty: planning reads no file of it, and the script must not need one.
        let prefix = tempfile::tempdir().unwrap();
        for (name, host) in hosts() {
            let label = format!("{} on {name}", path.display());
            let specs = script::evaluate_on(&path, args(), host, prefix.path())
                .unwrap_or_else(|e| panic!("{label}: {e:#}"));
            let base = path.parent().unwrap();
            let items = plan::items(specs, base, prefix.path(), &known, &Selection::default())
                .unwrap_or_else(|e| panic!("{label}: {e:#}"));
            assert!(!items.is_empty(), "{label} declares no package");
        }
        seen += 1;
    }
    assert!(seen >= 15, "expected the example scripts, saw {seen}");
}

#[cfg(unix)]
#[test]
fn a_script_packs_the_files_of_a_real_prefix() {
    use std::os::unix::fs::PermissionsExt;

    let dir = tempfile::tempdir().unwrap();
    let prefix = dir.path().join("prefix");
    for (path, text) in [
        ("usr/bin/hi", "#!/bin/sh\n"),
        ("usr/share/doc/hi/README", "hi\n"),
        ("usr/share/hi/data.txt", "data\n"),
        ("etc/hi/hi.conf", "mode=1\n"),
        ("etc/hi/extra.conf", "extra=1\n"),
    ] {
        let file = prefix.join(path);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(&file, text).unwrap();
    }
    std::fs::set_permissions(
        prefix.join("usr/bin/hi"),
        std::fs::Permissions::from_mode(0o755),
    )
    .unwrap();
    let script_file = dir.path().join("pkgsmith.star");
    std::fs::write(
        &script_file,
        r#"
package(
    "deb",
    name = "hi",
    version = "1.0.0",
    description = "d",
    exclude = ["/usr/share/doc/**"],
    include = ["/usr/bin/**", "/usr/share/**", "/etc/**"],
    config_files = ["/etc/hi/*.conf"],
)
"#,
    )
    .unwrap();
    let host = Host::from_os_release("ID=fedora\nVERSION_ID=43\n", "amd64", "glibc");
    let specs = script::evaluate_on(&script_file, BTreeMap::new(), host, &prefix).unwrap();
    let items = plan::items(specs, dir.path(), &prefix, &known(), &Selection::default()).unwrap();
    assert_eq!(items.len(), 1);
    let entries = tree::resolve(&items[0].manifest).unwrap();
    // The README is excluded, and so are the folders that the system owns (/etc, /usr, /usr/bin, /usr/share).
    let paths: Vec<&str> = entries.iter().map(|e| e.path.as_str()).collect();
    assert_eq!(
        paths,
        [
            "/etc/hi",
            "/etc/hi/extra.conf",
            "/etc/hi/hi.conf",
            "/usr/bin/hi",
            "/usr/share/hi",
            "/usr/share/hi/data.txt",
        ]
    );
    let by_path: BTreeMap<&str, _> = entries.iter().map(|e| (e.path.as_str(), e)).collect();
    assert!(by_path["/etc/hi/hi.conf"].config && by_path["/etc/hi/extra.conf"].config);
    assert!(!by_path["/usr/bin/hi"].config && !by_path["/usr/share/hi/data.txt"].config);
    assert_eq!(by_path["/usr/bin/hi"].mode, 0o755);
    // The content entry that the script exposed is the whole prefix.
    let Content { src, dst, .. } = &items[0].manifest.contents[0];
    assert_eq!((src.as_deref(), dst.as_str()), (Some("/"), "/"));
}

/// The paths of the files (not folders) that a package holds.
#[cfg(unix)]
fn file_paths(item: &plan::Item) -> Vec<String> {
    tree::resolve(&item.manifest)
        .unwrap()
        .into_iter()
        .filter(|e| !matches!(e.data, pkgsmith_plugin::Data::Dir))
        .map(|e| e.path)
        .collect()
}

#[cfg(unix)]
#[test]
fn the_split_example_gives_two_packages_with_different_files() {
    let dir = tempfile::tempdir().unwrap();
    let prefix = dir.path().join("prefix");
    let files = [
        "usr/bin/acme",
        "usr/lib/acme/plugin.so",
        "usr/share/doc/acme/README",
        "usr/share/man/man1/acme.1",
        "etc/acme.conf",
    ];
    for path in files {
        let file = prefix.join(path);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(&file, "x\n").unwrap();
    }
    let script_file = Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/split.star");
    let host = Host::from_os_release("ID=fedora\nVERSION_ID=43\n", "amd64", "glibc");
    let specs = script::evaluate_on(&script_file, BTreeMap::new(), host, &prefix).unwrap();
    let selection = Selection {
        formats: vec!["tar".into()],
        filters: Vec::new(),
    };
    let items = plan::items(
        specs,
        script_file.parent().unwrap(),
        &prefix,
        &known(),
        &selection,
    )
    .unwrap();
    assert_eq!(items.len(), 2);
    let program = file_paths(items.iter().find(|i| i.manifest.name == "acme").unwrap());
    let docs = file_paths(
        items
            .iter()
            .find(|i| i.manifest.name == "acme-doc")
            .unwrap(),
    );
    assert_eq!(
        program,
        ["/etc/acme.conf", "/usr/bin/acme", "/usr/lib/acme/plugin.so"]
    );
    assert_eq!(
        docs,
        ["/usr/share/doc/acme/README", "/usr/share/man/man1/acme.1"]
    );
    assert!(program.iter().all(|p| !docs.contains(p)));
}
