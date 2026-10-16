// SPDX-License-Identifier: GPL-3.0-only
// SPDX-FileCopyrightText: 2026 Curvy Protocol d.o.o.
//! `curvy-signet-ir-builder [options] <circuit.circom>`: see `cli::USAGE`.

mod cli;

use std::process::ExitCode;

// circom's simplification allocates from several threads; the system allocator
// contends on it and keeps what it frees. mimalloc halves the resident size
// and shortens the build, without changing the graph.
#[global_allocator]
static ALLOC: mimalloc::MiMalloc = mimalloc::MiMalloc;

fn main() -> ExitCode {
    match cli::parse(std::env::args_os().skip(1)) {
        Ok(cli::Command::Build(args)) => match cli::run(args) {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("Error: {error:?}");
                ExitCode::FAILURE
            }
        },
        Ok(cli::Command::Help) => {
            println!("{}", cli::USAGE);
            ExitCode::SUCCESS
        }
        Ok(cli::Command::Version) => {
            let version = env!("CARGO_PKG_VERSION");
            let circom = curvy_signet_ir_builder::CIRCOM_VERSION;
            println!("curvy-signet-ir-builder {version} (circom {circom})");
            ExitCode::SUCCESS
        }
        Err(message) => {
            eprintln!("{message}\n\n{}", cli::USAGE);
            ExitCode::from(2)
        }
    }
}
