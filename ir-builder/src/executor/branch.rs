// SPDX-License-Identifier: GPL-3.0-only
// SPDX-FileCopyrightText: 2026 Curvy Protocol d.o.o.
//! Branches whose condition depends on an input: both sides run under a guard and
//! their results are selected with the condition.

use std::collections::{HashMap, HashSet};

use circom_compiler::intermediate_representation::ir_interface::{
    BranchBucket, Instruction, InstructionList,
};
use curvy_signet_builder::graph::Operation;
use eyre::{bail, Result};
use ruint::aliases::U256;

use super::{Executor, Flow, Frame, InputWrite};

/// What one side of an input-dependent branch produced.
struct Side {
    variables: Vec<usize>,
    /// Signal values by signal, undone until the sides are merged.
    signals: HashMap<usize, usize>,
    /// Subcomponent input writes, counted once the sides are merged.
    inputs: Vec<InputWrite>,
}

impl Executor<'_> {
    pub(super) fn branch(&mut self, branch: &BranchBucket, frame: &mut Frame) -> Result<Flow> {
        let condition = self.scalar(&branch.cond, frame)?;
        if let Some(value) = self.graph.known(condition) {
            let body = if value != U256::ZERO {
                &branch.if_branch
            } else {
                &branch.else_branch
            };
            return self.execute(body, frame);
        }
        if let Some((name, _)) = &frame.function {
            bail!("a branch in function {name} depends on an input");
        }
        if !executable_eagerly(&branch.if_branch) || !executable_eagerly(&branch.else_branch) {
            bail!("a branch that depends on an input creates components or returns");
        }

        let taken = self.graph.truth(condition)?;
        let one = self.graph.one;
        let not_taken = self.graph.raw_op(Operation::Sub, one, taken)?;
        let (if_guard, else_guard) = match self.guard {
            None => (taken, not_taken),
            Some(guard) => (
                self.graph.raw_op(Operation::Mul, guard, taken)?,
                self.graph.raw_op(Operation::Mul, guard, not_taken)?,
            ),
        };
        let base = frame.variables.clone();
        let if_side = self.eager_side(&branch.if_branch, if_guard, frame)?;
        frame.variables.clone_from(&base);
        let else_side = self.eager_side(&branch.else_branch, else_guard, frame)?;
        let Side {
            variables: if_variables,
            signals: if_signals,
            inputs: if_inputs,
        } = if_side;
        let Side {
            variables: else_variables,
            signals: else_signals,
            inputs: else_inputs,
        } = else_side;

        for (slot, (if_value, else_value)) in
            if_variables.into_iter().zip(else_variables).enumerate()
        {
            frame.variables[slot] = self.graph.select(taken, if_value, else_value)?;
        }
        let written: HashSet<usize> = if_signals
            .keys()
            .chain(else_signals.keys())
            .copied()
            .collect();
        let mut written: Vec<usize> = written.into_iter().collect();
        written.sort_unstable();
        for signal in written {
            let (Some(&if_value), Some(&else_value)) =
                (if_signals.get(&signal), else_signals.get(&signal))
            else {
                bail!("signal {signal} is assigned on only one side of a branch that depends on an input");
            };
            let value = self.graph.select(taken, if_value, else_value)?;
            self.write_signal(signal, value)?;
        }

        // Both sides wrote the same signals, so the same subcomponent inputs. Count
        // them now that the merged values are in place, which may run a component,
        // or hand them to the enclosing eager side.
        let totals = |writes: &[InputWrite]| {
            let mut totals: Vec<(usize, usize)> = Vec::new();
            for write in writes {
                match totals.iter_mut().find(|(id, _)| *id == write.component) {
                    Some((_, total)) => *total += write.size,
                    None => totals.push((write.component, write.size)),
                }
            }
            totals.sort_unstable();
            totals
        };
        if totals(&if_inputs) != totals(&else_inputs) {
            bail!("the sides of a branch that depends on an input write different subcomponent inputs");
        }
        match self.deferred_inputs.last_mut() {
            Some(outer) => outer.extend(if_inputs),
            None => {
                for write in if_inputs {
                    self.apply_input_write(write)?;
                }
            }
        }
        Ok(Flow::Continue)
    }

    /// Executes one side of an input-dependent branch under `guard` and undoes its
    /// signal writes, returning the variables and signal values it produced and the
    /// subcomponent input writes it made.
    fn eager_side(
        &mut self,
        body: &InstructionList,
        guard: usize,
        frame: &mut Frame,
    ) -> Result<Side> {
        self.journals.push(Vec::new());
        self.deferred_inputs.push(Vec::new());
        let outer_guard = self.guard.replace(guard);
        let result = self.execute(body, frame);
        self.guard = outer_guard;
        let inputs = self.deferred_inputs.pop().expect("the list pushed above");
        let journal = self.journals.pop().expect("the journal pushed above");
        let mut written = HashMap::with_capacity(journal.len());
        for signal in journal {
            if let Some(value) = self.signals[signal].take() {
                written.insert(signal, value);
            }
        }
        match result? {
            Flow::Continue => Ok(Side {
                variables: frame.variables.clone(),
                signals: written,
                inputs,
            }),
            Flow::Return(_) => bail!("a branch that depends on an input returns"),
        }
    }

    /// `divisor` where the current path is taken, 1 where it is not.
    pub(super) fn guarded_divisor(&mut self, divisor: usize) -> Result<usize> {
        let Some(guard) = self.guard else {
            return Ok(divisor);
        };
        if matches!(self.graph.known(divisor), Some(value) if value != U256::ZERO) {
            return Ok(divisor);
        }
        let one = self.graph.one;
        self.graph.select(guard, divisor, one)
    }
}

/// Whether a branch body only assigns variables and signals, so both sides can run
/// and be selected afterwards. Creating a component or returning cannot be undone.
fn executable_eagerly(body: &InstructionList) -> bool {
    body.iter().all(|instruction| match instruction.as_ref() {
        Instruction::Branch(branch) => {
            executable_eagerly(&branch.if_branch) && executable_eagerly(&branch.else_branch)
        }
        Instruction::Loop(body) => executable_eagerly(&body.body),
        Instruction::CreateCmp(_) | Instruction::Return(_) => false,
        Instruction::Store(_)
        | Instruction::Call(_)
        | Instruction::Value(_)
        | Instruction::Load(_)
        | Instruction::Compute(_)
        | Instruction::Assert(_)
        | Instruction::Log(_) => true,
    })
}
