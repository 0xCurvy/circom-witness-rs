// SPDX-License-Identifier: GPL-3.0-only
// SPDX-FileCopyrightText: 2026 Curvy Protocol d.o.o.
//! Graph construction: the node list, constant interning and folding.
//!
//! Nothing here knows circom's types, so the arithmetic the executor relies on can be
//! tested on its own.

use std::collections::HashMap;
use std::hash::{BuildHasherDefault, Hasher};

use curvy_signet_builder::graph::{Node, Operation};
use curvy_signet_builder::M;
use eyre::{bail, Result};
use ruint::aliases::U256;

/// Cheap multiplicative hasher for tables keyed by integers and field limbs; the
/// default SipHash dominated the executor's constant lookups.
#[derive(Default, Clone, Copy)]
pub(crate) struct LimbHasher(u64);

impl Hasher for LimbHasher {
    fn finish(&self) -> u64 {
        self.0.rotate_left(26)
    }

    fn write(&mut self, bytes: &[u8]) {
        for chunk in bytes.chunks(8) {
            let mut word = [0u8; 8];
            word[..chunk.len()].copy_from_slice(chunk);
            self.write_u64(u64::from_le_bytes(word));
        }
    }

    fn write_u64(&mut self, word: u64) {
        self.0 = (self.0.rotate_left(5) ^ word).wrapping_mul(0x517c_c1b7_2722_0a95);
    }

    fn write_usize(&mut self, word: usize) {
        self.write_u64(word as u64);
    }
}

pub(crate) type FastMap<K, V> = HashMap<K, V, BuildHasherDefault<LimbHasher>>;

/// The graph being built. Constants are interned, so a value known at build time has
/// exactly one node.
pub(crate) struct NodeBuilder {
    pub nodes: Vec<Node>,
    constants: FastMap<U256, usize>,
    pub zero: usize,
    pub one: usize,
}

impl NodeBuilder {
    pub fn new() -> Self {
        let mut builder = Self {
            nodes: Vec::new(),
            constants: FastMap::default(),
            zero: 0,
            one: 0,
        };
        builder.zero = builder.constant(U256::ZERO);
        builder.one = builder.constant(U256::from(1));
        builder
    }

    pub fn push(&mut self, node: Node) -> usize {
        self.nodes.push(node);
        self.nodes.len() - 1
    }

    /// The build-time value of `node`, when it is a constant.
    #[inline]
    pub fn known(&self, node: usize) -> Option<U256> {
        match self.nodes[node] {
            Node::Constant(value) => Some(value),
            _ => None,
        }
    }

    pub fn constant(&mut self, value: U256) -> usize {
        if let Some(&node) = self.constants.get(&value) {
            return node;
        }
        let node = self.push(Node::Constant(value));
        self.constants.insert(value, node);
        node
    }

    /// Adds `op(a, b)`, folding it when both operands are known.
    pub fn raw_op(&mut self, op: Operation, a: usize, b: usize) -> Result<usize> {
        if let (Some(left), Some(right)) = (self.known(a), self.known(b)) {
            if matches!(op, Operation::Div | Operation::IDiv | Operation::Mod)
                && right == U256::ZERO
            {
                bail!("division by zero in an expression known at build time");
            }
            return Ok(self.constant(op.eval(left, right)));
        }
        Ok(self.push(Node::Op(op, a, b)))
    }

    /// `if_value` where the 0/1 node `condition` is 1, otherwise `else_value`.
    pub fn select(
        &mut self,
        condition: usize,
        if_value: usize,
        else_value: usize,
    ) -> Result<usize> {
        if if_value == else_value {
            return Ok(if_value);
        }
        if let (Some(a), Some(b)) = (self.known(if_value), self.known(else_value)) {
            if a == b {
                return Ok(if_value);
            }
        }
        let difference = self.raw_op(Operation::Sub, if_value, else_value)?;
        let selected = self.raw_op(Operation::Mul, condition, difference)?;
        self.raw_op(Operation::Add, else_value, selected)
    }

    /// A 0/1 node that is 1 exactly when `node` is non-zero.
    pub fn truth(&mut self, node: usize) -> Result<usize> {
        if let Node::Op(op, _, _) = self.nodes[node] {
            use Operation::*;
            if matches!(op, Eq | Neq | Lt | Gt | Leq | Geq | Land | Lor) {
                return Ok(node);
            }
        }
        if let Some(value) = self.known(node) {
            return Ok(self.constant(U256::from(value != U256::ZERO)));
        }
        self.raw_op(Operation::Neq, node, self.zero)
    }

    /// circom's `a / b`: the field quotient, and 0 when `b` is 0. The graph's own
    /// division fails on a zero divisor, so it only ever sees a non-zero one.
    pub fn div(&mut self, a: usize, b: usize) -> Result<usize> {
        match self.known(b) {
            Some(divisor) if divisor == U256::ZERO => Ok(self.zero),
            Some(_) => self.raw_op(Operation::Div, a, b),
            None => {
                let is_zero = self.raw_op(Operation::Eq, b, self.zero)?;
                let divisor = self.raw_op(Operation::Add, b, is_zero)?;
                let quotient = self.raw_op(Operation::Div, a, divisor)?;
                let non_zero = self.raw_op(Operation::Neq, b, self.zero)?;
                self.raw_op(Operation::Mul, non_zero, quotient)
            }
        }
    }

    /// circom's `a << b` (`left`) or `a >> b`. An amount below 254 shifts as written,
    /// an amount within 254 of p (a negative amount) shifts the other way by p - b,
    /// and any other amount gives 0. The graph's own shifts take amounts below 256,
    /// so they only ever see an amount below 254.
    pub fn shift(&mut self, left: bool, a: usize, b: usize) -> Result<usize> {
        let (forward, backward) = if left {
            (Operation::Shl, Operation::Shr)
        } else {
            (Operation::Shr, Operation::Shl)
        };
        let width = U256::from(FIELD_BITS);
        if let Some(amount) = self.known(b) {
            let negated = (M - amount) % M;
            return if amount < width {
                self.raw_op(forward, a, b)
            } else if negated < width {
                let negated = self.constant(negated);
                self.raw_op(backward, a, negated)
            } else {
                Ok(self.zero)
            };
        }
        // circom's comparisons read an element above p/2 as negative.
        let zero = self.zero;
        let width = self.constant(width);
        let minus_width = self.constant(M - U256::from(FIELD_BITS));
        let not_negative = self.raw_op(Operation::Geq, b, zero)?;
        let short = self.raw_op(Operation::Lt, b, width)?;
        let forward_taken = self.raw_op(Operation::Land, not_negative, short)?;
        let negative = self.raw_op(Operation::Lt, b, zero)?;
        let near = self.raw_op(Operation::Gt, b, minus_width)?;
        let backward_taken = self.raw_op(Operation::Land, negative, near)?;
        let forward_amount = self.raw_op(Operation::Mul, forward_taken, b)?;
        let negated = self.raw_op(Operation::Neg, b, b)?;
        let backward_amount = self.raw_op(Operation::Mul, backward_taken, negated)?;
        let forward_value = self.raw_op(forward, a, forward_amount)?;
        let backward_value = self.raw_op(backward, a, backward_amount)?;
        let forward_value = self.raw_op(Operation::Mul, forward_taken, forward_value)?;
        let backward_value = self.raw_op(Operation::Mul, backward_taken, backward_value)?;
        self.raw_op(Operation::Add, forward_value, backward_value)
    }

    /// circom's `a && b` (`Land`) or `a || b` (`Lor`). An operand known at build time
    /// decides the result or leaves the other operand's truth.
    pub fn logical(&mut self, op: Operation, a: usize, b: usize) -> Result<usize> {
        debug_assert!(matches!(op, Operation::Land | Operation::Lor));
        for (known, other) in [(a, b), (b, a)] {
            if let (Some(value), None) = (self.known(known), self.known(other)) {
                let decides = (value != U256::ZERO) == (op == Operation::Lor);
                return if decides {
                    Ok(self.constant(U256::from(op == Operation::Lor)))
                } else {
                    self.truth(other)
                };
            }
        }
        self.raw_op(op, a, b)
    }

    /// A node that is 0 where the 0/1 node `guard` is 0, and makes witness generation
    /// fail where it is 1.
    pub fn fail_where(&mut self, guard: usize) -> Result<usize> {
        let not_guard = self.raw_op(Operation::Sub, self.one, guard)?;
        self.raw_op(Operation::IDiv, self.zero, not_guard)
    }
}

/// Bits of a canonical bn128 element, `qbits` in circom's field code.
const FIELD_BITS: u64 = 254;

#[cfg(test)]
mod tests {
    use super::*;
    use curvy_signet_builder::graph::evaluate;

    /// Evaluates `build(builder, a, b)` with `a` and `b` as graph inputs, or `None`
    /// when witness generation fails.
    fn run(
        build: impl Fn(&mut NodeBuilder, usize, usize) -> Result<usize>,
        a: U256,
        b: U256,
    ) -> Option<U256> {
        let mut builder = NodeBuilder::new();
        let (x, y) = (builder.push(Node::Input(0)), builder.push(Node::Input(1)));
        let out = build(&mut builder, x, y).unwrap();
        evaluate(&builder.nodes, &[a, b], &[out])
            .ok()
            .map(|values| values[0])
    }

    /// Folds `build` with both operands known.
    fn fold(
        build: impl Fn(&mut NodeBuilder, usize, usize) -> Result<usize>,
        a: U256,
        b: U256,
    ) -> U256 {
        let mut builder = NodeBuilder::new();
        let (x, y) = (builder.constant(a), builder.constant(b));
        let out = build(&mut builder, x, y).unwrap();
        builder.known(out).expect("folded")
    }

    /// circom's runtime shift (`Fr_shl` / `Fr_shr` in its field code).
    fn circom_shift(left: bool, a: U256, b: U256) -> U256 {
        let mask = (U256::from(1) << 254) - U256::from(1);
        let shl = |s: usize| ((a << s) & mask) % M;
        let shr = |s: usize| a >> s;
        let width = U256::from(254);
        if b < width {
            let s = b.as_limbs()[0] as usize;
            if left {
                shl(s)
            } else {
                shr(s)
            }
        } else if M - b < width {
            let s = (M - b).as_limbs()[0] as usize;
            if left {
                shr(s)
            } else {
                shl(s)
            }
        } else {
            U256::ZERO
        }
    }

    fn edge_values() -> Vec<U256> {
        let mut values: Vec<U256> = (0..260u64).map(U256::from).collect();
        values.extend((1..260u64).map(|k| M - U256::from(k)));
        let half = M >> 1;
        values.extend([
            half - U256::from(1),
            half,
            half + U256::from(1),
            half + U256::from(2),
        ]);
        values.push(M - U256::from(1) - (U256::from(1) << 200));
        values
    }

    #[test]
    fn shifts_follow_circom_for_every_amount() {
        let operands = [
            U256::ZERO,
            U256::from(1),
            U256::from(0xdead_beefu64),
            M - U256::from(1),
            (U256::from(1) << 253) + U256::from(12345),
        ];
        for left in [true, false] {
            let build = |builder: &mut NodeBuilder, a, b| builder.shift(left, a, b);
            for &a in &operands {
                for b in edge_values() {
                    let expected = circom_shift(left, a, b);
                    assert_eq!(run(build, a, b), Some(expected), "left={left} a={a} b={b}");
                    assert_eq!(
                        fold(build, a, b),
                        expected,
                        "folded left={left} a={a} b={b}"
                    );
                }
            }
        }
    }

    #[test]
    fn division_by_zero_is_zero() {
        let div = |builder: &mut NodeBuilder, a, b| builder.div(a, b);
        assert_eq!(run(div, U256::from(5), U256::ZERO), Some(U256::ZERO));
        assert_eq!(fold(div, U256::from(5), U256::ZERO), U256::ZERO);
        let six = U256::from(6);
        let quotient = run(div, U256::from(5), six).unwrap();
        assert_eq!(quotient.mul_mod(six, M), U256::from(5));
        assert_eq!(fold(div, U256::from(5), six), quotient);
    }

    #[test]
    fn logical_operators_fold_a_known_operand() {
        for op in [Operation::Land, Operation::Lor] {
            for known in [U256::ZERO, U256::from(1), U256::from(7)] {
                for input in [U256::ZERO, U256::from(3)] {
                    let expected = op.eval(known, input);
                    let mut builder = NodeBuilder::new();
                    let x = builder.push(Node::Input(0));
                    let k = builder.constant(known);
                    for (a, b) in [(x, k), (k, x)] {
                        let out = builder.logical(op, a, b).unwrap();
                        let value = evaluate(&builder.nodes, &[input], &[out]).unwrap()[0];
                        assert_eq!(value, expected, "{op:?} {known} {input}");
                    }
                }
            }
        }
    }

    #[test]
    fn fail_where_fails_only_on_the_guarded_path() {
        let fail = |builder: &mut NodeBuilder, guard, _| builder.fail_where(guard);
        assert_eq!(run(fail, U256::ZERO, U256::ZERO), Some(U256::ZERO));
        assert_eq!(run(fail, U256::from(1), U256::ZERO), None);
    }
}
