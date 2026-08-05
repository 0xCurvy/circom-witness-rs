# circom-witness-rs - Curvy fork

Fork of [philsippl/circom-witness-rs](https://github.com/philsippl/circom-witness-rs),
based on `v0.2.3` (`38553d2`). Upstream's documentation is preserved in
[README.upstream.md](README.upstream.md) and still describes the library.

Used by [`curvy-signet`](https://github.com/0xCurvy/rs-core) to turn a Circom
circuit into a postcard `graph.bin`, which that tool then encodes into an
authenticated SIGNET artifact.

## The one thing to know before consuming graphs from this fork

Adding `Bor` and `Bxor` inserts two variants into `Operation` between `Band` and
`Neg`. postcard encodes a variant by its **declaration index**, so every index from
14 up shifts relative to upstream.

A `graph.bin` built before that change decodes incorrectly against this enum, and
one built after decodes incorrectly against upstream's - silently, because both
remain valid indices. Whatever consumes these files has to know which revision
produced them. `curvy-signet` models it explicitly as `--ops original|patched`.

## Building a graph

```bash
WITNESS_CPP=/abs/path/to/circuit.circom \
  cargo run --release --features build-witness --bin build_graph
```

Writes `graph.bin` in the working directory. The circuit must already have had
`patches/circomlib-iszero-bbf.patch` from `curvy-signet` applied to its circomlib
copy - that patch is circuit-side and does not live here.

## Updating from upstream

```bash
git remote add upstream https://github.com/philsippl/circom-witness-rs.git
git fetch upstream --tags
git rebase <new-upstream-tag>
```

Seven commits to replay. Rebuild every published graph afterwards and check it with
`signet validate` before repinning anything: the operation set and its ordering are
exactly what a rebase can disturb, and a wrong graph parses and evaluates rather
than failing.