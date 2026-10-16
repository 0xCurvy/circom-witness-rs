// SPDX-License-Identifier: GPL-3.0-only
// SPDX-FileCopyrightText: 2026 Curvy Protocol d.o.o.
//! Builds the fixtures listed in `tests/fixtures/cases.json` and checks each graph's
//! witness against the witness circom 2.2.3 computed for the same input, saved in
//! `tests/fixtures/expected` by `scripts/check-against-circom.sh --update-expected`.
//!
//! Cases that need circomlib run when `CIRCOMLIB_INCLUDE` names a directory holding
//! `circomlib/`, such as a `node_modules`.

mod support;

use std::path::PathBuf;

use curvy_signet_builder::{calculate_witness, init_graph, Graph};
use curvy_signet_ir_builder::{build_graph, BuildOptions};

fn options(needs: Option<&str>) -> Option<BuildOptions> {
    match needs {
        None => Some(BuildOptions::new()),
        Some("circomlib") => std::env::var_os("CIRCOMLIB_INCLUDE")
            .map(|include| BuildOptions::new().include_path(PathBuf::from(include))),
        Some(other) => panic!("unknown requirement {other}"),
    }
}

fn graph(circuit: &str, options: &BuildOptions) -> Graph {
    let built = build_graph(&support::fixtures().join("circuits").join(circuit), options)
        .unwrap_or_else(|error| panic!("{circuit} does not build: {error:?}"));
    init_graph(&built.to_bytes().unwrap()).unwrap()
}

#[test]
fn witnesses_match_circom() {
    let mut checked = 0;
    for case in support::cases("witness") {
        let Some(options) = options(case.needs.as_deref()) else {
            continue;
        };
        let input = case.input.expect("a witness case names its input");
        let inputs =
            support::read_inputs(&support::fixtures().join("inputs").join(&input)).unwrap();
        let witness: Vec<String> = calculate_witness(inputs, &graph(&case.circuit, &options))
            .unwrap_or_else(|error| panic!("{} on {input}: {error:?}", case.circuit))
            .iter()
            .map(ToString::to_string)
            .collect();
        let expected: Vec<String> = serde_json::from_slice(
            &std::fs::read(support::fixtures().join("expected").join(&input))
                .unwrap_or_else(|error| panic!("no expected witness for {input}: {error}")),
        )
        .unwrap();
        assert_eq!(witness, expected, "{} on {input}", case.circuit);
        checked += 1;
    }
    assert!(checked > 0, "no witness case ran");
}

#[test]
fn failures_happen_at_witness_time_where_circom_reads_outside_memory() {
    for case in support::cases("fails") {
        let input = case.input.expect("a failing case names its input");
        let inputs =
            support::read_inputs(&support::fixtures().join("inputs").join(&input)).unwrap();
        let graph = graph(&case.circuit, &options(case.needs.as_deref()).unwrap());
        assert!(
            calculate_witness(inputs, &graph).is_err(),
            "{} on {input} computed a witness",
            case.circuit
        );
    }
}

#[test]
fn unsupported_circuits_are_rejected() {
    for case in support::cases("rejected") {
        let reason = case.reason.expect("a rejected case names its reason");
        let error = build_graph(
            &support::fixtures().join("rejected").join(&case.circuit),
            &BuildOptions::new(),
        )
        .err()
        .unwrap_or_else(|| panic!("{} built", case.circuit));
        let chain = format!("{error:?}");
        assert!(chain.contains(&reason), "{}: {chain}", case.circuit);
    }
}

#[test]
fn builds_are_reproducible() {
    let build = || {
        build_graph(
            &support::fixtures().join("circuits/edge.circom"),
            &BuildOptions::new(),
        )
        .unwrap()
        .to_bytes()
        .unwrap()
    };
    assert_eq!(build(), build());
}
