// SPDX-License-Identifier: GPL-3.0-only
// SPDX-FileCopyrightText: 2026 Curvy Protocol d.o.o.
//! Resolving locations to variables and signals, and reading and writing them.

use circom_compiler::intermediate_representation::ir_interface::{
    AccessType, AddressType, LocationRule, SizeOption,
};
use eyre::{bail, eyre, Result};

use super::{checked_offset, Executor, Frame, Nodes};

enum Memory {
    Variables,
    Signals,
}

pub(super) struct Place {
    memory: Memory,
    offset: usize,
    pub component: Option<usize>,
}

impl Place {
    pub fn is_variables(&self) -> bool {
        matches!(self.memory, Memory::Variables)
    }
}

impl Executor<'_> {
    pub(super) fn place(
        &mut self,
        address_type: &AddressType,
        location: &LocationRule,
        frame: &mut Frame,
    ) -> Result<Place> {
        let component = match address_type {
            AddressType::SubcmpSignal { cmp_address, .. } => {
                Some(self.subcomponent(cmp_address, frame)?)
            }
            AddressType::Variable | AddressType::Signal => None,
        };
        let offset = match location {
            LocationRule::Indexed { location, .. } => self.address(location, frame)?,
            LocationRule::Mapped {
                signal_code,
                indexes,
            } => {
                let component =
                    component.ok_or_else(|| eyre!("a mapped location outside a subcomponent"))?;
                self.mapped_offset(component, *signal_code, indexes, frame)?
            }
        };
        Ok(match address_type {
            AddressType::Variable => Place {
                memory: Memory::Variables,
                offset,
                component: None,
            },
            AddressType::Signal => Place {
                memory: Memory::Signals,
                offset: checked_offset(self.component(frame.component)?.signal_start, offset)?,
                component: None,
            },
            AddressType::SubcmpSignal { .. } => {
                let component = component.expect("resolved above");
                Place {
                    memory: Memory::Signals,
                    offset: checked_offset(self.component(component)?.signal_start, offset)?,
                    component: Some(component),
                }
            }
        })
    }

    /// The offset of a signal of a subcomponent whose template is chosen at run time,
    /// computed as the generated code does from the template's IO table. The table
    /// circom writes for that code omits the first dimension of each array, so
    /// dimension `k` of the access is scaled by the length of dimension `k`.
    fn mapped_offset(
        &mut self,
        component: usize,
        signal_code: usize,
        accesses: &[AccessType],
        frame: &mut Frame,
    ) -> Result<usize> {
        let circuit = self.circuit;
        let producer = &circuit.c_producer;
        let template_id = self.component(component)?.template_id;
        let io = producer
            .get_io_map()
            .get(&template_id)
            .and_then(|signals| signals.get(signal_code))
            .ok_or_else(|| eyre!("template {template_id} has no IO signal {signal_code}"))?;
        let mut dimensions: &[usize] = &io.lengths;
        let mut element_size = io.size;
        let mut bus = io.bus_id;
        let mut offset = io.offset;
        for (position, access) in accesses.iter().enumerate() {
            match access {
                AccessType::Indexed(info) => {
                    if info.indexes.is_empty()
                        || info.indexes.len() > info.symbol_dim
                        || info.symbol_dim != dimensions.len()
                    {
                        bail!("an IO access does not match the signal's dimensions");
                    }
                    if info.indexes.len() < info.symbol_dim && position + 1 != accesses.len() {
                        bail!("a partial IO access is not the last access");
                    }
                    let mut index = self.address(&info.indexes[0], frame)?;
                    for (dimension, expression) in info.indexes.iter().enumerate().skip(1) {
                        let next = self.address(expression, frame)?;
                        index = index
                            .checked_mul(dimensions[dimension])
                            .and_then(|index| index.checked_add(next))
                            .ok_or_else(|| eyre!("an IO index overflows"))?;
                    }
                    for &length in &dimensions[info.indexes.len()..] {
                        index = index
                            .checked_mul(length)
                            .ok_or_else(|| eyre!("an IO index overflows"))?;
                    }
                    let scaled = index
                        .checked_mul(element_size)
                        .ok_or_else(|| eyre!("an IO offset overflows"))?;
                    offset = checked_offset(offset, scaled)?;
                }
                AccessType::Qualified(field) => {
                    let bus_id =
                        bus.ok_or_else(|| eyre!("a field access on a signal that is not a bus"))?;
                    let field = producer
                        .get_busid_field_info()
                        .get(bus_id)
                        .and_then(|fields| fields.get(*field))
                        .ok_or_else(|| eyre!("bus {bus_id} has no field {field}"))?;
                    offset = checked_offset(offset, field.offset)?;
                    dimensions = &field.dimensions;
                    element_size = field.size;
                    bus = field.bus_id;
                }
            }
        }
        Ok(offset)
    }

    /// Reads `size` values at `place`. In an eager branch side, a read outside the
    /// variables or signals cannot be resolved at build time; the values it yields make
    /// witness generation fail where that side is taken and are 0 where it is not.
    pub(super) fn read(&mut self, place: &Place, size: usize, frame: &Frame) -> Result<Nodes> {
        let end = checked_offset(place.offset, size)?;
        let in_range = match place.memory {
            Memory::Variables => end <= frame.variables.len(),
            Memory::Signals => end <= self.signals.len(),
        };
        if !in_range {
            let kind = match place.memory {
                Memory::Variables => "variable",
                Memory::Signals => "signal",
            };
            let Some(guard) = self.guard else {
                bail!("a {kind} read is out of range");
            };
            let failure = self.graph.fail_where(guard)?;
            return Ok(Nodes::from_elem(failure, size));
        }
        match place.memory {
            Memory::Variables => Ok(Nodes::from_slice(&frame.variables[place.offset..end])),
            Memory::Signals => self.signals[place.offset..end]
                .iter()
                .enumerate()
                .map(|(index, value)| {
                    value.ok_or_else(|| {
                        eyre!(
                            "signal {} is read before it is assigned",
                            place.offset + index
                        )
                    })
                })
                .collect(),
        }
    }

    pub(super) fn write(
        &mut self,
        place: &Place,
        values: &[usize],
        frame: &mut Frame,
    ) -> Result<()> {
        match place.memory {
            Memory::Variables => {
                let end = checked_offset(place.offset, values.len())?;
                frame
                    .variables
                    .get_mut(place.offset..end)
                    .ok_or_else(|| eyre!("a variable write is out of range"))?
                    .copy_from_slice(values);
            }
            Memory::Signals => {
                for (index, &value) in values.iter().enumerate() {
                    self.write_signal(place.offset + index, value)?;
                }
            }
        }
        Ok(())
    }

    pub(super) fn write_signal(&mut self, signal: usize, value: usize) -> Result<()> {
        let slot = self
            .signals
            .get_mut(signal)
            .ok_or_else(|| eyre!("signal {signal} is out of range"))?;
        if slot.is_some() {
            bail!("signal {signal} is assigned twice");
        }
        *slot = Some(value);
        if let Some(journal) = self.journals.last_mut() {
            journal.push(signal);
        }
        Ok(())
    }

    pub(super) fn size(&self, size: &SizeOption, component: Option<usize>) -> Result<usize> {
        match size {
            SizeOption::Single(size) => Ok(*size),
            SizeOption::Multiple(sizes) => {
                let component = component
                    .ok_or_else(|| eyre!("a size depends on a component that is not known"))?;
                let template_id = self.component(component)?.template_id;
                sizes
                    .iter()
                    .find(|(candidate, _)| *candidate == template_id)
                    .map(|(_, size)| *size)
                    .ok_or_else(|| eyre!("no size is given for template {template_id}"))
            }
        }
    }
}
