// SPDX-License-Identifier: GPL-3.0-only
// SPDX-FileCopyrightText: 2026 Curvy Protocol d.o.o.
//! Expressions: constants, loads, address arithmetic and field operations.

use circom_compiler::intermediate_representation::ir_interface::{
    ComputeBucket, Instruction, OperatorType, ValueType,
};
use curvy_signet_builder::graph::Operation;
use curvy_signet_builder::M;
use eyre::{bail, eyre, Result};
use ruint::aliases::U256;
use smallvec::{smallvec, SmallVec};

use super::{first, Executor, Frame, Value};

impl Executor<'_> {
    pub(super) fn eval(&mut self, instruction: &Instruction, frame: &mut Frame) -> Result<Value> {
        match instruction {
            Instruction::Value(value) => Ok(match value.parse_as {
                ValueType::U32 => Value::Address(value.value),
                ValueType::BigInt => Value::Field {
                    nodes: smallvec![*self
                        .field_constants
                        .get(value.value)
                        .ok_or_else(|| eyre!("constant {} is out of range", value.value))?],
                    component: None,
                },
            }),
            Instruction::Load(load) => {
                let place = self.place(&load.address_type, &load.src, frame)?;
                let size = self.size(&load.context.size, place.component)?;
                Ok(Value::Field {
                    nodes: self.read(&place, size, frame)?,
                    component: place.component,
                })
            }
            Instruction::Compute(compute) => self.compute(compute, frame),
            Instruction::Call(call) => self
                .call(call, frame)?
                .ok_or_else(|| eyre!("a call that stores its result is used as a value")),
            _ => bail!("a statement is used as a value"),
        }
    }

    /// Evaluates an expression that must be an address. Index arithmetic is most of the
    /// instruction stream, so it is computed without building a `Value`.
    pub(super) fn address(
        &mut self,
        instruction: &Instruction,
        frame: &mut Frame,
    ) -> Result<usize> {
        match instruction {
            Instruction::Value(value) if matches!(value.parse_as, ValueType::U32) => {
                Ok(value.value)
            }
            Instruction::Compute(compute)
                if matches!(
                    compute.op,
                    OperatorType::AddAddress | OperatorType::MulAddress
                ) && compute.stack.len() == 2 =>
            {
                let a = self.address(&compute.stack[0], frame)?;
                let b = self.address(&compute.stack[1], frame)?;
                let result = if matches!(compute.op, OperatorType::AddAddress) {
                    a.checked_add(b)
                } else {
                    a.checked_mul(b)
                };
                result.ok_or_else(|| eyre!("an address computation overflows"))
            }
            _ => self.eval(instruction, frame)?.address(),
        }
    }

    pub(super) fn scalar(&mut self, instruction: &Instruction, frame: &mut Frame) -> Result<usize> {
        let (nodes, _) = self.eval(instruction, frame)?.field()?;
        nodes
            .first()
            .copied()
            .ok_or_else(|| eyre!("an empty value"))
    }

    fn compute(&mut self, compute: &ComputeBucket, frame: &mut Frame) -> Result<Value> {
        let mut operands: SmallVec<[Value; 2]> = SmallVec::new();
        for operand in &compute.stack {
            operands.push(self.eval(operand, frame)?);
        }
        let mut operands = operands.into_iter();
        let mut next = || {
            operands
                .next()
                .ok_or_else(|| eyre!("an operation is missing an operand"))
        };

        use OperatorType as Op;
        let node = match &compute.op {
            Op::AddAddress | Op::MulAddress => {
                let (a, b) = (next()?.address()?, next()?.address()?);
                let result = if matches!(compute.op, Op::AddAddress) {
                    a.checked_add(b)
                } else {
                    a.checked_mul(b)
                };
                return Ok(Value::Address(
                    result.ok_or_else(|| eyre!("an address computation overflows"))?,
                ));
            }
            Op::ToAddress => {
                let (nodes, _) = next()?.field()?;
                let node = *nodes.first().ok_or_else(|| eyre!("an empty index"))?;
                let value = self
                    .graph
                    .known(node)
                    .ok_or_else(|| eyre!("an array index depends on an input"))?;
                return Ok(Value::Address(
                    usize::try_from(value).map_err(|_| eyre!("an array index is too large"))?,
                ));
            }
            Op::Eq(size) => {
                let (left, left_component) = next()?.field()?;
                let (right, right_component) = next()?.field()?;
                let size = self.size(size, left_component.or(right_component))?;
                if size == 0 || left.len() < size || right.len() < size {
                    bail!("an equality compares {size} values of operands with fewer");
                }
                let mut result = self.graph.raw_op(Operation::Eq, left[0], right[0])?;
                for index in 1..size {
                    let equal = self
                        .graph
                        .raw_op(Operation::Eq, left[index], right[index])?;
                    result = self.graph.raw_op(Operation::Mul, result, equal)?;
                }
                result
            }
            Op::PrefixSub => {
                let a = first(next()?)?;
                self.graph.raw_op(Operation::Neg, a, a)?
            }
            Op::BoolNot => {
                let a = first(next()?)?;
                let zero = self.graph.zero;
                self.graph.raw_op(Operation::Eq, a, zero)?
            }
            Op::Complement => {
                // circom complements the 254-bit representation and reduces it. For a
                // canonical value that is (2^254 - 1) - a, i.e. ((2^254 - 1) mod p) - a.
                let a = first(next()?)?;
                let mask = (U256::from(1) << 254) - U256::from(1);
                let mask = self.graph.constant(mask % M);
                self.graph.raw_op(Operation::Sub, mask, a)?
            }
            binary => {
                let operation = match binary {
                    Op::Mul => Operation::Mul,
                    Op::Div => Operation::Div,
                    Op::Add => Operation::Add,
                    Op::Sub => Operation::Sub,
                    Op::Pow => Operation::Pow,
                    Op::IntDiv => Operation::IDiv,
                    Op::Mod => Operation::Mod,
                    Op::ShiftL => Operation::Shl,
                    Op::ShiftR => Operation::Shr,
                    Op::LesserEq => Operation::Leq,
                    Op::GreaterEq => Operation::Geq,
                    Op::Lesser => Operation::Lt,
                    Op::Greater => Operation::Gt,
                    Op::NotEq => Operation::Neq,
                    Op::BoolOr => Operation::Lor,
                    Op::BoolAnd => Operation::Land,
                    Op::BitOr => Operation::Bor,
                    Op::BitAnd => Operation::Band,
                    Op::BitXor => Operation::Bxor,
                    _ => unreachable!("handled above"),
                };
                let a = first(next()?)?;
                let b = first(next()?)?;
                self.op(operation, a, b)?
            }
        };
        Ok(Value::Field {
            nodes: smallvec![node],
            component: None,
        })
    }

    /// Adds `op(a, b)` with circom's semantics, folding it when both operands are
    /// known. Under a guard, an integer divisor is replaced by 1 on the path not taken,
    /// so only the taken path can fail where circom aborts.
    fn op(&mut self, op: Operation, a: usize, b: usize) -> Result<usize> {
        match op {
            Operation::Div => self.graph.div(a, b),
            Operation::Shl => self.graph.shift(true, a, b),
            Operation::Shr => self.graph.shift(false, a, b),
            Operation::Land | Operation::Lor => self.graph.logical(op, a, b),
            Operation::IDiv | Operation::Mod => {
                let b = self.guarded_divisor(b)?;
                self.graph.raw_op(op, a, b)
            }
            _ => self.graph.raw_op(op, a, b),
        }
    }
}
