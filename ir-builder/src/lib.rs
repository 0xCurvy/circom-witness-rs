// SPDX-License-Identifier: GPL-3.0-only
// SPDX-FileCopyrightText: 2026 Curvy Protocol d.o.o.
//! Builds a circom witness graph by executing circom's intermediate representation
//! symbolically.
//!
//! The circuit is compiled in-process by circom 2.2.3's own front end with the
//! settings `circom --O2` uses, so the witness layout is the one that command
//! writes into the R1CS. Every template and function body then runs once:
//! values known at build time are folded, and every operation on a value derived
//! from an input becomes a graph node. The result is optimized and encoded with
//! `curvy-signet-builder`, the graph format SIGNET artifacts are made from.
//!
//! The executor follows the C++ that circom generates for the same circuit and
//! fails closed where that C++ would read uninitialized memory or rely on an
//! invariant without checking it:
//!
//! - reading a signal that was never assigned, or assigning one twice, is an error;
//! - every write to a subcomponent's inputs is counted, as circom's default sanity
//!   check level does, and a component runs exactly when its count reaches zero;
//! - a component that was created but never ran, and a witness signal that was never
//!   assigned, are errors.
//!
//! Control flow must be known at build time, with one exception: a branch in a
//! template whose condition depends on an input, such as circomlib's
//! `inv <-- in != 0 ? 1 / in : 0`. Both sides are executed and their results
//! selected with the condition. Field operations follow circom's runtime, where `/`
//! and the shifts never fail (a zero divisor gives 0, and every shift amount has a
//! result), so the unselected side only computes values nobody uses. Where circom
//! aborts, on `\` or `%` by zero, the divisor is 1 on the path the side is not taken,
//! and a read past the end of the variables or signals makes witness generation fail
//! only where its side is taken.

mod compile;
mod executor;
#[doc(hidden)]
pub mod files;
mod nodes;
pub mod r1cs;

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use circom_code_producers::c_elements::c_code_generator::generate_hash_map;
use curvy_signet_builder::graph::{self, Node};
use curvy_signet_builder::HashSignalInfo;
use eyre::{bail, Result, WrapErr};

pub use compile::CIRCOM_VERSION;

use files::{same_file, ScratchFile};

/// A graph in the encoding `curvy-signet-builder` reads and writes.
pub struct BuiltGraph {
    pub nodes: Vec<Node>,
    pub signals: Vec<usize>,
    pub input_mapping: Vec<HashSignalInfo>,
    /// Where the build spent its time.
    pub timings: Timings,
}

impl BuiltGraph {
    /// The postcard bytes of `(nodes, signals, input_mapping)`, as
    /// `curvy_signet_builder::init_graph` reads them.
    pub fn to_bytes(&self) -> Result<Vec<u8>> {
        Ok(postcard::to_stdvec(&(
            &self.nodes,
            &self.signals,
            &self.input_mapping,
        ))?)
    }
}

/// Time spent in each phase of a build.
#[derive(Debug, Clone, Copy, Default)]
pub struct Timings {
    /// Parsing, constraint generation and simplification, and circom's compiler.
    pub compile: Duration,
    /// Comparing this compilation's R1CS with the expected one.
    pub check: Duration,
    /// Executing the intermediate representation.
    pub execute: Duration,
    /// Optimizing the graph.
    pub optimize: Duration,
}

/// Where to look for includes, and what to do with the R1CS of the compilation.
#[derive(Debug, Clone, Default)]
#[non_exhaustive]
pub struct BuildOptions {
    /// Directories searched for includes, as `circom -l` searches them.
    pub include_paths: Vec<PathBuf>,
    /// Where to keep the R1CS of this compilation.
    pub r1cs_path: Option<PathBuf>,
    /// The R1CS a proving key was made from. The build fails unless this compilation
    /// describes the same circuit: the same header, the same wire-to-label map (the
    /// witness layout) and the same set of constraints. Hints (`<--`) have no
    /// constraints, so this does not cover the values they compute.
    pub expected_r1cs: Option<PathBuf>,
}

impl BuildOptions {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn include_path(mut self, path: impl Into<PathBuf>) -> Self {
        self.include_paths.push(path.into());
        self
    }

    pub fn r1cs_path(mut self, path: impl Into<PathBuf>) -> Self {
        self.r1cs_path = Some(path.into());
        self
    }

    pub fn expected_r1cs(mut self, path: impl Into<PathBuf>) -> Self {
        self.expected_r1cs = Some(path.into());
        self
    }
}

/// Compiles `circuit` and builds its optimized witness graph.
pub fn build_graph(circuit: &Path, options: &BuildOptions) -> Result<BuiltGraph> {
    let mut timings = Timings::default();
    if let Some(r1cs) = &options.r1cs_path {
        if same_file(r1cs, circuit) {
            bail!("the R1CS would overwrite the circuit {}", circuit.display());
        }
        if matches!(&options.expected_r1cs, Some(expected) if same_file(r1cs, expected)) {
            bail!(
                "--r1cs and --expect-r1cs name the same file {}: the compilation would overwrite the R1CS it is checked against",
                r1cs.display()
            );
        }
    }
    // Read before compiling: a missing file fails fast, and the bytes checked are the
    // bytes read here.
    let expected = match &options.expected_r1cs {
        Some(path) => Some(
            std::fs::read(path).wrap_err_with(|| format!("could not read {}", path.display()))?,
        ),
        None => None,
    };
    let scratch = match (&expected, &options.r1cs_path) {
        (Some(_), None) => Some(ScratchFile::create(
            &std::env::temp_dir(),
            "curvy-signet-ir-builder",
            "r1cs",
        )?),
        _ => None,
    };
    let r1cs_path = options
        .r1cs_path
        .as_deref()
        .or(scratch.as_ref().map(ScratchFile::path));

    let started = Instant::now();
    let circuit = compile::compile(circuit, &options.include_paths, r1cs_path)?;
    timings.compile = started.elapsed();

    if let (Some(expected), Some(path)) = (&expected, r1cs_path) {
        let started = Instant::now();
        let compiled =
            std::fs::read(path).wrap_err_with(|| format!("could not read {}", path.display()))?;
        r1cs::ensure_equivalent(&compiled, expected).wrap_err_with(|| {
            format!(
                "{} does not describe the compiled circuit",
                options.expected_r1cs.as_deref().unwrap_or(path).display()
            )
        })?;
        timings.check = started.elapsed();
    }
    drop(scratch);

    let started = Instant::now();
    let (mut nodes, mut signals) = executor::execute(&circuit)?;
    timings.execute = started.elapsed();
    let started = Instant::now();
    graph::optimize(&mut nodes, &mut signals);
    timings.optimize = started.elapsed();

    let producer = &circuit.c_producer;
    let input_mapping = generate_hash_map(
        producer.get_main_input_list(),
        producer.get_input_hash_map_entry_size(),
    )
    .into_iter()
    .map(|(hash, signalid, signalsize)| HashSignalInfo {
        hash,
        signalid,
        signalsize,
    })
    .collect();
    Ok(BuiltGraph {
        nodes,
        signals,
        input_mapping,
        timings,
    })
}
