# Changes to the `msi` crate

This folder is `msi` 0.11.1 from <https://github.com/mdsteele/rust-msi> (MIT, Copyright (c) 2017 Matthew D. Steele; see
`LICENSE`). The root `Cargo.toml` routes the crates.io `msi` here with `[patch.crates-io]`.

Changes to the upstream files:

- `Cargo.toml` is rewritten: no `ffi` workspace, no examples, no dev-dependencies of the examples, and two clippy lints
  allowed, so that `cargo clippy --workspace -- -D warnings` passes with the folder as a workspace member.
- **Long string references.** An MSI refers to a string with its number in the string pool: two bytes, so at most
  65,535 distinct strings. A database with more sets the top bit of the code page word in `_StringPool`, and then every
  string column of every table is three bytes wide. Upstream panics with "Too many strings; rewriting to long string
  refs is not yet supported".
  - `Package::insert_rows` and `update_rows` work out how many entries a query can add to the pool. When two bytes would
    no longer address the pool, `Package` reads every table, sets the flag, and writes every table again with the
    wider columns (`switch_to_long_string_refs` in `src/internal/package.rs`). Strings keep their numbers.
  - A database that stays below 65,535 strings is written as before, with two-byte references.
  - `Package::long_string_refs()` tells which form a package has.
- **A hash index in the string pool.** `StringPool::incref` scanned the whole pool for each string, which is quadratic:
  100,000 strings took minutes. The pool keeps a map from string to entry and the set of free entries, and picks the
  same entry the scan did.
- New tests: `tests/long_string_refs.rs` and two tests in `src/internal/stringpool.rs`.

The change is meant for upstream. Drop this folder and the `[patch.crates-io]` section when a released `msi` writes long
string references.
