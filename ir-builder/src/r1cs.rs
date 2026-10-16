// SPDX-License-Identifier: GPL-3.0-only
// SPDX-FileCopyrightText: 2026 Curvy Protocol d.o.o.
//! Decides whether two R1CS files describe the same circuit.
//!
//! circom 2.2.3 does not order the constraints of some circuits deterministically,
//! in-process or from its command line, so two compilations of one circuit can differ
//! byte for byte. What a witness depends on does not vary: the header (field, wire and
//! input counts), the wire-to-label map (which signal each witness position holds) and
//! the multiset of constraints. This module compares exactly those, ignoring the order
//! of constraints and of the terms inside each linear combination. Every other section,
//! custom gates included, must match byte for byte. The comparison errs towards
//! refusing: a zero coefficient written out, or one wire's term split in two, makes
//! equal constraints compare as different, never the reverse.

use std::collections::BTreeMap;

use eyre::{bail, ensure, eyre, Result};

const HEADER: u32 = 1;
const CONSTRAINTS: u32 = 2;

/// One linear combination as `(wire, coefficient bytes)`, sorted by wire.
type Combination<'a> = Vec<(u32, &'a [u8])>;

/// Fails with the first difference that matters to a witness.
pub fn ensure_equivalent(compiled: &[u8], expected: &[u8]) -> Result<()> {
    let compiled = sections(compiled).map_err(|error| eyre!("compiled R1CS: {error}"))?;
    let expected = sections(expected).map_err(|error| eyre!("expected R1CS: {error}"))?;
    let kinds = |sections: &BTreeMap<u32, &[u8]>| sections.keys().copied().collect::<Vec<_>>();
    ensure!(
        kinds(&compiled) == kinds(&expected),
        "the R1CS files have different sections: {:?} and {:?}",
        kinds(&compiled),
        kinds(&expected)
    );
    for (kind, compiled_section) in &compiled {
        let expected_section = expected[kind];
        if *kind == CONSTRAINTS {
            let field_size = field_size(compiled[&HEADER])?;
            ensure!(
                constraints(compiled_section, field_size)?
                    == constraints(expected_section, field_size)?,
                "the circuits have different constraints"
            );
        } else if compiled_section != &expected_section {
            bail!(
                "R1CS section {kind} differs{}",
                match *kind {
                    HEADER => " (the field, wire, input or constraint counts)",
                    3 => " (the wire-to-label map, which fixes the witness layout)",
                    _ => "",
                }
            );
        }
    }
    Ok(())
}

fn sections(bytes: &[u8]) -> Result<BTreeMap<u32, &[u8]>> {
    let mut reader = Reader(bytes);
    ensure!(reader.take(4)? == b"r1cs", "not an R1CS file");
    let version = reader.u32()?;
    ensure!(version == 1, "unsupported R1CS version {version}");
    let count = reader.u32()?;
    let mut sections = BTreeMap::new();
    for _ in 0..count {
        let kind = reader.u32()?;
        let size = usize::try_from(reader.u64()?).map_err(|_| eyre!("oversized section"))?;
        ensure!(
            sections.insert(kind, reader.take(size)?).is_none(),
            "section {kind} appears twice"
        );
    }
    ensure!(reader.0.is_empty(), "trailing bytes after the last section");
    ensure!(sections.contains_key(&HEADER), "no header section");
    Ok(sections)
}

fn field_size(header: &[u8]) -> Result<usize> {
    Ok(Reader(header).u32()? as usize)
}

/// The constraints as a sorted list, each with its linear combinations sorted by wire.
fn constraints(section: &[u8], field_size: usize) -> Result<Vec<[Combination<'_>; 3]>> {
    let mut reader = Reader(section);
    let mut constraints = Vec::new();
    while !reader.0.is_empty() {
        let mut combination = || -> Result<Combination<'_>> {
            let terms = reader.u32()?;
            let mut combination = (0..terms)
                .map(|_| Ok((reader.u32()?, reader.take(field_size)?)))
                .collect::<Result<Combination<'_>>>()?;
            combination.sort_unstable();
            Ok(combination)
        };
        constraints.push([combination()?, combination()?, combination()?]);
    }
    constraints.sort_unstable();
    Ok(constraints)
}

struct Reader<'a>(&'a [u8]);

impl<'a> Reader<'a> {
    fn take(&mut self, count: usize) -> Result<&'a [u8]> {
        ensure!(self.0.len() >= count, "truncated R1CS");
        let (taken, rest) = self.0.split_at(count);
        self.0 = rest;
        Ok(taken)
    }

    fn u32(&mut self) -> Result<u32> {
        Ok(u32::from_le_bytes(
            self.take(4)?.try_into().expect("four bytes"),
        ))
    }

    fn u64(&mut self) -> Result<u64> {
        Ok(u64::from_le_bytes(
            self.take(8)?.try_into().expect("eight bytes"),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An R1CS with one-byte coefficients and the given constraints, as `(wire, coefficient)` terms.
    fn r1cs(constraints: &[[&[(u32, u8)]; 3]], wire_map: &[u64]) -> Vec<u8> {
        file(&sections(constraints, wire_map, &[]))
    }

    /// The header, constraint and wire-map sections, then `extra`.
    fn sections(
        constraints: &[[&[(u32, u8)]; 3]],
        wire_map: &[u64],
        extra: &[(u32, Vec<u8>)],
    ) -> Vec<(u32, Vec<u8>)> {
        let mut header = 1u32.to_le_bytes().to_vec();
        header.push(7);
        header.extend_from_slice(&[0; 20]);
        let mut body = Vec::new();
        for constraint in constraints {
            for combination in constraint {
                body.extend_from_slice(&(combination.len() as u32).to_le_bytes());
                for (wire, coefficient) in *combination {
                    body.extend_from_slice(&wire.to_le_bytes());
                    body.push(*coefficient);
                }
            }
        }
        let map: Vec<u8> = wire_map
            .iter()
            .flat_map(|label| label.to_le_bytes())
            .collect();
        let mut sections = vec![(1u32, header), (2, body), (3, map)];
        sections.extend_from_slice(extra);
        sections
    }

    fn file(sections: &[(u32, Vec<u8>)]) -> Vec<u8> {
        let mut file = b"r1cs".to_vec();
        file.extend_from_slice(&1u32.to_le_bytes());
        file.extend_from_slice(&(sections.len() as u32).to_le_bytes());
        for (kind, section) in sections {
            file.extend_from_slice(&kind.to_le_bytes());
            file.extend_from_slice(&(section.len() as u64).to_le_bytes());
            file.extend_from_slice(section);
        }
        file
    }

    #[test]
    fn constraint_and_term_order_do_not_matter() {
        let a = r1cs(
            &[[&[(1, 2), (2, 3)], &[], &[(3, 1)]], [&[], &[], &[(4, 5)]]],
            &[0, 1, 2],
        );
        let b = r1cs(
            &[[&[], &[], &[(4, 5)]], [&[(2, 3), (1, 2)], &[], &[(3, 1)]]],
            &[0, 1, 2],
        );
        ensure_equivalent(&a, &b).unwrap();
    }

    #[test]
    fn different_constraints_or_layouts_are_rejected() {
        let a = r1cs(&[[&[(1, 2)], &[], &[(3, 1)]]], &[0, 1, 2]);
        let coefficient = r1cs(&[[&[(1, 9)], &[], &[(3, 1)]]], &[0, 1, 2]);
        let layout = r1cs(&[[&[(1, 2)], &[], &[(3, 1)]]], &[0, 2, 1]);
        assert!(ensure_equivalent(&a, &coefficient).is_err());
        assert!(ensure_equivalent(&a, &layout).is_err());
        assert!(ensure_equivalent(&a, b"r1cs").is_err());
    }

    #[test]
    fn duplicate_constraints_count() {
        let x: [&[(u32, u8)]; 3] = [&[(1, 2)], &[], &[(3, 1)]];
        let y: [&[(u32, u8)]; 3] = [&[(2, 2)], &[], &[(3, 1)]];
        let twice = r1cs(&[x, x], &[0, 1, 2]);
        ensure_equivalent(&twice, &r1cs(&[x, x], &[0, 1, 2])).unwrap();
        assert!(ensure_equivalent(&twice, &r1cs(&[x, y], &[0, 1, 2])).is_err());
        assert!(ensure_equivalent(&twice, &r1cs(&[x], &[0, 1, 2])).is_err());
    }

    #[test]
    fn section_order_does_not_matter_but_every_section_does() {
        let x: [&[(u32, u8)]; 3] = [&[(1, 2)], &[], &[(3, 1)]];
        let gates = (4u32, vec![1, 2, 3]);
        let base = sections(&[x], &[0, 1, 2], std::slice::from_ref(&gates));
        let mut reordered = base.clone();
        reordered.rotate_left(1);
        ensure_equivalent(&file(&base), &file(&reordered)).unwrap();

        let other_gates = sections(&[x], &[0, 1, 2], &[(4, vec![1, 2, 4])]);
        assert!(ensure_equivalent(&file(&base), &file(&other_gates)).is_err());
        let without_gates = sections(&[x], &[0, 1, 2], &[]);
        assert!(ensure_equivalent(&file(&base), &file(&without_gates)).is_err());
        let mut duplicated = base.clone();
        duplicated.push(gates);
        assert!(ensure_equivalent(&file(&base), &file(&duplicated)).is_err());
    }

    #[test]
    fn malformed_files_are_rejected() {
        let x: [&[(u32, u8)]; 3] = [&[(1, 2)], &[], &[(3, 1)]];
        let good = r1cs(&[x], &[0, 1, 2]);
        for cut in [3, 11, 20, good.len() - 1] {
            assert!(
                ensure_equivalent(&good, &good[..cut]).is_err(),
                "cut at {cut}"
            );
        }
        let mut trailing = good.clone();
        trailing.push(0);
        assert!(ensure_equivalent(&good, &trailing).is_err());
        let headerless: Vec<_> = sections(&[x], &[0, 1, 2], &[])
            .into_iter()
            .skip(1)
            .collect();
        assert!(ensure_equivalent(&good, &file(&headerless)).is_err());
    }

    #[test]
    fn written_out_zero_coefficients_are_refused() {
        let x: [&[(u32, u8)]; 3] = [&[(1, 2)], &[], &[(3, 1)]];
        let zero: [&[(u32, u8)]; 3] = [&[(1, 2), (2, 0)], &[], &[(3, 1)]];
        assert!(ensure_equivalent(&r1cs(&[x], &[0, 1, 2]), &r1cs(&[zero], &[0, 1, 2])).is_err());
    }
}
