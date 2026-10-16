// SPDX-License-Identifier: GPL-3.0-only
// SPDX-FileCopyrightText: 2026 Curvy Protocol d.o.o.
//! Prints the witness a graph computes for a circom input file, as a JSON array of
//! decimal strings: `cargo run --example witness -- graph.bin input.json`.

#[path = "../tests/support/mod.rs"]
mod support;

fn main() -> eyre::Result<()> {
    let arguments: Vec<String> = std::env::args().collect();
    eyre::ensure!(
        arguments.len() == 3,
        "usage: witness <graph.bin> <input.json>"
    );
    let graph = curvy_signet_builder::init_graph(&std::fs::read(&arguments[1])?)?;
    let inputs = support::read_inputs(arguments[2].as_ref())?;
    let witness = curvy_signet_builder::calculate_witness(inputs, &graph)?;
    let decimals: Vec<String> = witness.iter().map(ToString::to_string).collect();
    println!("{}", serde_json::to_string(&decimals)?);
    Ok(())
}
