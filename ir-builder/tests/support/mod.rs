// SPDX-License-Identifier: GPL-3.0-only
// SPDX-FileCopyrightText: 2026 Curvy Protocol d.o.o.
//! Helpers shared by the integration tests and, through `#[path]`, the example tools.
#![allow(dead_code)]

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use ruint::aliases::U256;

/// `tests/fixtures`.
pub fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

/// A circom input file as the flat values of each named input.
pub fn read_inputs(path: &Path) -> eyre::Result<HashMap<String, Vec<U256>>> {
    let json: serde_json::Map<String, serde_json::Value> =
        serde_json::from_slice(&std::fs::read(path)?)?;
    json.into_iter()
        .map(|(name, value)| {
            let mut values = Vec::new();
            flatten(&value, &mut values)?;
            Ok((name, values))
        })
        .collect()
}

fn flatten(value: &serde_json::Value, out: &mut Vec<U256>) -> eyre::Result<()> {
    match value {
        serde_json::Value::Array(items) => {
            for item in items {
                flatten(item, out)?;
            }
        }
        serde_json::Value::String(text) => out.push(text.parse()?),
        serde_json::Value::Number(number) => {
            out.push(U256::from(number.as_u64().ok_or_else(|| {
                eyre::eyre!("unsupported input number {number}")
            })?))
        }
        other => eyre::bail!("unsupported input value {other}"),
    }
    Ok(())
}

/// One entry of `tests/fixtures/cases.json`.
pub struct Case {
    pub circuit: String,
    pub input: Option<String>,
    pub reason: Option<String>,
    /// An include tree the circuit needs, such as `circomlib`.
    pub needs: Option<String>,
}

/// The cases of one list in `tests/fixtures/cases.json`: `witness`, `fails` or `rejected`.
pub fn cases(list: &str) -> Vec<Case> {
    let manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(fixtures().join("cases.json")).unwrap()).unwrap();
    let text = |entry: &serde_json::Value, key: &str| entry[key].as_str().map(str::to_owned);
    manifest[list]
        .as_array()
        .unwrap_or_else(|| panic!("cases.json has no list {list}"))
        .iter()
        .map(|entry| Case {
            circuit: text(entry, "circuit").expect("a case names its circuit"),
            input: text(entry, "input"),
            reason: text(entry, "reason"),
            needs: text(entry, "needs"),
        })
        .collect()
}
