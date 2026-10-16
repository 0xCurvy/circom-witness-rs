// SPDX-License-Identifier: GPL-3.0-only
// SPDX-FileCopyrightText: 2026 Curvy Protocol d.o.o.
//! Compares the full witnesses two graphs compute for the same random inputs:
//! `cargo run --example compare -- a.bin b.bin [trials]`.
//!
//! Witness generation does not check constraints, so random inputs exercise every
//! node. An input that makes evaluation fail must make both graphs fail, and at
//! least one input must give both graphs a witness: a comparison in which every
//! trial failed proves nothing and fails too.

use std::collections::BTreeMap;

use curvy_signet_builder::{get_inputs_size, graph, init_graph, Graph, M};
use rand::Rng;
use ruint::aliases::U256;

fn evaluate(graph: &Graph, inputs: &[U256]) -> Result<Vec<U256>, String> {
    graph::evaluate(&graph.nodes, inputs, &graph.signals).map_err(|error| error.to_string())
}

/// An evaluation error without the node index, which differs between graphs.
fn reason(error: &str) -> String {
    error
        .split(" at graph node")
        .next()
        .unwrap_or(error)
        .to_owned()
}

fn main() -> eyre::Result<()> {
    let arguments: Vec<String> = std::env::args().collect();
    eyre::ensure!(
        (3..=4).contains(&arguments.len()),
        "usage: compare <a.bin> <b.bin> [trials]"
    );
    let a = init_graph(&std::fs::read(&arguments[1])?)?;
    let b = init_graph(&std::fs::read(&arguments[2])?)?;
    let trials: usize = arguments.get(3).map_or(Ok(20), |text| text.parse())?;

    let mapping = |graph: &Graph| {
        let mut entries: Vec<_> = graph
            .input_mapping
            .iter()
            .filter(|entry| entry.signalsize != 0)
            .map(|entry| (entry.hash, entry.signalid, entry.signalsize))
            .collect();
        entries.sort_unstable();
        entries
    };
    eyre::ensure!(
        mapping(&a) == mapping(&b),
        "the graphs map their inputs differently"
    );
    eyre::ensure!(
        a.signals.len() == b.signals.len(),
        "the witnesses have {} and {} signals",
        a.signals.len(),
        b.signals.len()
    );

    let size = get_inputs_size(&a).max(get_inputs_size(&b));
    let mut rng = rand::thread_rng();
    let mut failures = 0;
    let mut identical = 0;
    let mut both_failed: BTreeMap<String, usize> = BTreeMap::new();
    for trial in 0..trials {
        let mut inputs = vec![U256::ZERO; size];
        inputs[0] = U256::from(1);
        for value in inputs.iter_mut().skip(1) {
            // Mix full-width field elements with small values and bits, which reach
            // more branches of comparisons, bit decompositions and selects.
            *value = match trial % 3 {
                0 => rng.gen::<U256>() % M,
                1 => U256::from(rng.gen::<u8>() & 1),
                _ => U256::from(rng.gen::<u16>()),
            };
        }
        let (left, right) = (evaluate(&a, &inputs), evaluate(&b, &inputs));
        match (&left, &right) {
            (Ok(left), Ok(right)) if left == right => identical += 1,
            (Err(left), Err(right)) => {
                let key = if reason(left) == reason(right) {
                    reason(left)
                } else {
                    format!("{} / {}", reason(left), reason(right))
                };
                *both_failed.entry(key).or_default() += 1;
            }
            _ => {
                failures += 1;
                let first = match (&left, &right) {
                    (Ok(left), Ok(right)) => left
                        .iter()
                        .zip(right)
                        .position(|(x, y)| x != y)
                        .map(|index| format!("first difference at witness {index}"))
                        .unwrap_or_default(),
                    _ => format!("{:?} vs {:?}", left.as_ref().err(), right.as_ref().err()),
                };
                eprintln!("trial {trial}: witnesses differ ({first})");
            }
        }
    }
    let failed: usize = both_failed.values().sum();
    println!(
        "{trials} trials: {identical} identical, {failed} failed in both, {failures} mismatching; {} witness signals",
        a.signals.len()
    );
    for (reason, count) in &both_failed {
        println!("  failed in both ({count}): {reason}");
    }
    eyre::ensure!(failures == 0, "the graphs disagree");
    eyre::ensure!(
        identical > 0,
        "no trial gave both graphs a witness, so nothing was compared"
    );
    Ok(())
}
