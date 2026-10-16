# curvy-signet-ir-builder

Builds witness graphs, in the encoding of the `curvy-signet-builder` crate in the parent directory,
from circom circuits. It compiles a circuit with circom 2.2.3 in-process and executes circom's
intermediate representation. No C++ toolchain is needed.

GPL-3.0-only (see `LICENSE`). Not published; the crate in the parent directory does not depend on it.

## Usage

```bash
./build-graph.sh circuit.circom -o graph.bin [options]   # from the repository root
cargo install --locked --path ir-builder                 # or install curvy-signet-ir-builder
```

| Option | |
|---|---|
| `-o, --output <file>` | The graph. Required. Replaced only by a complete graph; a failed build leaves no file. |
| `-l, --include <dir>` | Include directory, as `circom -l`. Repeatable. |
| `--r1cs <file>` | Also write the R1CS of this compilation. |
| `--expect-r1cs <file>` | Write the graph only if the compilation matches this R1CS: header, witness layout and constraints, in any order. Hints (`<--`) are not covered. |
| `--timings` | Print the time each phase took. |
| `-V, --version` | Print the version and the circom release. |

The circuit is compiled as `circom --O2` compiles it. No two of the circuit, `-o`, `--r1cs` and
`--expect-r1cs` may be the same file. circom's compilation summary goes to standard output; everything
else goes to standard error.

## Semantics

The graph computes circom's witness, including:

- `a / b` is 0 when `b` is 0;
- `a << b` and `a >> b` accept any amount, as circom does;
- `a \ b` and `a % b` with `b` equal to 0 fail at witness generation;
- `assert` and `===` are not checked.

A branch whose condition depends on an input runs both sides and selects the results. A read past the
end of the signals or variables on such a side fails at witness generation only where that side is
taken.

## Errors

The build fails on:

- a signal read before it is assigned, or assigned twice;
- a subcomponent input count that does not match circom's expectation;
- a component created but never run, or a witness signal never assigned;
- an input-dependent branch that creates components or returns, or that assigns a signal on one side
  only;
- an input-dependent branch in a function, array index or loop condition;
- `extern_c` templates.

## Tests

```bash
cargo test
CIRCOMLIB_INCLUDE=path/to/node_modules cargo test         # also the circomlib cases
scripts/check-against-circom.sh --all --circomlib path/to/node_modules
```

`cargo test` compares each fixture in `tests/fixtures/cases.json` with the witness circom computed
(`tests/fixtures/expected`). `scripts/check-against-circom.sh` recomputes those witnesses with circom
2.2.3 and node, and `--update-expected` rewrites them. It checks a single circuit as
`scripts/check-against-circom.sh circuit.circom input.json [include dir]...`.

circom's WASM witness calculator, the reference, does not run input-less components in a partly created
component array; do not add such a circuit to the fixtures.

## Releasing

After a version change in the parent crate, run `cargo update -p curvy-signet-builder` here.
