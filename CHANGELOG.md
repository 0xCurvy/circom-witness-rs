# Changelog

Versioning restarts at `0.1.0` for this fork. Upstream's history through `v0.2.3` is in
[its changelog](https://github.com/philsippl/circom-witness-rs/blob/master/CHANGELOG.md).

## 0.1.2

### Breaking

- Removed the C++ path: the `build-witness` feature, the `build_graph` binary and the `generate` module.
- Removed black-box functions: `Node::BBF` and `BlackBoxFunction`.
- `calculate_witness` and `graph::evaluate` no longer take black-box functions.

### Added

- `ir-builder/`: `curvy-signet-ir-builder`, a separate GPL-3.0 program that builds graphs from circom
  circuits. Not part of the published package.
- `build-graph.sh`, which runs it.
- `Operation::checked_eval`.

### Changed

- `graph::optimize` is about three times faster and produces the same graphs.

### Fixed

- `get_inputs_size` covers trailing inputs that no node reads.
- `graph::evaluate` returns an error instead of panicking on a division or modulus by zero, the inverse
  of zero or a shift amount of 256 or more, and on an input outside the buffer (was read as `U256::MAX`).
- `graph::optimize` no longer panics on a constant zero divisor or a constant shift amount of 256 or
  more.
- `calculate_witness` returns an error instead of panicking on an unknown input name, an input value
  longer than declared or an impossible input buffer size.

## 0.1.0-rc.1

### Fixed

- The `build-witness` feature builds; the cxx include paths named the pre-fork package. `0.1.0` is
  yanked.

## 0.1.0

Forked from upstream `v0.2.3` (`38553d2`).

### Breaking, relative to upstream

- `Operation` gains `Bor` and `Bxor` between `Band` and `Neg`, which shifts every encoded operation index
  from 14 up. Graphs from upstream and from this crate do not decode with each other's types.
- The library is `curvy_signet_builder`, not `circom_witness_rs`.

### Added

- `build_graph` binary.
- `Fr_bor` and `Fr_bxor` in the cxx bridge.
- wasm32 support, through `getrandom`'s `js` backend.

### Changed

- Non-field operations evaluate in circom's integer representation instead of panicking.
- `build_graph` requires the `build-witness` feature.

### Removed

- The inherited GitHub workflows and `release-plz.toml`.
