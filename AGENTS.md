# pkgsmith: notes for agents and contributors

pkgsmith builds distribution packages from one manifest. The host reads the manifest and runs the builds. Every package
format and every signing method is a plugin: a shared library named `pkgsmith-plugin-*.so` that the host loads.

## Layout

| Path                      | What it is                                                                       |
| ------------------------- | -------------------------------------------------------------------------------- |
| `src/`                    | The host: manifest, matrix, containers, progress, plugin loader, command line    |
| `crates/pkgsmith-plugin/` | The SDK: the `Package` type, the C interface, the `Plugin` trait, shared helpers |
| `plugins/*/`              | The built-in plugins. One `cdylib` crate each. `plugins/*` joins the workspace by itself |
| `examples/`               | Complete manifests, and a C and a Rust example plugin in `examples/plugins/`     |
| `docs/`                   | The documentation. `README.md` links to every page                               |
| `pkgsmith.yaml`           | The manifest that pkgsmith uses to package itself                                |

## Change the code, change the docs

Update the documentation in the same change as the code. A change is not finished until this list is done.

1. A new or changed **format plugin**: add or update its row in the format table of `README.md`, its options in
   `docs/plugins.md` (the table of format options), and an example in `examples/` when it helps.
2. A new or changed **signer plugin** or signing option: update `docs/signing.md` and `docs/cli.md`.
3. A new or changed **command line option or command**: update `docs/cli.md`, and the command table in `README.md`.
4. A new or changed **manifest key**: update `docs/manifest.md`, and `examples/full.yaml`, which uses every key.
5. A change to the **plugin interface** (`crates/pkgsmith-plugin/src/abi.rs` or `plugin.rs`): update the C interface in
   `docs/plugins.md` and the comment at the top of `abi.rs`, and rebuild `examples/plugins/c-listing`.
6. Check every link and command you wrote against the current code and `--help` output. Do not document a feature that
   you did not run.

## Writing Markdown

- One idea per paragraph, under about 60 words. Use a list or a table for options, values and steps.
- For each option, give its type or accepted values, its default and its effect. List every value of an enum.
- Start with a small working example, then the details. Do not explain a table again in prose.
- Link text says what the link opens. Check every link and command against the current code and `--help`.

## Verify with the real tool

A format is not done until the real package manager or tool of that format accepts the package. Build the package, then
install or inspect it with that tool, usually in a container. Examples that exist in this repository:

- rpm: `rpm -Kv` and `rpm -i` in Fedora. deb: `dpkg -i` and `dpkg -V` in Debian.
- Arch: `pacman -U` and `pacman -Qkk`. apk: `apk add` in Alpine.
- Slackware: `installpkg`, `upgradepkg` and `removepkg` in the Slackware image.
- OCI: `podman load` and `podman run`.

Say in the commit message and in the docs what was checked and what was not. If the real tool cannot run here (snap needs
snapd, FreeBSD needs FreeBSD), say that plainly. Never claim a check that did not run.

## Build and test

```sh
cargo build --workspace
cargo clippy --workspace --all-targets
cargo test --workspace
```

`cargo build --workspace` puts the plugins next to the `pkgsmith` binary, where the host finds them. Run
`pkgsmith plugins` to see what loaded.

## Rules for code

- Plain Rust. No `unsafe` outside the C interface (`abi.rs`, `plugin.rs`, `src/plugins.rs`), and give every `unsafe`
  block a `SAFETY` comment.
- A plugin turns bad input into an `Err` with a clear message. It never panics on user input.
- Format options are a typed struct with `#[serde(deny_unknown_fields)]`, so a typo in the manifest is an error.
- A tool or library that only one plugin needs is a dependency of that plugin, not of the SDK or the host.
- Keep secrets out of the repository, the manifest and the command line. Key files and passphrase files are paths.
- Commit in small steps with a conventional subject (`feat(scope): ...`). The body says what changed, why, how it was
  checked and what is left. Push after each commit.
