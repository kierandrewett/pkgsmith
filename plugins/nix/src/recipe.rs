//! The `nix` format: an archive that holds a flake, a `callPackage` expression and the files.
//!
//! The archive unpacks to `NAME-VERSION/` with
//! - `package.nix`: a function in the style of nixpkgs, `{ lib, stdenv, ... }: stdenv.mkDerivation { ... }`. Its
//!   arguments are the libraries of the package, so `callPackage` fills them and `.override` changes them.
//! - `flake.nix`: `packages.<system>`, and an overlay, over the `nixpkgs` input.
//! - `default.nix`: the same for `nix-build`, without flakes.
//! - `files/`: the content of the store path, with the prefix removed.
//!
//! The derivation builds nothing. It copies `files/` into `$out`, then the fixup phase runs: `autoPatchelfHook` points
//! the programs at the libraries of nixpkgs, and `patchShebangs` and the other hooks of `stdenv` do their work.
//!
//! Specifications used:
//! - `stdenv.mkDerivation` and its phases: https://nixos.org/manual/nixpkgs/stable/#chap-stdenv
//! - `autoPatchelfHook`: https://nixos.org/manual/nixpkgs/stable/#setup-hook-autopatchelfhook
//! - `meta` attributes: https://nixos.org/manual/nixpkgs/stable/#chap-meta
//! - `callPackage` and overlays: https://nixos.org/manual/nixpkgs/stable/#chap-overlays
//! - Flake outputs: https://nix.dev/manual/nix/stable/command-ref/new-cli/nix3-flake-check.html

use crate::nixlang::{Expr, attribute_name, is_identifier, string};
use crate::options::{self, DEFAULT_REWRITE, Options};
use anyhow::{Result, bail};
use pkgsmith_plugin::archive::{self, Compression, Leading};
use pkgsmith_plugin::version::Version;
use pkgsmith_plugin::{Data, Entry, Host, Package};
use std::path::PathBuf;

/// `name-version-arch.nix.tar.xz`.
pub fn file_name(package: &Package) -> Result<String> {
    let options = Options::of(package)?;
    let version = Version::parse(&package.version)?.plain();
    let compression = Compression::parse(options.compression.as_deref().unwrap_or("xz"))?;
    Ok(format!(
        "{}-{version}-{}.nix.tar{}",
        package.name,
        package.arch,
        compression.extension()
    ))
}

pub fn build(package: &Package, host: &Host) -> Result<PathBuf> {
    let options = Options::of(package)?;
    let version = Version::parse(&package.version)?.plain();
    let compression = Compression::parse(options.compression.as_deref().unwrap_or("xz"))?;
    options::warn_unexpressed(package, host);
    let entries = options::relocate(package, &options, host)?;
    options::warn_entries(&entries, host);
    let top = format!("{}-{version}", package.name);
    let leading = [
        text(&format!("{top}/flake.nix"), flake(package, &options)?),
        text(
            &format!("{top}/package.nix"),
            package_nix(package, &options, &version, &entries, host)?,
        ),
        text(&format!("{top}/default.nix"), default_nix()),
    ];
    let tar = archive::tar(&entries, &format!("{top}/files/"), package.mtime, &leading)?;
    let file = package.output(&file_name(package)?);
    std::fs::create_dir_all(&package.out)?;
    std::fs::write(&file, compression.compress(&tar)?)?;
    Ok(file)
}

fn text(name: &str, content: String) -> Leading {
    Leading {
        name: name.to_owned(),
        data: content.into_bytes(),
        mode: 0o644,
    }
}

/// An attribute path of nixpkgs such as `openssl` or `xorg.libX11`, checked so that it is code and nothing else.
fn attribute_path(path: &str, key: &str) -> Result<()> {
    if path.is_empty() || !path.split('.').all(is_identifier) {
        bail!(
            "the option {key} has {path:?}, which is not an attribute of nixpkgs such as openssl or xorg.libX11"
        );
    }
    Ok(())
}

/// The argument of `package.nix` that provides an attribute path: its first part.
fn head(path: &str) -> &str {
    path.split('.').next().unwrap_or(path)
}

/// `meta.license` from an SPDX expression such as `MIT` or `MIT OR Apache-2.0`. nixpkgs knows the SPDX ids, so
/// `lib.getLicenseFromSpdxId` maps them. Text that is no SPDX expression, and every license of an unfree package,
/// becomes a license set with its `shortName`.
fn license(text: &str, unfree: bool) -> Expr {
    let words: Vec<&str> = text
        .split(|c: char| c.is_whitespace() || c == '(' || c == ')')
        .filter(|w| !w.is_empty())
        .collect();
    let mut ids = Vec::new();
    let mut after_with = false;
    for word in &words {
        if after_with {
            after_with = false;
        } else if word.eq_ignore_ascii_case("with") {
            after_with = true;
        } else if !(word.eq_ignore_ascii_case("or") || word.eq_ignore_ascii_case("and")) {
            ids.push(*word);
        }
    }
    let spdx = !unfree
        && !ids.is_empty()
        && ids.iter().all(|id| {
            id.chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '+'))
        });
    if !spdx {
        let mut set = vec![("shortName".to_owned(), Expr::Str(text.to_owned()))];
        if unfree {
            set.push(("free".into(), Expr::Bool(false)));
        }
        return Expr::Attrs(set);
    }
    let call = |id: &str| format!("lib.getLicenseFromSpdxId {}", string(id));
    if ids.len() == 1 {
        Expr::Raw(call(ids[0]))
    } else {
        Expr::List(
            ids.into_iter()
                .map(|id| Expr::Raw(format!("({})", call(id))))
                .collect(),
        )
    }
}

/// `package.nix`.
fn package_nix(
    package: &Package,
    options: &Options,
    version: &str,
    entries: &[Entry],
    host: &Host,
) -> Result<String> {
    for (key, list) in [
        ("inputs", &options.inputs),
        ("native_inputs", &options.native_inputs),
        ("runtime_inputs", &options.runtime_inputs),
    ] {
        for path in list {
            attribute_path(path, key)?;
        }
    }
    let elf: Vec<Option<bool>> = entries
        .iter()
        .filter_map(|e| match &e.data {
            Data::File { src, .. } => Some(options::elf_is_dynamic(src)),
            _ => None,
        })
        .collect();
    let patchelf = options
        .patchelf
        .unwrap_or_else(|| elf.contains(&Some(true)));
    if options.patchelf.is_none() && patchelf {
        host.info("the package has dynamically linked programs, so autoPatchelfHook is on; name their libraries in the option inputs, and set patchelf to false to keep them as built");
    }
    let native_code = elf.iter().any(Option::is_some);

    let mut native: Vec<String> = Vec::new();
    if patchelf {
        native.push("autoPatchelfHook".into());
    }
    native.extend(options.native_inputs.iter().cloned());

    // The arguments of the function: `lib`, the standard environment, then the first part of every attribute.
    let environment = if patchelf { "stdenv" } else { "stdenvNoCC" };
    let mut arguments: Vec<String> = vec!["lib".into(), environment.into()];
    for path in native
        .iter()
        .chain(&options.inputs)
        .chain(&options.runtime_inputs)
    {
        let argument = head(path).to_owned();
        if !arguments.contains(&argument) {
            arguments.push(argument);
        }
    }

    let mut attrs: Vec<(String, Expr)> = vec![
        ("pname".into(), Expr::Str(package.name.clone())),
        ("version".into(), Expr::Str(version.to_owned())),
        ("src".into(), Expr::Raw("./files".into())),
    ];
    for (key, list) in [
        ("nativeBuildInputs", &native),
        ("buildInputs", &options.inputs),
        ("runtimeDependencies", &options.runtime_inputs),
    ] {
        if !list.is_empty() {
            attrs.push((key.into(), Expr::raws(list)));
        }
    }
    if !options.ignore_missing.is_empty() {
        attrs.push((
            "autoPatchelfIgnoreMissingDeps".into(),
            Expr::strings(&options.ignore_missing),
        ));
    }
    attrs.push(("dontConfigure".into(), Expr::Bool(true)));
    attrs.push(("dontBuild".into(), Expr::Bool(true)));
    if !options.strip {
        attrs.push(("dontStrip".into(), Expr::Bool(true)));
    }
    attrs.push((
        "installPhase".into(),
        Expr::Script(
            "runHook preInstall\nmkdir -p $out\ncp -a . $out/\nrunHook postInstall".into(),
        ),
    ));
    let rewrites = rewrites(options, entries)?;
    if !rewrites.is_empty() {
        let prefix = options.prefix()?;
        let mut script = String::new();
        for path in rewrites {
            script.push_str(&format!(
                "substituteInPlace {} --replace-fail {} \"$out/\"\n",
                shell_path(&path),
                shell_word(&format!("{prefix}/")),
            ));
        }
        attrs.push(("postInstall".into(), Expr::Script(script)));
    }

    let mut meta: Vec<(String, Expr)> = Vec::new();
    if !package.summary().is_empty() {
        meta.push((
            "description".into(),
            Expr::Str(package.summary().to_owned()),
        ));
    }
    let long = package
        .description
        .lines()
        .skip(1)
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_owned();
    if !long.is_empty() {
        meta.push(("longDescription".into(), Expr::Str(long)));
    }
    if let Some(homepage) = &package.homepage {
        meta.push(("homepage".into(), Expr::Str(homepage.clone())));
    }
    if let Some(spdx) = &package.license {
        meta.push(("license".into(), license(spdx, options.unfree)));
    }
    if native_code {
        meta.push((
            "sourceProvenance".into(),
            Expr::Raw("[ lib.sourceTypes.binaryNativeCode ]".into()),
        ));
    }
    meta.push((
        "platforms".into(),
        match options::systems(package, options)? {
            Some(systems) => Expr::strings(systems),
            None => Expr::Raw("lib.platforms.all".into()),
        },
    ));
    let main = options.main_program.clone().or_else(|| {
        let wanted = format!("/bin/{}", package.name);
        entries
            .iter()
            .any(|e| e.path == wanted && !e.is_dir())
            .then(|| package.name.clone())
    });
    if let Some(main) = main {
        meta.push(("mainProgram".into(), Expr::Str(main)));
    }
    attrs.push(("meta".into(), Expr::Attrs(meta)));

    let arguments = arguments
        .iter()
        .map(|a| format!("  {a},\n"))
        .collect::<String>();
    Ok(format!(
        "{{\n{arguments}}}:\n\n{environment}.mkDerivation {}\n",
        Expr::Attrs(attrs).render(0)
    ))
}

/// The text files that name the prefix and must name `$out`: the files that match `rewrite` and hold `PREFIX/`.
fn rewrites(options: &Options, entries: &[Entry]) -> Result<Vec<String>> {
    let prefix = options.prefix()?;
    if prefix.is_empty() {
        return Ok(Vec::new());
    }
    let globs: Vec<&str> = match &options.rewrite {
        Some(list) => list.iter().map(String::as_str).collect(),
        None => DEFAULT_REWRITE.to_vec(),
    };
    let needle = format!("{prefix}/");
    let mut found = Vec::new();
    for entry in entries {
        let Data::File { src, .. } = &entry.data else {
            continue;
        };
        let relative = entry.path.trim_start_matches('/');
        if !globs.iter().any(|g| options::glob(g, relative)) {
            continue;
        }
        let bytes = std::fs::read(src)?;
        if bytes.windows(needle.len()).any(|w| w == needle.as_bytes()) {
            found.push(relative.to_owned());
        }
    }
    Ok(found)
}

/// `"$out/path"` for a shell, with the characters that a double-quoted word expands escaped.
fn shell_path(relative: &str) -> String {
    let mut out = String::from("\"$out/");
    for c in relative.chars() {
        if matches!(c, '"' | '\\' | '$' | '`') {
            out.push('\\');
        }
        out.push(c);
    }
    out.push('"');
    out
}

/// A word in single quotes.
fn shell_word(text: &str) -> String {
    format!("'{}'", text.replace('\'', "'\\''"))
}

/// `default.nix`, for `nix-build` without flakes.
fn default_nix() -> String {
    "{\n  pkgs ? import <nixpkgs> { },\n}:\n\npkgs.callPackage ./package.nix { }\n".into()
}

/// `flake.nix`.
fn flake(package: &Package, options: &Options) -> Result<String> {
    let nixpkgs = options
        .nixpkgs
        .as_deref()
        .unwrap_or("github:NixOS/nixpkgs/nixos-unstable");
    if nixpkgs.is_empty()
        || !nixpkgs
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || ":/@._-?=&".contains(c))
    {
        bail!(
            "the option nixpkgs has {nixpkgs:?}, which is not a flake reference such as github:NixOS/nixpkgs/nixos-unstable"
        );
    }
    let systems = match options::systems(package, options)? {
        Some(list) => Expr::strings(list).render(3),
        None => "nixpkgs.lib.systems.flakeExposed".into(),
    };
    let summary = if package.summary().is_empty() {
        &package.name
    } else {
        package.summary()
    };
    let attr = attribute_name(&package.name);
    // A package with an unfree license is evaluated by nixpkgs only when it is allowed, and only this package is.
    let pkgs = if options.unfree {
        format!(
            "import nixpkgs {{\n            inherit system;\n            config.allowUnfreePredicate = pkg: nixpkgs.lib.getName pkg == {};\n          }}",
            string(&package.name)
        )
    } else {
        "nixpkgs.legacyPackages.${system}".into()
    };
    Ok(format!(
        "{{\n  description = {};\n\n  inputs.nixpkgs.url = {};\n\n  outputs =\n    {{ self, nixpkgs }}:\n    let\n      systems = {systems};\n      forAllSystems = nixpkgs.lib.genAttrs systems;\n    in\n    {{\n      overlays.default = final: _prev: {{\n        {attr} = final.callPackage ./package.nix {{ }};\n      }};\n\n      packages = forAllSystems (\n        system:\n        let\n          pkgs = {pkgs};\n        in\n        {{\n          {attr} = pkgs.callPackage ./package.nix {{ }};\n          default = self.packages.${{system}}.{attr};\n        }}\n      );\n    }};\n}}\n",
        string(summary),
        string(nixpkgs),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spdx_licenses_use_the_nixpkgs_lookup() {
        assert_eq!(
            license("MIT", false).render(0),
            "lib.getLicenseFromSpdxId \"MIT\""
        );
        assert_eq!(
            license("(MIT OR Apache-2.0)", false).render(0),
            "[ (lib.getLicenseFromSpdxId \"MIT\") (lib.getLicenseFromSpdxId \"Apache-2.0\") ]"
        );
        assert_eq!(
            license("GPL-2.0-or-later WITH Classpath-exception-2.0", false).render(0),
            "lib.getLicenseFromSpdxId \"GPL-2.0-or-later\""
        );
    }

    #[test]
    fn other_licenses_keep_their_text() {
        assert_eq!(
            license("Custom terms, see LICENSE", false).render(0),
            "{\n  shortName = \"Custom terms, see LICENSE\";\n}"
        );
        assert_eq!(
            license("MIT", true).render(0),
            "{\n  shortName = \"MIT\";\n  free = false;\n}"
        );
    }

    #[test]
    fn attributes_are_code_and_nothing_else() {
        assert!(attribute_path("xorg.libX11", "inputs").is_ok());
        assert!(attribute_path("stdenv.cc.cc.lib", "inputs").is_ok());
        for bad in ["", "a b", "a;b", "1a", "a..b", "a.", "${x}", "a.let"] {
            assert!(attribute_path(bad, "inputs").is_err(), "{bad:?}");
        }
    }

    #[test]
    fn shell_words_are_quoted() {
        assert_eq!(
            shell_path("lib/a b\"$x.service"),
            "\"$out/lib/a b\\\"\\$x.service\""
        );
        assert_eq!(shell_word("/usr/"), "'/usr/'");
        assert_eq!(shell_word("a'b"), "'a'\\''b'");
    }

    #[test]
    fn a_script_package_gets_a_plain_derivation() {
        let dir = tempfile::tempdir().unwrap();
        let package = crate::testing::package(
            dir.path(),
            "nix",
            &[
                ("/usr/bin/hi", "#!/bin/sh\n", 0o755),
                (
                    "/usr/lib/systemd/system/hi.service",
                    "ExecStart=/usr/bin/hi\n",
                    0o644,
                ),
                ("/usr/share/doc/hi/README", "no prefix here\n", 0o644),
            ],
            serde_json::json!({"main_program": "hi-cli"}),
        );
        let options = Options::of(&package).unwrap();
        let entries = options::relocate(&package, &options, &Host::silent()).unwrap();
        let text = package_nix(&package, &options, "1.0.0", &entries, &Host::silent()).unwrap();
        let expected = r#"{
  lib,
  stdenvNoCC,
}:

stdenvNoCC.mkDerivation {
  pname = "hi";
  version = "1.0.0";
  src = ./files;
  dontConfigure = true;
  dontBuild = true;
  dontStrip = true;
  installPhase = ''
    runHook preInstall
    mkdir -p $out
    cp -a . $out/
    runHook postInstall
  '';
  postInstall = ''
    substituteInPlace "$out/lib/systemd/system/hi.service" --replace-fail '/usr/' "$out/"
  '';
  meta = {
    description = "Says hi";
    longDescription = "More text.";
    homepage = "https://example.org/hi";
    license = lib.getLicenseFromSpdxId "MIT";
    platforms = [ "x86_64-linux" ];
    mainProgram = "hi-cli";
  };
}
"#;
        assert_eq!(text, expected);
    }

    #[test]
    fn inputs_become_arguments_and_a_dynamic_program_turns_on_the_hook() {
        let dir = tempfile::tempdir().unwrap();
        let mut package = crate::testing::package(
            dir.path(),
            "nix",
            &[("/usr/bin/hi", "x", 0o755)],
            serde_json::json!({
                "inputs": ["openssl", "xorg.libX11", "stdenv.cc.cc.lib"],
                "runtime_inputs": ["libGL"],
                "patchelf": true,
                "ignore_missing": ["libcuda.so.1"],
                "strip": true,
                "rewrite": [],
            }),
        );
        package.arch = "arm64".into();
        package.license = Some("Proprietary, see LICENSE".into());
        let options = Options::of(&package).unwrap();
        let entries = options::relocate(&package, &options, &Host::silent()).unwrap();
        let text = package_nix(&package, &options, "1.0.0", &entries, &Host::silent()).unwrap();
        assert!(text.starts_with(
            "{\n  lib,\n  stdenv,\n  autoPatchelfHook,\n  openssl,\n  xorg,\n  libGL,\n}:\n\nstdenv.mkDerivation {"
        ));
        assert!(text.contains("  nativeBuildInputs = [ autoPatchelfHook ];\n"));
        assert!(text.contains("  buildInputs = [ openssl xorg.libX11 stdenv.cc.cc.lib ];\n"));
        assert!(text.contains("  runtimeDependencies = [ libGL ];\n"));
        assert!(text.contains("  autoPatchelfIgnoreMissingDeps = [ \"libcuda.so.1\" ];\n"));
        assert!(!text.contains("dontStrip"));
        assert!(!text.contains("postInstall = "));
        assert!(text.contains("platforms = [ \"aarch64-linux\" ];"));
        assert!(
            text.contains("license = {\n      shortName = \"Proprietary, see LICENSE\";\n    };")
        );
    }

    #[test]
    fn a_bad_input_is_an_error_and_so_is_an_option_of_another_format() {
        let dir = tempfile::tempdir().unwrap();
        let mut package = crate::testing::package(
            dir.path(),
            "nix",
            &[("/usr/bin/hi", "x", 0o755)],
            serde_json::json!({"inputs": ["openssl; rm -rf /"]}),
        );
        let options = Options::of(&package).unwrap();
        let entries = options::relocate(&package, &options, &Host::silent()).unwrap();
        assert!(package_nix(&package, &options, "1.0.0", &entries, &Host::silent()).is_err());
        package.options = serde_json::json!({"store_dir": "/nix/store"});
        assert!(Options::of(&package).is_err());
        package.options = serde_json::json!({"typo": 1});
        assert!(Options::of(&package).is_err());
    }

    #[test]
    fn the_flake_lists_the_systems_and_follows_the_nixpkgs_option() {
        let dir = tempfile::tempdir().unwrap();
        let mut package = crate::testing::package(
            dir.path(),
            "nix",
            &[("/usr/bin/hi", "x", 0o755)],
            serde_json::json!({"nixpkgs": "github:NixOS/nixpkgs/nixos-25.05"}),
        );
        let text = flake(&package, &Options::of(&package).unwrap()).unwrap();
        assert!(text.contains("inputs.nixpkgs.url = \"github:NixOS/nixpkgs/nixos-25.05\";"));
        assert!(text.contains("systems = [ \"x86_64-linux\" ];"));
        assert!(text.contains("hi = pkgs.callPackage ./package.nix { };"));
        assert!(text.contains("pkgs = nixpkgs.legacyPackages.${system};"));
        package.arch = "all".into();
        package.options = serde_json::json!({"unfree": true});
        let text = flake(&package, &Options::of(&package).unwrap()).unwrap();
        assert!(text.contains("systems = nixpkgs.lib.systems.flakeExposed;"));
        assert!(text.contains("allowUnfreePredicate = pkg: nixpkgs.lib.getName pkg == \"hi\";"));
        package.options = serde_json::json!({"nixpkgs": "x y"});
        assert!(flake(&package, &Options::of(&package).unwrap()).is_err());
    }

    #[test]
    fn the_archive_holds_the_expression_and_the_files_below_the_prefix() {
        let dir = tempfile::tempdir().unwrap();
        let package = crate::testing::package(
            dir.path(),
            "nix",
            &[
                ("/usr/bin/hi", "#!/bin/sh\n", 0o755),
                ("/etc/hi.conf", "a=1\n", 0o644),
            ],
            serde_json::json!({"compression": "xz"}),
        );
        let file = build(&package, &Host::silent()).unwrap();
        assert_eq!(file.file_name().unwrap(), "hi-1.0.0-amd64.nix.tar.xz");
        let mut tar = Vec::new();
        std::io::copy(
            &mut liblzma::read::XzDecoder::new(std::fs::File::open(file).unwrap()),
            &mut tar,
        )
        .unwrap();
        let members = crate::testing::members(&tar);
        let names: Vec<&str> = members.keys().map(String::as_str).collect();
        assert_eq!(
            names,
            [
                "hi-1.0.0/default.nix",
                "hi-1.0.0/files/bin/hi",
                "hi-1.0.0/files/etc/hi.conf",
                "hi-1.0.0/flake.nix",
                "hi-1.0.0/package.nix",
            ]
        );
    }
}
