# Curvy fork of `circom-witness-rs`

Branch `curvy/patched`, based on upstream `38553d2` (v0.2.3).

Used by [`curvy-signet`](https://github.com/0xCurvy/rs-core) to turn a Circom
circuit into a postcard `graph.bin`, which that tool then encodes into an
authenticated SIGNET artifact.

## Why a fork

The changes below were carried as a zero-context patch applied at build time.
That made every graph rebuild depend on this upstream repository staying
available under its current name, and `git apply --unidiff-zero` anchors hunks on
line numbers alone — the pin was doing all the work. A fork removes both problems
and gives a normal rebase path onto newer upstream.

## What changed

| commit | why |
|---|---|
| Add a `build_graph` binary | the generator was library-only |
| Evaluate non-field operations in canonical integer form | Montgomery evaluation panicked on comparisons, shifts and bitwise ops |
| Implement bitwise OR and XOR | the cxx bridge declared them but left them commented out |
| Build the evaluator for wasm32 | `getrandom` needs its `js` backend there |
| Gate `build_graph` behind its feature | plain `cargo check` failed at the bin |

## The one thing to know before consuming graphs from this fork

Adding `Bor` and `Bxor` inserts two variants into `Operation` between `Band` and
`Neg`. postcard encodes a variant by its **declaration index**, so every index
from 14 up shifts relative to upstream.

A `graph.bin` built before that change decodes incorrectly against this enum, and
one built after decodes incorrectly against upstream's — silently, because both
remain valid indices. Whatever consumes these files has to know which revision
produced them. `curvy-signet` models this explicitly as `--ops original|patched`.

## Building a graph

```bash
WITNESS_CPP=/abs/path/to/circuit.circom \
  cargo run --release --features build-witness --bin build_graph
```

Writes `graph.bin` in the working directory. The circuit must already have had
`patches/circomlib-iszero-bbf.patch` from `curvy-signet` applied to its circomlib
copy — that patch is circuit-side and does not live here.
