# Changelog

Versioning restarts at `0.1.0` for this fork. Upstream's history through `v0.2.3`
is in that project's changelog:
<https://github.com/philsippl/circom-witness-rs/blob/master/CHANGELOG.md>.

## 0.1.0-rc.1

### Fixed

- The cxx include paths still named the pre-fork package, so `0.1.0` could not be
  built with the `build-witness` feature at all. cxx resolves `include!` against
  the *package* name and symlinks `curvy-signet-builder` into
  `$OUT_DIR/cxxbridge/crate/`, so `circom-witness-rs/include/witness.h` pointed at
  nothing and every consumer enabling the feature failed in the C++ step with
  `fatal error: 'circom-witness-rs/include/witness.h' file not found`. Fixed in
  `src/generate.rs` and in the header `script/replace.sh` writes into the
  generated `src/circuit.cc`.

  Build-path only: a graph generated after this change is byte-identical to one
  generated before it, so nothing needs regenerating. `0.1.0` is yanked, since it
  cannot produce a graph at all.

## 0.1.0

Forked from upstream `v0.2.3` (`38553d2`) and published under a new name.

### Breaking, relative to upstream

- `Operation` gains `Bor` and `Bxor` between `Band` and `Neg`. postcard encodes a
  variant by its declaration index, so every index from 14 up shifts. A `graph.bin`
  written by upstream decodes incorrectly against this crate and vice versa, and it
  does so silently, because both remain valid indices. Consumers must track which
  build produced a file. `curvy-signet` models this as `--ops original|patched`.
- The library is `curvy_signet_builder`, not `circom_witness_rs`.

### Added

- `build_graph` binary, so an artifact pipeline can invoke the generator directly
  rather than through a library call:
  `WITNESS_CPP=<circuit.circom> cargo run --release --features build-witness --bin build_graph`.
- `Fr_bor` and `Fr_bxor`. The cxx bridge declared both but left them commented out,
  so generated C++ failed to link for any circuit using `|` or `^` with
  "use of undeclared identifier 'Fr_bxor'".
- wasm32 target support, via `getrandom`'s `js` backend.

### Changed

- Non-field operations evaluate in Circom's canonical integer representation
  instead of panicking. Montgomery evaluation previously refused everything outside
  the field arithmetic set, and `montgomery_form` refused to walk a graph containing
  one, which ruled out any circuit using comparisons, shifts or bitwise operators.
  Field operations are untouched and keep their fast path.
- `build_graph` is gated behind `required-features = ["build-witness"]`. Without it,
  a plain `cargo check` failed at the binary on an unresolved module.

### Removed

- The inherited GitHub workflows and `release-plz.toml`. The release job triggered
  on push to `master` with `contents` and `id-token` write permissions, which in a
  fork means every push attempts somebody else's release process. Its config had
  `git_release_enable` and `changelog_update` set, so leaving it behind would let a
  reinstated workflow, or a manual `release-plz` run, cut releases and rewrite this
  file.
