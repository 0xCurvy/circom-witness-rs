// SPDX-License-Identifier: GPL-3.0-only
// SPDX-FileCopyrightText: 2026 Curvy Protocol d.o.o.
//! Compiles a circuit in-process the way `circom --O2` does.

use std::path::{Path, PathBuf};

use circom_compiler::compiler_interface::{self, Circuit, Config};
use circom_constraint_generation::{build_circuit, BuildConfig};
use circom_program_structure::error_definition::Report;
use circom_program_structure::utils::constants::UsefulConstants;
use curvy_signet_builder::M;
use eyre::{bail, eyre, Result};
use ruint::aliases::U256;

/// The circom release whose front end compiles the circuit.
pub const CIRCOM_VERSION: &str = "2.2.3";

/// circom's default `--sanity_check` level. At this level the code circom generates
/// counts every subcomponent input write.
const SANITY_CHECK_STYLE: usize = 2;

/// Parses, type-checks and compiles the circuit the way `circom --O2` does, writing
/// the R1CS of this compilation to `r1cs_path` when one is given.
pub(crate) fn compile(
    circuit: &Path,
    include_paths: &[PathBuf],
    r1cs_path: Option<&Path>,
) -> Result<Circuit> {
    let prime = UsefulConstants::new(&"bn128".to_string()).get_p().clone();
    let (mut program, warnings) = circom_parser::run_parser(
        circuit.to_string_lossy().into_owned(),
        CIRCOM_VERSION,
        include_paths.to_vec(),
        &prime,
        false,
    )
    .map_err(|(library, reports)| {
        Report::print_reports(&reports, &library);
        eyre!("circom could not parse {}", circuit.display())
    })?;
    Report::print_reports(&warnings, &program.file_library);
    match circom_type_analysis::check_types::check_types(&mut program) {
        Ok(warnings) => Report::print_reports(&warnings, &program.file_library),
        Err(errors) => {
            Report::print_reports(&errors, &program.file_library);
            bail!("circom type analysis rejected {}", circuit.display());
        }
    }

    let custom_gates = program.custom_gates;
    let (exporter, vcp) = build_circuit(
        program,
        BuildConfig {
            no_rounds: usize::MAX,
            flag_json_sub: false,
            json_substitutions: String::new(),
            flag_s: false,
            flag_f: false,
            flag_p: false,
            flag_verbose: false,
            flag_old_heuristics: false,
            inspect_constraints: false,
            prime: "bn128".to_owned(),
        },
    )
    .map_err(|()| {
        eyre!(
            "circom constraint generation failed for {}",
            circuit.display()
        )
    })?;
    if let Some(path) = r1cs_path {
        exporter
            .r1cs(&path.to_string_lossy(), custom_gates)
            .map_err(|()| eyre!("could not write the R1CS to {}", path.display()))?;
    }

    compiler_interface::run_compiler(
        vcp,
        Config {
            debug_output: false,
            produce_input_log: false,
            wat_flag: false,
            sanity_check_style: SANITY_CHECK_STYLE,
            no_asm_flag: false,
        },
        CIRCOM_VERSION,
    )
    .map_err(|()| eyre!("circom could not compile {}", circuit.display()))
}

/// A decimal entry of circom's constant table, reduced into the field.
pub(crate) fn parse_field_constant(value: &str) -> Result<U256> {
    let (negative, digits) = match value.strip_prefix('-') {
        Some(digits) => (true, digits),
        None => (false, value),
    };
    let magnitude = digits
        .parse::<U256>()
        .map_err(|_| eyre!("invalid circom constant {value}"))?
        % M;
    Ok(if negative && magnitude != U256::ZERO {
        M - magnitude
    } else {
        magnitude
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constants_reduce_into_the_field() {
        assert_eq!(parse_field_constant("5").unwrap(), U256::from(5));
        assert_eq!(parse_field_constant("-1").unwrap(), M - U256::from(1));
        assert_eq!(parse_field_constant("-0").unwrap(), U256::ZERO);
        assert_eq!(parse_field_constant(&M.to_string()).unwrap(), U256::ZERO);
        assert!(parse_field_constant("x").is_err());
    }
}
