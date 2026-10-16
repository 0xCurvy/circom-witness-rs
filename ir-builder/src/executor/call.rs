// SPDX-License-Identifier: GPL-3.0-only
// SPDX-FileCopyrightText: 2026 Curvy Protocol d.o.o.
//! Function calls.

use circom_compiler::intermediate_representation::ir_interface::{CallBucket, ReturnType};
use eyre::{bail, eyre, Result, WrapErr};
use smallvec::smallvec;

use super::{Executor, Flow, Frame, Value};

impl Executor<'_> {
    /// Executes a call. An intermediate call returns its value; a final call stores it.
    pub(super) fn call(&mut self, call: &CallBucket, frame: &mut Frame) -> Result<Option<Value>> {
        let mut arguments = Vec::new();
        for (argument, context) in call.arguments.iter().zip(&call.argument_types) {
            let (values, component) = self.eval(argument, frame)?.field()?;
            let size = self.size(&context.size, component)?;
            if values.len() < size {
                bail!(
                    "an argument of {} has fewer than {size} values",
                    call.symbol
                );
            }
            arguments.extend_from_slice(&values[..size]);
        }
        match &call.return_info {
            ReturnType::Intermediate { .. } => {
                let values = self.run_function(call, arguments, frame.component, 1)?;
                let node = *values
                    .first()
                    .ok_or_else(|| eyre!("{} returned no value", call.symbol))?;
                Ok(Some(Value::Field {
                    nodes: smallvec![node],
                    component: None,
                }))
            }
            ReturnType::Final(data) => {
                let place = self.place(&data.dest_address_type, &data.dest, frame)?;
                let size = self.size(&data.context.size, place.component)?;
                let values = self.run_function(call, arguments, frame.component, size)?;
                // A function may return fewer values than its destination holds. The
                // generated code then copies what was returned and leaves the rest of a
                // variable as it was; a signal left unassigned would be read as garbage.
                if values.len() < size && !place.is_variables() {
                    bail!(
                        "{} returned {} values, {size} are stored in signals",
                        call.symbol,
                        values.len()
                    );
                }
                let size = size.min(values.len());
                self.write(&place, &values[..size], frame)?;
                self.count_inputs(&data.dest_address_type, place.component, size)?;
                Ok(None)
            }
        }
    }

    fn run_function(
        &mut self,
        call: &CallBucket,
        arguments: Vec<usize>,
        component: usize,
        result_size: usize,
    ) -> Result<Vec<usize>> {
        let function = *self
            .functions
            .get(call.symbol.as_str())
            .ok_or_else(|| eyre!("function {} is missing", call.symbol))?;
        let mut variables = vec![self.graph.zero; call.arena_size.max(function.max_number_of_vars)];
        if arguments.len() > variables.len() {
            bail!("the arguments of {} do not fit its variables", call.symbol);
        }
        variables[..arguments.len()].copy_from_slice(&arguments);
        let mut frame = Frame {
            component,
            variables,
            function: Some((call.symbol.clone(), result_size)),
        };
        match self
            .execute(&function.body, &mut frame)
            .wrap_err_with(|| format!("in function {}", call.symbol))?
        {
            Flow::Return(values) => Ok(values),
            Flow::Continue => bail!("function {} ended without returning", call.symbol),
        }
    }
}
