#!/usr/bin/env bash
# Checks the IR builder against circom itself.
#
#   scripts/check-against-circom.sh <circuit.circom> <input.json> [include dir]...
#   scripts/check-against-circom.sh --all [--circomlib <dir>] [--update-expected]
#
# For one case, the builder's compilation must describe the circuit `circom --O2`
# writes to its R1CS (`--expect-r1cs`), and the graph's witness must equal the one
# circom's WASM computes for the input.
#
# --all runs every case in tests/fixtures/cases.json: each witness case as above and
# against its file in tests/fixtures/expected, and each failing case, whose graph
# must refuse the input. Cases that need circomlib run only with --circomlib, a
# directory holding circomlib/ (such as a node_modules). --update-expected rewrites
# the expected files from circom's witnesses instead of comparing with them; the
# graph must still agree with circom.
#
# circom's WASM witness calculator is the reference, and it is wrong for one
# construct: in an array of components that is only partly created, it never runs
# the components that have no inputs (`z[1] = Z(); z[3] = Z();`), where circom's C++
# and the graph do. Such a circuit does not belong in the fixtures.
#
# Needs circom 2.2.3 and node; builds into ir-builder/target.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
FIXTURES="$ROOT/tests/fixtures"

test "$(circom --version)" = "circom compiler 2.2.3" || { echo "needs circom 2.2.3" >&2; exit 1; }
command -v node >/dev/null || { echo "needs node" >&2; exit 1; }
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$ROOT/target}"
cargo build --quiet --release --locked --manifest-path "$ROOT/Cargo.toml" \
  --bin curvy-signet-ir-builder --example witness
BUILDER="$CARGO_TARGET_DIR/release/curvy-signet-ir-builder"
WITNESS="$CARGO_TARGET_DIR/release/examples/witness"
UPDATE=0

# circom's witness for input $3, from the WASM in directory $1 named $2.
circom_witness() {
  node - "$1" "$2" "$3" <<'JS'
const fs = require("fs");
const [dir, name, input] = process.argv.slice(2);
require(`${dir}/witness_calculator.js`)(fs.readFileSync(`${dir}/${name}.wasm`)).then(async (calculator) => {
  const witness = await calculator.calculateWitness(JSON.parse(fs.readFileSync(input, "utf8")), 0);
  console.log(JSON.stringify(witness.map(String)));
});
JS
}

# Compiles circuit $1 with circom and the builder, includes from $3 onwards, into a
# work directory, and prints that directory. $2 names the work directory.
build() {
  local circuit="$1" work="$CARGO_TARGET_DIR/check/$2"
  shift 2
  local includes=()
  for dir in "$@"; do includes+=(-l "$dir"); done
  local name
  name="$(basename "$circuit" .circom)"
  rm -rf "$work" && mkdir -p "$work"
  circom "$circuit" --O2 --r1cs --wasm "${includes[@]+"${includes[@]}"}" -o "$work" > "$work/circom.log" 2>&1 \
    || { tail -20 "$work/circom.log" >&2; return 1; }
  "$BUILDER" "$circuit" "${includes[@]+"${includes[@]}"}" -o "$work/graph.bin" \
    --expect-r1cs "$work/$name.r1cs" > "$work/build.log" 2>&1 \
    || { tail -20 "$work/build.log" >&2; return 1; }
  echo "$work"
}

# Checks circuit $1 on input $2, includes from $3 onwards.
check() {
  local circuit input name work
  circuit="$(cd "$(dirname "$1")" && pwd)/$(basename "$1")"
  input="$(cd "$(dirname "$2")" && pwd)/$(basename "$2")"
  name="$(basename "$circuit" .circom)"
  shift 2
  work="$(build "$circuit" "$name-$(basename "$input" .json)" "$@")"
  circom_witness "$work/${name}_js" "$name" "$input" > "$work/circom-witness.json"
  "$WITNESS" "$work/graph.bin" "$input" > "$work/graph-witness.json"
  test -s "$work/circom-witness.json" && test -s "$work/graph-witness.json" \
    || { echo "$name $(basename "$input"): no witness" >&2; return 1; }
  if ! cmp -s "$work/circom-witness.json" "$work/graph-witness.json"; then
    echo "$name $(basename "$input"): witness differs" >&2
    echo "circom: $(cat "$work/circom-witness.json")" >&2
    echo "graph:  $(cat "$work/graph-witness.json")" >&2
    return 1
  fi
  local expected="$FIXTURES/expected/$(basename "$input")"
  if [[ "$UPDATE" == 1 ]]; then
    cp "$work/circom-witness.json" "$expected"
  elif [[ -f "$expected" ]] && ! cmp -s "$work/circom-witness.json" "$expected"; then
    echo "$name $(basename "$input"): circom's witness differs from $expected" >&2
    return 1
  fi
  echo "$name $(basename "$input"): same circuit as circom's R1CS, witness identical ($(tr ',' '\n' < "$work/graph-witness.json" | wc -l | tr -d ' ') signals)"
}

# Checks that the graph of circuit $1 refuses input $2.
check_fails() {
  local circuit="$1" input="$2" name work
  name="$(basename "$circuit" .circom)"
  work="$(build "$circuit" "$name-$(basename "$input" .json)")"
  if "$WITNESS" "$work/graph.bin" "$input" > /dev/null 2>&1; then
    echo "$name $(basename "$input"): the graph computed a witness" >&2
    return 1
  fi
  echo "$name $(basename "$input"): the graph refuses the input, as expected"
}

# One "<list> <circuit> <input> <needs>" line per case in cases.json.
cases() {
  node -e '
const cases = JSON.parse(require("fs").readFileSync(process.argv[1], "utf8"));
for (const list of ["witness", "fails"])
  for (const c of cases[list]) console.log([list, c.circuit, c.input, c.needs || "-"].join(" "));
' "$FIXTURES/cases.json"
}

if [[ "${1:-}" != --all ]]; then
  [[ $# -ge 2 ]] || { sed -n '2,5p' "$0" | sed 's/^# \{0,1\}//' >&2; exit 2; }
  check "$@"
  exit
fi
shift
CIRCOMLIB=""
while [[ $# -gt 0 ]]; do
  case "$1" in
    --circomlib) CIRCOMLIB="${2:?--circomlib needs a directory}"; shift 2 ;;
    --update-expected) UPDATE=1; shift ;;
    *) echo "unknown option $1" >&2; exit 2 ;;
  esac
done
ran=0
while read -r list circuit input needs; do
  includes=()
  if [[ "$needs" == circomlib ]]; then
    [[ -n "$CIRCOMLIB" ]] || { echo "skipped $circuit $input: needs --circomlib"; continue; }
    includes=("$CIRCOMLIB")
  fi
  if [[ "$list" == witness ]]; then
    check "$FIXTURES/circuits/$circuit" "$FIXTURES/inputs/$input" "${includes[@]+"${includes[@]}"}"
  else
    check_fails "$FIXTURES/circuits/$circuit" "$FIXTURES/inputs/$input"
  fi
  ran=$((ran + 1))
done < <(cases)
[[ "$ran" -gt 0 ]] || { echo "no case ran" >&2; exit 1; }
echo "$ran cases checked"
