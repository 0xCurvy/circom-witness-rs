#!/usr/bin/env bash
# Builds a witness graph from a circom circuit with curvy-signet-ir-builder.
#
#   ./build-graph.sh circuit.circom -o graph.bin [-l include_dir]... \
#     [--expect-r1cs proving_key.r1cs] [--r1cs circuit.r1cs] [--timings]
#
# The arguments are the builder's own (`./build-graph.sh --help` lists them), and
# paths are relative to where you run it. The first run compiles the builder, which
# takes a minute or two (mostly circom's crates); later runs start at once. Needs
# cargo; checking against `--expect-r1cs` needs nothing else.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")" && pwd)"
cargo build --quiet --release --locked --manifest-path "$ROOT/ir-builder/Cargo.toml" \
  --bin curvy-signet-ir-builder
exec "${CARGO_TARGET_DIR:-$ROOT/ir-builder/target}/release/curvy-signet-ir-builder" "$@"
