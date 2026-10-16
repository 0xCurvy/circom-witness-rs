// SPDX-License-Identifier: GPL-3.0-only
// SPDX-FileCopyrightText: 2026 Curvy Protocol d.o.o.
//! Statements: sequences, loops, returns, stores and the dispatch to branches, calls
//! and component creation.

use circom_compiler::intermediate_representation::ir_interface::{
    Instruction, InstructionList, ObtainMeta, StoreBucket,
};
use eyre::{bail, Result, WrapErr};
use ruint::aliases::U256;

use super::{Executor, Flow, Frame};

impl Executor<'_> {
    pub(super) fn execute(&mut self, body: &InstructionList, frame: &mut Frame) -> Result<Flow> {
        for instruction in body {
            let flow = self
                .statement(instruction, frame)
                .wrap_err_with(|| format!("at line {}", instruction.get_line()))?;
            if let Flow::Return(values) = flow {
                return Ok(Flow::Return(values));
            }
        }
        Ok(Flow::Continue)
    }

    fn statement(&mut self, instruction: &Instruction, frame: &mut Frame) -> Result<Flow> {
        match instruction {
            Instruction::Store(store) => self.store(store, frame)?,
            Instruction::Call(call) => {
                self.call(call, frame)?;
            }
            Instruction::Branch(branch) => return self.branch(branch, frame),
            Instruction::Loop(body) => {
                while self.condition(&body.continue_condition, frame)? {
                    if let Flow::Return(values) = self.execute(&body.body, frame)? {
                        return Ok(Flow::Return(values));
                    }
                }
            }
            Instruction::Return(bucket) => {
                let Some(result_size) = frame.function.as_ref().map(|(_, size)| *size) else {
                    bail!("a template cannot return");
                };
                let (mut values, _) = self.eval(&bucket.value, frame)?.field()?;
                // The generated code copies `min(with_size, destination size)` values, or
                // one value when the function returns a single one.
                let count = if bucket.with_size > 1 {
                    bucket.with_size.min(result_size)
                } else {
                    1
                };
                if values.len() < count {
                    bail!(
                        "a return provides {} values, {count} are needed",
                        values.len()
                    );
                }
                values.truncate(count);
                return Ok(Flow::Return(values.into_vec()));
            }
            Instruction::CreateCmp(create) => self.create(create, frame)?,
            // Witness generation does not check constraints or print.
            Instruction::Assert(_) | Instruction::Log(_) => {}
            Instruction::Value(_) | Instruction::Load(_) | Instruction::Compute(_) => {
                self.eval(instruction, frame)?;
            }
        }
        Ok(Flow::Continue)
    }

    fn condition(&mut self, instruction: &Instruction, frame: &mut Frame) -> Result<bool> {
        let node = self.scalar(instruction, frame)?;
        match self.graph.known(node) {
            Some(value) => Ok(value != U256::ZERO),
            None => bail!("a loop condition depends on an input"),
        }
    }

    fn store(&mut self, store: &StoreBucket, frame: &mut Frame) -> Result<()> {
        let place = self.place(&store.dest_address_type, &store.dest, frame)?;
        let destination_size = self.size(&store.context.size, place.component)?;
        let (values, value_component) = self.eval(&store.src, frame)?.field()?;
        let source_component = match &store.src_address_type {
            Some(address) => Some(self.subcomponent(address, frame)?),
            None => value_component,
        };
        let source_size = self.size(&store.src_context.size, source_component)?;
        let size = destination_size.min(source_size);
        if values.len() < size {
            bail!("a store copies {size} values from {}", values.len());
        }
        self.write(&place, &values[..size], frame)?;
        self.count_inputs(&store.dest_address_type, place.component, size)
    }
}
