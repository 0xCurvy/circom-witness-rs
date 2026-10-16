# curvy-signet-builder

Curvy's fork of [philsippl/circom-witness-rs](https://github.com/philsippl/circom-witness-rs), based on
upstream `v0.2.3` (`38553d2`). [README.upstream.md](README.upstream.md) is upstream's documentation and
describes upstream, not this fork.

This repository holds two packages:

- `curvy-signet-builder` (MIT): the postcard witness-graph format of SIGNET artifacts, its optimizer and
  its evaluator.
- [`curvy-signet-ir-builder`](ir-builder/README.md) in `ir-builder/` (GPL-3.0, not published): builds
  graphs from circom circuits. `curvy-signet-builder` does not depend on it.

## Building a graph

```bash
./build-graph.sh path/to/circuit.circom -o graph.bin --expect-r1cs path/to/proving_key.r1cs
```

Needs cargo; the first run compiles the builder. `./build-graph.sh --help` lists the options. To install
the builder on `PATH`:

```bash
cargo install --locked --path ir-builder
```

## Computing a witness

```rust
let graph = curvy_signet_builder::init_graph(&std::fs::read("graph.bin")?)?;
// Main inputs by name, values flattened: HashMap<String, Vec<U256>>.
let witness = curvy_signet_builder::calculate_witness(inputs, &graph)?;
```

## Graph encoding

`Operation` has `Bor` and `Bxor` between `Band` and `Neg`, so every operation index from 14 up differs
from upstream's. Graphs from upstream and from this fork do not decode with each other's types.
