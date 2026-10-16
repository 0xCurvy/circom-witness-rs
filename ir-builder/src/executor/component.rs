// SPDX-License-Identifier: GPL-3.0-only
// SPDX-FileCopyrightText: 2026 Curvy Protocol d.o.o.
//! Creating components, counting their input writes and running their templates.

use circom_compiler::intermediate_representation::ir_interface::{
    AddressType, CreateCmpBucket, InputInformation, Instruction, StatusInput,
};
use eyre::{bail, eyre, Result, WrapErr};

use super::{Executor, Flow, Frame};

/// A write to a subcomponent's inputs: `size` values, and whether circom expects it
/// to be the last (`None` when circom does not know).
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) struct InputWrite {
    pub component: usize,
    pub size: usize,
    pub last: Option<bool>,
}

#[derive(Clone)]
pub(super) struct Component {
    pub template_id: usize,
    pub signal_start: usize,
    /// Input signals not yet written, as circom's generated code counts them.
    pub inputs_left: usize,
    pub subcomponents: Vec<Option<usize>>,
    pub ran: bool,
}

impl Executor<'_> {
    pub(super) fn run_template(&mut self, id: usize) -> Result<()> {
        let circuit = self.circuit;
        let component = self.component_mut(id)?;
        if component.ran {
            bail!("component {id} would run twice");
        }
        component.ran = true;
        let template = circuit.get_template(component.template_id);
        let mut frame = Frame {
            component: id,
            variables: vec![self.graph.zero; template.var_stack_depth],
            function: None,
        };
        match self
            .execute(&template.body, &mut frame)
            .wrap_err_with(|| format!("in template {} (component {id})", template.header))?
        {
            Flow::Continue => Ok(()),
            Flow::Return(_) => bail!("template {} returned a value", template.header),
        }
    }

    pub(super) fn create(&mut self, create: &CreateCmpBucket, frame: &mut Frame) -> Result<()> {
        let circuit = self.circuit;
        let template = circuit.get_template(create.template_id);
        if template.is_extern_c {
            bail!(
                "extern_c template {} has no circom body to execute",
                template.header
            );
        }
        let first_slot = self.address(&create.sub_cmp_id, frame)?;
        let parent_signal_start = self.component(frame.component)?.signal_start;
        for (created, &(position, _)) in create.defined_positions.iter().enumerate() {
            let id = frame.component
                + create.component_offset
                + 1
                + created * create.component_offset_jump;
            let signal_start =
                parent_signal_start + create.signal_offset + created * create.signal_offset_jump;
            let slot = first_slot + position;
            let parent = self.component_mut(frame.component)?;
            let entry = parent
                .subcomponents
                .get_mut(slot)
                .ok_or_else(|| eyre!("subcomponent slot {slot} is out of range"))?;
            if entry.is_some() {
                bail!("subcomponent slot {slot} is created twice");
            }
            *entry = Some(id);
            let target = self
                .components
                .get_mut(id)
                .ok_or_else(|| eyre!("component {id} is out of range"))?;
            if target.is_some() {
                bail!("component {id} is created twice");
            }
            *target = Some(Component {
                template_id: create.template_id,
                signal_start,
                inputs_left: template.number_of_inputs,
                subcomponents: vec![None; template.number_of_components],
                ran: false,
            });
            if !create.has_inputs {
                self.run_template(id)?;
            }
        }
        Ok(())
    }

    /// Counts a write to a subcomponent's inputs and runs it when the last one lands,
    /// checking circom's own expectation of whether this write was the last. Inside
    /// an eager branch side the count waits until both sides are merged.
    pub(super) fn count_inputs(
        &mut self,
        address_type: &AddressType,
        component: Option<usize>,
        size: usize,
    ) -> Result<()> {
        let AddressType::SubcmpSignal {
            input_information: InputInformation::Input { status, .. },
            ..
        } = address_type
        else {
            return Ok(());
        };
        let write = InputWrite {
            component: component.expect("a subcomponent write has a component"),
            size,
            last: match status {
                StatusInput::Last => Some(true),
                StatusInput::NoLast => Some(false),
                StatusInput::Unknown => None,
            },
        };
        match self.deferred_inputs.last_mut() {
            Some(deferred) => {
                deferred.push(write);
                Ok(())
            }
            None => self.apply_input_write(write),
        }
    }

    pub(super) fn apply_input_write(&mut self, write: InputWrite) -> Result<()> {
        let InputWrite {
            component: id,
            size,
            last,
        } = write;
        let state = self.component_mut(id)?;
        state.inputs_left = state
            .inputs_left
            .checked_sub(size)
            .ok_or_else(|| eyre!("component {id} receives more input values than it declares"))?;
        let complete = state.inputs_left == 0;
        match last {
            Some(true) if !complete => {
                bail!("circom expects component {id}'s inputs to be complete, but some are missing")
            }
            Some(false) if complete => {
                bail!("circom expects more inputs for component {id}, but all are written")
            }
            _ => {}
        }
        if complete {
            self.run_template(id)?;
        }
        Ok(())
    }

    pub(super) fn subcomponent(
        &mut self,
        address: &Instruction,
        frame: &mut Frame,
    ) -> Result<usize> {
        let slot = self.address(address, frame)?;
        self.component(frame.component)?
            .subcomponents
            .get(slot)
            .copied()
            .flatten()
            .ok_or_else(|| eyre!("subcomponent slot {slot} is used before it is created"))
    }

    pub(super) fn component(&self, id: usize) -> Result<&Component> {
        self.components
            .get(id)
            .and_then(Option::as_ref)
            .ok_or_else(|| eyre!("component {id} does not exist"))
    }

    pub(super) fn component_mut(&mut self, id: usize) -> Result<&mut Component> {
        self.components
            .get_mut(id)
            .and_then(Option::as_mut)
            .ok_or_else(|| eyre!("component {id} does not exist"))
    }
}
