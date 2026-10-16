// SPDX-License-Identifier: GPL-3.0-only
// SPDX-FileCopyrightText: 2026 Curvy Protocol d.o.o.
//! The program's handling of its files: what it refuses to overwrite, and that a
//! failed build leaves no graph behind.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn builder(arguments: &[&Path]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_curvy-signet-ir-builder"))
        .args(arguments)
        .output()
        .expect("the builder runs")
}

fn circuit(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/circuits")
        .join(name)
}

/// A fresh directory for one test under cargo's per-target scratch directory.
fn work(test: &str) -> PathBuf {
    let directory = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("cli")
        .join(test);
    let _ = std::fs::remove_dir_all(&directory);
    std::fs::create_dir_all(&directory).unwrap();
    directory
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

/// No `.partial` scratch file is left in `directory`.
fn assert_no_partial_files(directory: &Path) {
    for entry in std::fs::read_dir(directory).unwrap() {
        let name = entry.unwrap().file_name();
        assert!(
            !name.to_string_lossy().ends_with(".partial"),
            "left behind: {name:?}"
        );
    }
}

#[test]
fn a_matching_r1cs_writes_the_graph() {
    let work = work("matching");
    let (r1cs, graph) = (work.join("levels.r1cs"), work.join("graph.bin"));
    let kept = builder(&[
        &circuit("levels.circom"),
        "-o".as_ref(),
        &work.join("first.bin"),
        "--r1cs".as_ref(),
        &r1cs,
    ]);
    assert!(kept.status.success(), "{}", stderr(&kept));
    let checked = builder(&[
        &circuit("levels.circom"),
        "-o".as_ref(),
        &graph,
        "--expect-r1cs".as_ref(),
        &r1cs,
    ]);
    assert!(checked.status.success(), "{}", stderr(&checked));
    assert!(stderr(&checked).contains("matches"));
    assert_eq!(
        std::fs::read(&graph).unwrap(),
        std::fs::read(work.join("first.bin")).unwrap()
    );
    assert_no_partial_files(&work);
}

#[test]
fn a_different_r1cs_writes_nothing_and_removes_the_old_graph() {
    let work = work("mismatch");
    let (r1cs, graph) = (work.join("levels.r1cs"), work.join("graph.bin"));
    let kept = builder(&[
        &circuit("levels.circom"),
        "-o".as_ref(),
        &graph,
        "--r1cs".as_ref(),
        &r1cs,
    ]);
    assert!(kept.status.success(), "{}", stderr(&kept));
    assert!(graph.exists());
    let refused = builder(&[
        &circuit("ops.circom"),
        "-o".as_ref(),
        &graph,
        "--expect-r1cs".as_ref(),
        &r1cs,
    ]);
    assert!(!refused.status.success());
    assert!(
        stderr(&refused).contains("does not describe the compiled circuit"),
        "{}",
        stderr(&refused)
    );
    assert!(!graph.exists(), "the old graph survived a failed build");
    assert_no_partial_files(&work);
}

#[test]
fn the_expected_r1cs_is_never_overwritten() {
    let work = work("aliasing");
    let r1cs = work.join("key.r1cs");
    let kept = builder(&[
        &circuit("levels.circom"),
        "-o".as_ref(),
        &work.join("g.bin"),
        "--r1cs".as_ref(),
        &r1cs,
    ]);
    assert!(kept.status.success(), "{}", stderr(&kept));
    let before = std::fs::read(&r1cs).unwrap();

    let link = work.join("link.r1cs");
    std::os::unix::fs::symlink(&r1cs, &link).unwrap();
    for (r1cs_flag, expected) in [(&r1cs, &r1cs), (&link, &r1cs)] {
        let refused = builder(&[
            &circuit("ops.circom"),
            "-o".as_ref(),
            &work.join("ops.bin"),
            "--r1cs".as_ref(),
            r1cs_flag,
            "--expect-r1cs".as_ref(),
            expected,
        ]);
        assert!(!refused.status.success());
        assert!(
            stderr(&refused).contains("name the same file"),
            "{}",
            stderr(&refused)
        );
    }
    let refused = builder(&[
        &circuit("ops.circom"),
        "-o".as_ref(),
        &r1cs,
        "--expect-r1cs".as_ref(),
        &r1cs,
    ]);
    assert!(!refused.status.success());
    assert!(
        stderr(&refused).contains("would overwrite"),
        "{}",
        stderr(&refused)
    );
    assert_eq!(std::fs::read(&r1cs).unwrap(), before);
}

#[test]
fn the_circuit_is_never_overwritten() {
    let work = work("circuit");
    let source = work.join("levels.circom");
    std::fs::copy(circuit("levels.circom"), &source).unwrap();
    let before = std::fs::read(&source).unwrap();
    for flag in ["-o", "--r1cs"] {
        let mut arguments: Vec<&Path> = vec![&source, flag.as_ref(), &source];
        let graph = work.join("g.bin");
        if flag == "--r1cs" {
            arguments.extend(["-o".as_ref(), graph.as_path()]);
        }
        let refused = builder(&arguments);
        assert!(!refused.status.success(), "{flag}");
        assert!(
            stderr(&refused).contains("overwrite"),
            "{flag}: {}",
            stderr(&refused)
        );
    }
    assert_eq!(std::fs::read(&source).unwrap(), before);
}

#[test]
fn a_missing_output_directory_fails_before_the_build() {
    let work = work("no-directory");
    let refused = builder(&[
        &circuit("levels.circom"),
        "-o".as_ref(),
        &work.join("missing/g.bin"),
    ]);
    assert!(!refused.status.success());
    let message = stderr(&refused);
    assert!(message.contains("could not create a file in"), "{message}");
    assert!(
        !message.contains("template instances"),
        "the build ran: {message}"
    );
}

#[test]
fn command_line_mistakes_exit_with_usage() {
    let refused = builder(&[&circuit("levels.circom")]);
    assert_eq!(refused.status.code(), Some(2));
    assert!(stderr(&refused).contains("no output given"));
    assert!(stderr(&refused).contains("usage:"));

    let version = builder(&["--version".as_ref()]);
    assert!(version.status.success());
    let text = String::from_utf8_lossy(&version.stdout);
    assert!(
        text.starts_with("curvy-signet-ir-builder ") && text.contains("(circom 2.2.3)"),
        "{text}"
    );
}
