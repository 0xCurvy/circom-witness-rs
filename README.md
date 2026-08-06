# curvy-signet-builder

Curvy's fork of [philsippl/circom-witness-rs](https://github.com/philsippl/circom-witness-rs). Based on upstream
`v0.2.3` (`38553d2`); versioning restarts at `0.1.0`. Upstream's documentation is preserved in
[README.upstream.md](README.upstream.md) and still describes the library.

## The one thing to know before consuming graphs from this fork

Adding `Bor` and `Bxor` inserts two variants into `Operation` between `Band` and
`Neg`. postcard encodes a variant by its **declaration index**, so every index from
14 up shifts relative to upstream.

A `graph.bin` built before that change decodes incorrectly against this enum, and
one built after decodes incorrectly against upstream's - silently, because both
remain valid indices. Whatever consumes these files has to know which revision
produced them.

## Building a graph

```bash
WITNESS_CPP=/abs/path/to/circuit.circom \
  cargo run --release --features build-witness --bin build_graph
```