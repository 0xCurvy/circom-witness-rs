// SPDX-License-Identifier: GPL-3.0-only
// SPDX-FileCopyrightText: 2026 Curvy Protocol d.o.o.
//! Command-line parsing and the build the program runs.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use curvy_signet_ir_builder::files::{same_file, ScratchFile};
use curvy_signet_ir_builder::{build_graph, BuildOptions};
use eyre::{bail, Result, WrapErr};

pub const USAGE: &str = "\
usage: curvy-signet-ir-builder [options] <circuit.circom>

  -o, --output <graph.bin>        where to write the graph (required)
  -l, --include <dir>             search <dir> for includes, as circom -l (repeatable)
      --r1cs <circuit.r1cs>       keep the R1CS of this compilation
      --expect-r1cs <key.r1cs>    write the graph only if this compilation describes
                                  the circuit in <key.r1cs>: the same header, witness
                                  layout and constraints (hints are not covered)
      --timings                   print the time each phase took
  -h, --help                      print this help
  -V, --version                   print the version and the circom release it uses";

#[derive(Debug, PartialEq, Eq)]
pub struct Args {
    pub circuit: PathBuf,
    pub output: PathBuf,
    pub include_paths: Vec<PathBuf>,
    pub r1cs: Option<PathBuf>,
    pub expected_r1cs: Option<PathBuf>,
    pub timings: bool,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Command {
    Build(Args),
    Help,
    Version,
}

/// Parses the arguments after the program name. Options take their value as the
/// next argument; `--` ends the options.
pub fn parse(arguments: impl IntoIterator<Item = OsString>) -> Result<Command, String> {
    let mut arguments = arguments.into_iter();
    let mut circuit = None;
    let mut output = None;
    let mut include_paths = Vec::new();
    let mut r1cs = None;
    let mut expected_r1cs = None;
    let mut timings = false;
    let mut options_ended = false;
    while let Some(argument) = arguments.next() {
        let text = argument.to_string_lossy().into_owned();
        if options_ended || !text.starts_with('-') || text == "-" {
            if circuit.replace(PathBuf::from(&argument)).is_some() {
                return Err(format!(
                    "unexpected argument {text}: only one circuit is built"
                ));
            }
            continue;
        }
        let mut value = || {
            arguments
                .next()
                .map(PathBuf::from)
                .ok_or_else(|| format!("{text} needs a value"))
        };
        let single = |slot: &mut Option<PathBuf>, value: PathBuf| {
            if slot.replace(value).is_some() {
                return Err(format!("{text} is given twice"));
            }
            Ok(())
        };
        match text.as_str() {
            "-o" | "--output" => single(&mut output, value()?)?,
            "-l" | "--include" => include_paths.push(value()?),
            "--r1cs" => single(&mut r1cs, value()?)?,
            "--expect-r1cs" => single(&mut expected_r1cs, value()?)?,
            "--timings" => timings = true,
            "-h" | "--help" => return Ok(Command::Help),
            "-V" | "--version" => return Ok(Command::Version),
            "--" => options_ended = true,
            _ => return Err(format!("unknown option {text}")),
        }
    }
    Ok(Command::Build(Args {
        circuit: circuit.ok_or("no circuit given")?,
        output: output.ok_or("no output given: pass -o <graph.bin>")?,
        include_paths,
        r1cs,
        expected_r1cs,
        timings,
    }))
}

/// Builds the graph and writes it to `args.output`. A file there is replaced only by
/// a complete graph: a failed build leaves no file, not even an older graph.
pub fn run(args: Args) -> Result<()> {
    let inputs = [
        Some(&args.circuit),
        args.r1cs.as_ref(),
        args.expected_r1cs.as_ref(),
    ];
    for input in inputs.into_iter().flatten() {
        if same_file(&args.output, input) {
            bail!(
                "the output {} would overwrite {}",
                args.output.display(),
                input.display()
            );
        }
    }
    // A device such as /dev/null is written in place; a file is replaced only by a
    // complete graph, through a scratch file created first so that an unwritable
    // directory fails before the build.
    let device = std::fs::metadata(&args.output).is_ok_and(|metadata| !metadata.is_file());
    let partial = if device {
        None
    } else {
        let directory = match args.output.parent() {
            Some(parent) if !parent.as_os_str().is_empty() => parent,
            _ => Path::new("."),
        };
        let name = args
            .output
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| "graph".to_owned());
        let partial = ScratchFile::create(directory, &name, "partial")?;
        if args.output.exists() {
            std::fs::remove_file(&args.output)
                .wrap_err_with(|| format!("could not remove the old {}", args.output.display()))?;
        }
        Some(partial)
    };

    let mut options = BuildOptions::new();
    options.include_paths = args.include_paths;
    options.r1cs_path = args.r1cs;
    options.expected_r1cs = args.expected_r1cs.clone();
    let started = std::time::Instant::now();
    let graph = build_graph(&args.circuit, &options)?;
    if let Some(expected) = &args.expected_r1cs {
        eprintln!("matches {}", expected.display());
    }
    let bytes = graph.to_bytes()?;
    let target = partial
        .as_ref()
        .map_or(args.output.as_path(), ScratchFile::path);
    std::fs::write(target, &bytes)
        .wrap_err_with(|| format!("could not write {}", target.display()))?;
    if let Some(partial) = partial {
        partial.persist(&args.output)?;
    }
    eprintln!(
        "{}: {} nodes, {} witness signals, {} bytes in {:.1?}",
        args.output.display(),
        graph.nodes.len(),
        graph.signals.len(),
        bytes.len(),
        started.elapsed()
    );
    if args.timings {
        let timings = graph.timings;
        eprintln!(
            "compile {:.2?}, check {:.2?}, execute {:.2?}, optimize {:.2?}",
            timings.compile, timings.check, timings.execute, timings.optimize
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_strs(arguments: &[&str]) -> Result<Command, String> {
        parse(arguments.iter().map(OsString::from))
    }

    #[test]
    fn a_full_command_line_parses() {
        let command = parse_strs(&[
            "-l",
            "a",
            "c.circom",
            "--include",
            "b",
            "-o",
            "g.bin",
            "--r1cs",
            "c.r1cs",
            "--expect-r1cs",
            "k.r1cs",
            "--timings",
        ]);
        assert_eq!(
            command,
            Ok(Command::Build(Args {
                circuit: "c.circom".into(),
                output: "g.bin".into(),
                include_paths: vec!["a".into(), "b".into()],
                r1cs: Some("c.r1cs".into()),
                expected_r1cs: Some("k.r1cs".into()),
                timings: true,
            }))
        );
    }

    #[test]
    fn mistakes_are_refused() {
        let refused = |arguments: &[&str], reason: &str| {
            let error = parse_strs(arguments).unwrap_err();
            assert!(error.contains(reason), "{arguments:?}: {error}");
        };
        refused(&["c.circom"], "no output given");
        refused(&["-o", "g.bin"], "no circuit given");
        refused(
            &["c.circom", "-o", "g.bin", "--bogus"],
            "unknown option --bogus",
        );
        refused(&["c.circom", "-o=g.bin"], "unknown option -o=g.bin");
        refused(&["c.circom", "-o", "a", "-o", "b"], "-o is given twice");
        refused(
            &[
                "c.circom",
                "-o",
                "g",
                "--expect-r1cs",
                "a",
                "--expect-r1cs",
                "b",
            ],
            "given twice",
        );
        refused(&["c.circom", "d.circom", "-o", "g"], "only one circuit");
        refused(&["c.circom", "-o"], "-o needs a value");
    }

    #[test]
    fn dash_dash_ends_the_options() {
        let Ok(Command::Build(args)) = parse_strs(&["-o", "g.bin", "--", "-odd.circom"]) else {
            panic!("did not parse");
        };
        assert_eq!(args.circuit, PathBuf::from("-odd.circom"));
    }

    #[test]
    fn help_and_version() {
        assert_eq!(parse_strs(&["--help"]), Ok(Command::Help));
        assert_eq!(parse_strs(&["-V"]), Ok(Command::Version));
    }
}
