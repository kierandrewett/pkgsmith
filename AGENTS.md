# pkgsmith: notes for agents and contributors

pkgsmith builds distribution packages from one script. The host runs the script and runs the builds. Every package
format and every signing method is a plugin: a shared library named `pkgsmith-plugin-*.so` that the host loads.

## Layout

| Path                      | What it is                                                                       |
| ------------------------- | -------------------------------------------------------------------------------- |
| `src/`                    | The host: script, targets, containers, progress, plugin loader, command line     |
| `crates/pkgsmith-plugin/` | The SDK: the `Package` type, the C interface, the `Plugin` trait, shared helpers |
| `plugins/*/`              | The built-in plugins. One `cdylib` crate each. `plugins/*` joins the workspace by itself |
| `examples/`               | Complete scripts, and a C and a Rust example plugin in `examples/plugins/`     |
| `docs/`                   | The documentation. `README.md` links to every page                               |
| `pkgsmith.star`           | The script that pkgsmith uses to package itself                                  |

## Change the code, change the docs

Update the documentation in the same change as the code. A change is not finished until this list is done.

1. A new or changed **format plugin**: add or update its row in the format table of `README.md`, its options in
   `docs/plugins.md` (the table of format options), and an example in `examples/` when it helps.
2. A new or changed **signer plugin** or signing option: update `docs/signing.md` and `docs/cli.md`.
3. A new or changed **command line option or command**: update `docs/cli.md`, and the command table in `README.md`.
4. A new or changed **script function or field**: update `docs/manifest.md`, `docs/scripting.md` when it changes how a
   script is written, and `examples/full.star`, which uses every field.
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

A format is not done until the real package manager or tool of that format accepts the package. The test suite does this:

```sh
cargo build --workspace
scripts/verify-formats.sh target/debug/pkgsmith           # every format
scripts/verify-formats.sh target/debug/pkgsmith rpm deb   # some formats
scripts/verify-signing.sh target/debug/pkgsmith           # signatures
```

Each script builds a small package, installs or inspects it with the tool of the format in Podman, and prints `ok`,
`FAIL` or `skip` for every check. A run writes `verify-formats.log` with the digest of every image.

- The suite pulls the newest release of every distro image on each run. Do not pin an image in the script: the suite is
  there to catch what a new release breaks. Use `IMAGE_FEDORA=fedora:43` and the like to reproduce one fixed release.
- A new format plugin gets its checks in `scripts/verify-formats.sh` in the same change. Build, install, run, remove, and
  check the package manager's own verification (`rpm -V`, `dpkg -V`, `pacman -Qkk`).
- Say which checks could not run here (snap needs snapd, FreeBSD needs FreeBSD) in the commit and in the docs. Never claim
  a check that did not run.
- `.github/workflows/formats.yml` runs both scripts every day. A failure opens a GitHub issue with the label
  `format-regression`, assigned to the maintainer, and later failures comment on it. A passing run closes it.

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
- Format options are a typed struct with `#[serde(deny_unknown_fields)]`, so a typo in the script is an error.
- A tool or library that only one plugin needs is a dependency of that plugin, not of the SDK or the host.
- Keep secrets out of the repository, the script and the command line. Key files and passphrase files are paths.
- Commit in small steps with a conventional subject (`feat(scope): ...`). The body says what changed, why, how it was
  checked and what is left. Push after each commit.
