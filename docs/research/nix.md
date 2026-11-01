# Nix packages from a prefix: options

This note compares ways for pkgsmith to write Nix-consumable packages from a prefix. It says which parts are facts from
the Nix and nixpkgs documentation or from a run, and which parts are judgement. The decision is at the end. The
implementation is the three `nix` formats in [formats](../formats.md#nix).

Sources are the [Nix Reference Manual](https://nix.dev/manual/nix/stable/), the
[Nixpkgs manual](https://nixos.org/manual/nixpkgs/stable/) and the source of Nix in
[`src/libstore`](https://github.com/NixOS/nix/tree/master/src/libstore). Runs used Nix 2.35.4 in `docker.io/nixos/nix:latest`
as root (single-user), with network access, on 2026-11-01.

## The problem

pkgsmith receives a prefix: files that someone built and installed with their own tools, laid out like an installed Linux
system (`usr/bin/hi`, `usr/lib/systemd/system/hi.service`, `etc/hi.conf`). pkgsmith builds nothing and runs no command.
Every other format puts these files in a container that a package manager unpacks at `/`. Nix has no such container: the
files must end up in `/nix/store/DIGEST-NAME`, and nothing else may be written outside the store.

Two things make this different from `rpm` and `deb`:

1. A program in the store finds its libraries by absolute store paths. A program built for `/lib64/ld-linux-x86-64.so.2`
   does not run on NixOS, which has no such file.
2. A store path has a digest that Nix checks, and a package has no install script that could fix things afterwards.

## Facts about Nix

### Store objects and store paths

- A store object is a file system object, its references to other store objects, and metadata. Its path is
  `STORE_DIR/DIGEST-NAME`, and `/nix/store` is the usual `STORE_DIR`.
  ([Store object](https://nix.dev/manual/nix/stable/store/store-object.html))
- The digest of a built path comes from the derivation, so it is *input-addressed*. A store object can also be
  *content-addressed*: its digest comes only from its content, its references, the store directory and its name.
  ([Content-addressing store objects](https://nix.dev/manual/nix/stable/store/store-object/content-address.html))
- For the NAR method with SHA-256, the fingerprint is
  `source[:REFERENCE...][:self]:sha256:HEX(SHA256(NAR)):STORE_DIR:NAME`. The digest is the SHA-256 of the fingerprint,
  XOR-folded to 20 bytes and written in Nix32.
  ([Store path specification](https://nix.dev/manual/nix/stable/protocols/store-path.html),
  [Nix32](https://nix.dev/manual/nix/stable/protocols/nix32.html)) The manual says that data hashed this way "always uses the
  `source:...` construction, regardless of how it was produced", so `nix-store --add`, a fixed-output derivation and a
  hand-written cache give the same path for the same content.
- A NAR is a fixed serialisation of a file tree with sorted names, an executable flag and symbolic links, and with no
  owners, times or modes.
  ([NAR format](https://nix.dev/manual/nix/stable/protocols/nix-archive.html))
- Check (run): `nix store add --mode nar --name x-1 d` for a folder `d` that holds the file `f` with the text `hello\n`
  printed `/nix/store/34iwnl73jrwnfm9znrglzgjk0v6pjff0-x-1`. The plugin computes the same path, and a unit test holds it.

### References and closures

- A reference is an edge between store objects. The closure of a path is everything reachable by references, and "for
  correct deployment it is necessary to deploy whole closures".
  ([Glossary](https://nix.dev/manual/nix/stable/glossary.html))
- Nix finds references by scanning the output of a build for the digests of the paths that the build could use.
  pkgsmith has no build and no list of candidate paths.

### Binary caches and `nix copy`

- A binary cache keeps metadata in `.narinfo` files, not in a database. `nix copy --from file:///DIR PATH` and a
  `substituters` entry read it. Nixpkgs documents the same flat layout for `mkBinaryCache`.
  ([Glossary](https://nix.dev/manual/nix/stable/glossary.html),
  [`nix copy`](https://nix.dev/manual/nix/stable/command-ref/new-cli/nix3-copy.html),
  [`mkBinaryCache`](https://nixos.org/manual/nixpkgs/stable/#sec-pkgs-binary-cache))
- A narinfo has `StorePath`, `URL`, `Compression`, `FileHash`, `FileSize`, `NarHash`, `NarSize`, `References`, optionally
  `Deriver`, `Sig` and `CA`. ([`nar-info.cc`](https://github.com/NixOS/nix/blob/master/src/libstore/nar-info.cc))
- A store accepts a path without a signature when the path is content-addressed: the narinfo gives `CA`, and the path that
  Nix computes from it equals `StorePath`. In the source, `ValidPathInfo::checkSignatures` returns at once for such a
  path, and `LocalStore::addToStore` otherwise throws "cannot add path ... because it lacks a signature by a trusted key"
  (`path-info.cc`, `local-store.cc`).
  - Check (run): a cache that pkgsmith wrote was read by `nix copy --from file:///cache PATH`, with signature checking
    left on. `nix path-info --json` shows `"ca":"fixed:r:sha256:..."` and no signature.
  - Check (run): a narinfo that was edited to claim another digest made Nix 2.35.4 stop with the internal assertion
    `storePathForDst == storePathForSrc` in `copyPaths`. The path was not added. The failure is an abort, not an error
    message. The assertion is made only for a path without references. For a path with references, the source suggests
    that Nix treats the path as not content-addressed and asks for a signature. That case was not run.

### `nix-store --export` and `--import`

- `--import` reads the stream that `--export` writes: for each path the number `1`, the NAR, the magic `0x4558494e`, the
  path, the references, the deriver and a signature flag; then `0`. Paths that are in the store already are ignored, and a
  reference that is missing makes the import fail.
  ([`nix-store --import`](https://nix.dev/manual/nix/stable/command-ref/nix-store/import.html),
  [`export-import.cc`](https://github.com/NixOS/nix/blob/master/src/libstore/export-import.cc))
- The stream has no `CA` field. Check (run): after `nix-store --import`, `nix path-info --json` shows `"ca":null`.
- `importPaths` takes a `CheckSigsFlag`, and `nix-store --import` passes `NoCheckSigs`
  (`src/nix/nix-store/nix-store.cc`, lines 791 and 992 at commit `8c8e8b8`). Whether a daemon accepts the stream from a user
  that is not trusted was not checked.

### Derivations, flakes and nixpkgs

- A derivation is a pure function that makes store objects. A fixed-output derivation has a hash that is known in
  advance, and its builder may use the network. ([Glossary](https://nix.dev/manual/nix/stable/glossary.html))
- A flake's `packages.<system>.<name>` is built by `nix build .#name` and run by `nix run`. The `default` attribute is what
  `nix build .` builds. `overlays.<name>` and `nixosModules.<name>` are the other output types that `nix flake check`
  knows. ([`nix flake check`](https://nix.dev/manual/nix/stable/command-ref/new-cli/nix3-flake-check.html))
- `stdenv.mkDerivation` has phases. With `dontConfigure` and `dontBuild`, `installPhase` can copy files. The fixup phase then
  strips, shrinks `RPATH`, patches shebangs and runs hooks. `stdenvNoCC` is the same without a compiler.
  ([Standard environment](https://nixos.org/manual/nixpkgs/stable/#chap-stdenv))
- `autoPatchelfHook` looks for missing shared libraries of ELF files in `buildInputs` and `nativeBuildInputs`.
  `runtimeDependencies` adds libraries unconditionally, "useful for programs that use `dlopen`". By default the hook
  fails when a library is not found, and `autoPatchelfIgnoreMissingDeps` relaxes that.
  ([Hooks](https://nixos.org/manual/nixpkgs/stable/#setup-hook-autopatchelfhook))
- `meta` carries `description`, `longDescription`, `homepage`, `license`, `sourceProvenance`, `platforms` and
  `mainProgram`. `lib.getLicenseFromSpdxId` maps an SPDX id to `lib.licenses` and falls back to `{ shortName = ...; }`.
  An unfree license stops evaluation unless the user allows it.
  ([Meta](https://nixos.org/manual/nixpkgs/stable/#chap-meta))
- `callPackage` fills the arguments of a function from a package set, and `.override` replaces them. An overlay
  `final: prev: { ... }` adds packages to a set.
  ([Overlays](https://nixos.org/manual/nixpkgs/stable/#chap-overlays))

## What the plugin did before

Facts from reading `plugins/nix` before the rework:

- One format, `nix`. It wrote `NAME-VERSION/default.nix`, `flake.nix` and `files/` into a tar.
- `default.nix` was `{ pkgs ? import <nixpkgs> { } }: with pkgs; stdenv.mkDerivation { ... }`. It used `with pkgs;`, the
  channel from `NIX_PATH`, `stdenv` always, a fixed `installPhase` string and strings joined with `format!`.
- A `match` with 18 arms mapped SPDX ids to `lib.licenses` names. Any other id became `{ spdxId = ...; shortName = ...; }`,
  which lacks the other fields of a license such as `free`.
- A match on `arch` gave `x86_64-linux`, `aarch64-linux`, and for `all` a fixed list of four systems.
- `autoPatchelfHook` was an opt-in boolean. A dynamically linked program built without it did not run on NixOS.
- No `mainProgram`, no `sourceProvenance`, no `longDescription`, no `runtimeDependencies`, no way to unset `strip`.
- `flake.nix` was built by pasting strings. It imported nothing for unfree licenses.
- The `.nix.tar` was the only artefact. No store path was computed, so nothing could go into a binary cache.
- Files such as `lib/systemd/system/hi.service` kept `ExecStart=/usr/bin/hi`.

Judgement: the rework should remove the tables that nixpkgs already has (licenses, systems), build expressions from
values instead of text, and make the choices that Nix expects (explicit inputs, patching, `meta`) data in options with no
package name in the code.

## Options

Each option is a way to turn the prefix into something Nix consumes.

### 1. A tarball plus a generated derivation or flake that unpacks and copies

The package is the files plus Nix code that copies them to `$out`. This is what existed.

- Pros: the user needs only Nix and the file. nixpkgs provides libraries and the loader, so a derivation can patch the
  programs for NixOS. It works with `nix build URL`, `nix run`, `nix profile`, and as an input of another flake.
- Cons: the store path is input-addressed, so it differs between nixpkgs revisions and cannot be known by pkgsmith or put
  in a cache by pkgsmith. The user must build (copy and patch) on their machine, with a nixpkgs that they trust. A flake
  without `flake.lock` takes whatever nixpkgs revision resolves.
- Risk: the flake input is not pinned by pkgsmith, so two users get different libraries.

### 2. The same, written the way nixpkgs writes a binary package

This is option 1 with `autoPatchelfHook`, `meta`, `callPackage`-style arguments and an overlay.

- Pros: follows nixpkgs practice for packages of prebuilt binaries. `.override` and `.overrideAttrs` work. Unfree and
  platform checks work because `meta` is complete. `inputs`, `runtime_inputs` and `ignore_missing` map to attributes that
  Nix already has. nixpkgs maps licenses, so no table is kept.
- Cons: libraries are named by nixpkgs attribute, which the user must know. Patching changes the files, so the result is
  not the bytes of the prefix. A missing library fails the build, by design, at the user's side.
- Risk: attribute names change between nixpkgs revisions. pkgsmith checks only that an attribute path is syntactically
  a path.

### 3. A store path: NAR plus narinfo for a binary cache

Compute the NAR of the files, the store path as content-addressed, and write a cache directory.

- Pros: the result is exactly the bytes of the prefix, with one name that is the same on every machine. `nix copy`,
  `nix build --substituters` and an HTTP server all work. No signature is needed, because Nix recomputes the path from the
  content. No nixpkgs revision matters. References that the files spell are kept, so a closure copies as a unit.
- Cons: nothing patches the files. A prefix built for a distro points to `/lib64`, so the path runs on a system that has
  those files and does not run on NixOS. The path is not what `nix build nixpkgs#hi` gives, so it is a separate identity from
  any package that nixpkgs has. The user must know the store path, which only pkgsmith's output prints. No `Sig:` is written, so
  a store that requires trusted keys for non-content-addressed paths gets no extra trust from a key.
- Risk: a reference that pkgsmith finds by reading text may be wrong (a store path that appears only in a comment), and
  the import then fails. The wrong-digest case ends in an abort of `nix` and not an error text.

### 4. The export stream for `nix-store --import`

The same NAR, in one file.

- Pros: one file, no directory and no server. Works offline.
- Cons: no `CA` field on import. A trusted user is needed, so it does not fit multi-user Nix with an unprivileged user.
  The stream is an old interface, kept for `nix-store` only.

### 5. Other ways, not implemented

| Way                                            | Fact or judgement                                                                                   |
| ---------------------------------------------- | --------------------------------------------------------------------------------------------------- |
| `fetchurl` with a hash in a derivation         | Needs a URL that pkgsmith does not know. Fixed-output only helps once a URL is chosen.                |
| `runCommand` with `${./files}`                 | Same as option 1 with a worse fixup story: no hooks of `stdenv`, and no `autoPatchelfHook`.           |
| Flake that points `src` at an absolute prefix and uses `nix build --impure` | Reads outside the flake. Not reproducible, and it is not a package that can be shipped. |
| `buildFHSEnv`                                  | Runs the binary in a bubblewrap root that looks like FHS. It needs user namespaces, builds an environment per program, and keeps the file layout the vendor chose. It is a wrapper, not a package of the files. |
| `programs.nix-ld` on NixOS                      | A system setting that provides a loader. A package cannot enable it, so it is documentation, not output. |
| `nix bundle`                                   | Converts a store path to a single file (an AppImage or a self-extracting executable). It consumes a package, so it can follow option 2. |
| Sign a cache with an ed25519 key                | Gives a trusted-publisher model on top of content addressing. Needs a key and the `Sig:` fingerprint. Not implemented, see below. |
| Submit to nixpkgs                               | A maintainer's work, with review. It is the opposite of generating files from a prefix.               |

### 6. Overlays and `callPackage` expressions

Facts: `package.nix` as a function of its dependencies is the form that nixpkgs uses for every package. A flake may expose
`overlays.default` so that `import nixpkgs { overlays = [ hi.overlays.default ]; }` has `pkgs.hi`.

- A single `default.nix` that takes `pkgs` is simple but cannot be overridden except by editing it.
- A `package.nix` function plus a flake and a `default.nix` that call it keeps one definition for flakes, `nix-build` and
  overlays. This is the shape that the rework writes.
- Cost: three small files in the archive instead of one.

### 7. Install scripts, configuration files, systemd units and `/etc`

Facts: a Nix package has no install or remove script, and nothing runs when it is added to a store. Its files are read-only.
A store path cannot contain a file that a user edits. The store cannot own `/etc`.

| Content of the prefix                | What Nix does                                                                                          | What the plugin does |
| ------------------------------------ | ------------------------------------------------------------------------------------------------------ | --------------------- |
| `pre_install`, `post_install`, ...    | Nothing. Equivalent work belongs in `postInstall`, in a NixOS module or in the program.                | Warns. The scripts are not written. |
| A configuration file in `/etc`         | On NixOS, `environment.etc."hi.conf".source = "${pkg}/etc/hi.conf"` links it. Elsewhere there is no mapping. | The file is `etc/hi.conf` in the store path. `config` flags are dropped with a warning. |
| A systemd unit in `/usr/lib/systemd`   | NixOS reads units from `systemd.packages`, and a unit is enabled by `systemd.services.NAME.wantedBy`. The unit must name store paths. | The unit is `lib/systemd/system/...`. `prefix/` in it is rewritten to `$out/` with `substituteInPlace --replace-fail`. Enabling is left to the user. |
| A `.desktop` file, a D-Bus service      | Found through `$out/share` by the profile and the session. They must name store paths.                   | The same rewrite. |
| A user or group, `tmpfiles.d`, `sysusers.d` | NixOS has `users.users`, `systemd.tmpfiles` and `systemd.sysusers`. A store path cannot create them.   | Files are copied as they are. Nothing creates the user. |
| Owner, group, setuid                   | Ignored. The store is owned by root and read-only. A setuid program needs `security.wrappers` on NixOS.  | Warns. |

Judgement: a `nixosModules.default` that enables units and links `/etc` could be generated from the same facts, but it is a
choice of option names and service defaults, and the units of a vendor differ widely. It was not written, and a unit that
is only copied into a store path stays inactive.

## Decision

- Keep the flake archive, now as option 2 with option 6, under the format name `nix`. This is the format that makes the
  files run on NixOS, so it is the main one.
- Add option 3 as `nix-cache` and option 4 as `nix-export`. They are cheap once a NAR exists, they give a store path that
  pkgsmith can print, and they are the right form for a machine that has a loader, such as Fedora or Debian with Nix.
- Do not implement the options of section 5.

### What is data and what is code

- The licenses, the systems of `all`, the `stdenv` choice, `mainProgram`, provenance and the unit rewrites come from
  nixpkgs functions or from the files of the package. The code holds no package name.
- The only fixed tables are the mapping from pkgsmith `arch` names to Nix CPU names, the default list of globs for
  `rewrite`, and the three-line shell of `installPhase`. All of them are documented, and the globs can be replaced.
- Every choice that the user makes is an option in a `#[serde(deny_unknown_fields)]` struct. An option of the wrong
  format is an error.

### Verified

See [formats](../formats.md#what-was-checked). In short: the flake, the cache and the export all work in a real Nix, a
dynamically linked program from Debian runs after patching, a content-addressed path is accepted without a signature, and
a wrong digest is refused.

## Open

- **Signed caches.** A `Sig:` line would add a publisher key. It needs an ed25519 implementation in a plugin and the
  fingerprint `1;STOREPATH;NARHASH;NARSIZE;REFERENCES`. Not done.
- **A lock file.** The flake has an unpinned `nixpkgs` input. pkgsmith cannot look up a revision without the network.
  `nixpkgs` accepts a pinned reference, and a user can run `nix flake lock`.
- **NixOS module.** See section 7.
- **Multi-user Nix.** `nix-store --import` and `nix copy` as an unprivileged user through the daemon are not checked. The
  content-addressed path should not need a signature there, but this was not run.
- **Nix on macOS and `aarch64`.** The system names are generated and unit-tested, and no such system was available.
- **Large prefixes.** The NAR of the whole package is built in memory before it is hashed and compressed, as the tar of
  the other formats is. A streaming writer would need a hash that the compression library can feed, and it was not
  needed.
- **Attribute names of nixpkgs.** They are written as the user gives them. A name that does not exist fails at the user's
  `nix build`, not in pkgsmith.
- **References by name.** `scan_references` reads a name from the text, up to the first character that a name cannot hold.
  A path followed by a dot, such as the end of a sentence, loses the dot. A false reference is possible. The option
  `scan_references = false` turns it off, and `references` lists candidates by hand.
