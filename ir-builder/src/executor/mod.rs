// SPDX-License-Identifier: GPL-3.0-only
// SPDX-FileCopyrightText: 2026 Curvy Protocol d.o.o.
//! Symbolic execution of circom's intermediate representation.
//!
//! The executor follows the C++ circom generates for the same circuit: `component.rs`
//! creates and runs components, `stmt.rs` runs statements, `expr.rs` evaluates
//! expressions, `memory.rs` resolves and accesses variables and signals, `call.rs`
//! runs functions and `branch.rs` runs branches whose condition depends on an input.

mod branch;
mod call;
mod component;
mod expr;
mod memory;
mod stmt;

use circom_code_producers::c_elements::CProducer;
use circom_compiler::circuit_design::function::FunctionCodeInfo;
use circom_compiler::compiler_interface::Circuit;
use curvy_signet_builder::graph::Node;
use eyre::{bail, eyre, Result, WrapErr};
use smallvec::SmallVec;

use crate::compile::parse_field_constant;
use crate::nodes::{FastMap, NodeBuilder};

use component::{Component, InputWrite};

/// The nodes of one value. Almost every value is a single node, so one is stored inline.
type Nodes = SmallVec<[usize; 1]>;

/// Stack for the executor thread. Templates and functions run recursively.
const EXECUTOR_STACK_BYTES: usize = 1 << 30;

/// Runs the executor on a thread with room for deeply nested templates and functions,
/// returning the graph's nodes and the node of each witness signal.
pub(crate) fn execute(circuit: &Circuit) -> Result<(Vec<Node>, Vec<usize>)> {
    std::thread::scope(|scope| {
        std::thread::Builder::new()
            .name("circom-ir-executor".to_owned())
            .stack_size(EXECUTOR_STACK_BYTES)
            .spawn_scoped(scope, || Executor::new(circuit)?.run())
            .wrap_err("could not start the executor thread")?
            .join()
            .map_err(|_| eyre!("the executor panicked"))?
    })
}

struct Frame {
    component: usize,
    variables: Vec<usize>,
    /// The function being executed and the number of values its caller takes, or
    /// `None` in a template body.
    function: Option<(String, usize)>,
}

enum Flow {
    Continue,
    Return(Vec<usize>),
}

enum Value {
    Address(usize),
    Field {
        nodes: Nodes,
        /// The subcomponent the values were read from, which sizes multi-template accesses.
        component: Option<usize>,
    },
}

impl Value {
    fn address(self) -> Result<usize> {
        match self {
            Self::Address(address) => Ok(address),
            Self::Field { .. } => bail!("expected an address, found a field value"),
        }
    }

    fn field(self) -> Result<(Nodes, Option<usize>)> {
        match self {
            Self::Field { nodes, component } => Ok((nodes, component)),
            Self::Address(_) => bail!("expected a field value, found an address"),
        }
    }
}

struct Executor<'a> {
    circuit: &'a Circuit,
    graph: NodeBuilder,
    /// Node for each entry of circom's constant table.
    field_constants: Vec<usize>,
    /// Functions by header, the symbol a call names.
    functions: FastMap<&'a str, &'a FunctionCodeInfo>,
    signals: Vec<Option<usize>>,
    components: Vec<Option<Component>>,
    /// Signals written by each eager branch side being executed, innermost last.
    journals: Vec<Vec<usize>>,
    /// Subcomponent input writes of each eager branch side being executed, counted
    /// once both sides are merged.
    deferred_inputs: Vec<Vec<InputWrite>>,
    /// A 0/1 node that is 1 exactly when the code being executed is the path taken,
    /// while an eager branch side runs.
    guard: Option<usize>,
}

impl<'a> Executor<'a> {
    fn new(circuit: &'a Circuit) -> Result<Self> {
        let producer = &circuit.c_producer;
        if producer.prime_str != "bn128" {
            bail!(
                "only the bn128 field is supported, not {}",
                producer.prime_str
            );
        }
        let mut graph = NodeBuilder::new();
        let field_constants = producer
            .get_field_constant_list()
            .iter()
            .map(|value| parse_field_constant(value).map(|value| graph.constant(value)))
            .collect::<Result<_>>()?;
        let functions = circuit
            .functions
            .iter()
            .map(|function| (function.header.as_str(), &**function))
            .collect();
        let mut executor = Self {
            circuit,
            graph,
            field_constants,
            functions,
            signals: vec![None; producer.total_number_of_signals],
            components: vec![None; producer.number_of_components],
            journals: Vec::new(),
            deferred_inputs: Vec::new(),
            guard: None,
        };

        *executor
            .signals
            .first_mut()
            .ok_or_else(|| eyre!("the circuit has no signals"))? = Some(executor.graph.one);
        let inputs_start = producer.get_number_of_main_outputs();
        let inputs_end = inputs_start + producer.get_number_of_main_inputs();
        if inputs_end > executor.signals.len() {
            bail!("main inputs lie outside the signal table");
        }
        for signal in inputs_start..inputs_end {
            let node = executor.graph.push(Node::Input(signal));
            executor.signals[signal] = Some(node);
        }
        Ok(executor)
    }

    fn run(mut self) -> Result<(Vec<Node>, Vec<usize>)> {
        let circuit = self.circuit;
        let producer: &CProducer = &circuit.c_producer;
        let main = circuit
            .templates
            .iter()
            .find(|template| template.header == producer.main_header)
            .ok_or_else(|| eyre!("the main template {} is missing", producer.main_header))?;
        if main.is_extern_c {
            bail!(
                "extern_c template {} has no circom body to execute",
                main.header
            );
        }
        // The generated `run` creates the main component and runs it directly: its
        // inputs come from the caller, not from input writes.
        *self
            .components
            .first_mut()
            .ok_or_else(|| eyre!("the circuit has no components"))? = Some(Component {
            template_id: main.id,
            signal_start: producer.get_main_signal_offset(),
            inputs_left: 0,
            subcomponents: vec![None; main.number_of_components],
            ran: false,
        });
        self.run_template(0)?;

        for (id, component) in self.components.iter().enumerate() {
            if let Some(component) = component {
                if !component.ran {
                    let template = circuit.get_template(component.template_id);
                    bail!(
                        "component {id} ({}) was created but never ran: {} of its inputs were never written",
                        template.header,
                        component.inputs_left
                    );
                }
            }
        }
        let outputs = producer
            .get_witness_to_signal_list()
            .iter()
            .map(|&signal| {
                self.signals
                    .get(signal)
                    .copied()
                    .flatten()
                    .ok_or_else(|| eyre!("witness signal {signal} was never assigned"))
            })
            .collect::<Result<Vec<_>>>()?;
        Ok((self.graph.nodes, outputs))
    }
}

fn first(value: Value) -> Result<usize> {
    match value {
        Value::Field { nodes, .. } => nodes
            .first()
            .copied()
            .ok_or_else(|| eyre!("an empty operand")),
        Value::Address(_) => bail!("expected a field value, found an address"),
    }
}

fn checked_offset(base: usize, offset: usize) -> Result<usize> {
    base.checked_add(offset)
        .ok_or_else(|| eyre!("a memory offset overflows"))
}
