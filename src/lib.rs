//! The postcard witness-graph format behind SIGNET artifacts: decoding a graph,
//! optimizing one, and computing a witness from named inputs.
//!
//! Graphs come from `curvy-signet-ir-builder` in this repository's `ir-builder/`, a
//! separate GPL program that compiles a circuit with circom and executes circom's
//! intermediate representation.

pub mod graph;

use std::collections::HashMap;

use ruint::{aliases::U256, uint};
use serde::{Deserialize, Serialize};

use crate::graph::Node;

pub const M: U256 =
    uint!(21888242871839275222246405745257275088548364400416034343698204186575808495617_U256);

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct HashSignalInfo {
    pub hash: u64,
    pub signalid: u64,
    pub signalsize: u64,
}

#[derive(Clone)]
pub struct Graph {
    pub nodes: Vec<Node>,
    pub signals: Vec<usize>,
    pub input_mapping: Vec<HashSignalInfo>,
}

fn fnv1a(s: &str) -> u64 {
    let mut hash: u64 = 0xCBF29CE484222325;
    for c in s.bytes() {
        hash ^= c as u64;
        hash = hash.wrapping_mul(0x100000001B3);
    }
    hash
}

/// Loads the graph from bytes
pub fn init_graph(graph_bytes: &[u8]) -> eyre::Result<Graph> {
    let (nodes, signals, input_mapping): (Vec<Node>, Vec<usize>, Vec<HashSignalInfo>) =
        postcard::from_bytes(graph_bytes)?;

    Ok(Graph {
        nodes,
        signals,
        input_mapping,
    })
}

/// The length of the input buffer `populate_inputs` fills: one past the last signal
/// any named input covers or any input node reads.
///
/// The input table is what callers write through, so it decides the size.
/// Optimization removes input nodes nothing reads, so counting nodes alone falls
/// short when the last inputs are unused, and input nodes need not come first.
pub fn get_inputs_size(graph: &Graph) -> usize {
    let named = graph
        .input_mapping
        .iter()
        .filter(|entry| entry.signalsize != 0)
        .map(|entry| {
            usize::try_from(entry.signalid.saturating_add(entry.signalsize)).unwrap_or(usize::MAX)
        });
    let read = graph.nodes.iter().filter_map(|node| match node {
        Node::Input(index) => Some(index.saturating_add(1)),
        _ => None,
    });
    named.chain(read).max().unwrap_or(1).max(1)
}

/// Allocates inputs vec with position 0 set to 1
pub fn get_inputs_buffer(size: usize) -> Vec<U256> {
    let mut inputs = vec![U256::ZERO; size];
    inputs[0] = U256::from(1);
    inputs
}

/// Calculates the position of the given signal in the inputs buffer.
///
/// Panics on a name the graph does not declare; `calculate_witness` checks names first.
pub fn get_input_mapping(input_list: &Vec<String>, graph: &Graph) -> HashMap<String, usize> {
    let mut input_mapping = HashMap::new();
    for key in input_list {
        let h = fnv1a(key);
        let pos = graph
            .input_mapping
            .iter()
            .position(|x| x.hash == h)
            .unwrap();
        let si = (graph.input_mapping[pos].signalid) as usize;
        input_mapping.insert(key.to_string(), si);
    }
    input_mapping
}

/// Sets all provided inputs given the mapping and inputs buffer.
///
/// Panics when a value runs past the end of the buffer; `calculate_witness` checks
/// lengths first.
pub fn populate_inputs(
    input_list: &HashMap<String, Vec<U256>>,
    input_mapping: &HashMap<String, usize>,
    input_buffer: &mut [U256],
) {
    for (key, value) in input_list {
        let start = input_mapping[key];
        let end = start + value.len();
        input_buffer[start..end].copy_from_slice(value);
    }
}

/// Calculate witness based on serialized graph and inputs
pub fn calculate_witness(
    input_list: HashMap<String, Vec<U256>>,
    graph: &Graph,
) -> eyre::Result<Vec<U256>> {
    for (name, values) in &input_list {
        let hash = fnv1a(name);
        let entry = graph
            .input_mapping
            .iter()
            .find(|entry| entry.hash == hash && entry.signalsize != 0)
            .ok_or_else(|| eyre::eyre!("the circuit has no input named {name}"))?;
        eyre::ensure!(
            values.len() as u64 <= entry.signalsize,
            "input {name} has {} values, the circuit declares {}",
            values.len(),
            entry.signalsize
        );
    }
    // A corrupt input table can name an absurd size; fail instead of aborting.
    let size = get_inputs_size(graph);
    let mut inputs_buffer = Vec::new();
    inputs_buffer
        .try_reserve_exact(size)
        .map_err(|_| eyre::eyre!("the graph asks for a {size}-entry input buffer"))?;
    inputs_buffer.resize(size, U256::ZERO);
    inputs_buffer[0] = U256::from(1);
    let input_mapping = get_input_mapping(&input_list.keys().cloned().collect(), graph);
    populate_inputs(&input_list, &input_mapping, &mut inputs_buffer);
    graph::evaluate(&graph.nodes, &inputs_buffer, &graph.signals)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One three-signal input `a` at signals 1..4 of which only the first is read,
    /// as optimization leaves a graph whose last inputs are unused.
    fn graph_with_unused_trailing_inputs() -> Graph {
        Graph {
            nodes: vec![Node::Constant(U256::from(9)), Node::Input(1)],
            signals: vec![1],
            input_mapping: vec![
                HashSignalInfo::default(),
                HashSignalInfo {
                    hash: fnv1a("a"),
                    signalid: 1,
                    signalsize: 3,
                },
            ],
        }
    }

    #[test]
    fn input_buffer_covers_inputs_no_node_reads() {
        let graph = graph_with_unused_trailing_inputs();
        assert_eq!(get_inputs_size(&graph), 4);

        let inputs = HashMap::from([(
            "a".to_owned(),
            vec![U256::from(5), U256::from(6), U256::from(7)],
        )]);
        assert_eq!(
            calculate_witness(inputs, &graph).unwrap(),
            vec![U256::from(5)]
        );
    }

    #[test]
    fn input_buffer_covers_input_nodes_outside_the_table() {
        let mut graph = graph_with_unused_trailing_inputs();
        graph.nodes.push(Node::Input(6));
        assert_eq!(get_inputs_size(&graph), 7);
    }

    #[test]
    fn unknown_and_over_long_inputs_are_errors() {
        let graph = graph_with_unused_trailing_inputs();
        let unknown = HashMap::from([("b".to_owned(), vec![U256::from(1)])]);
        let error = calculate_witness(unknown, &graph).unwrap_err().to_string();
        assert!(error.contains("no input named b"), "{error}");
        let long = HashMap::from([("a".to_owned(), vec![U256::from(1); 4])]);
        let error = calculate_witness(long, &graph).unwrap_err().to_string();
        assert!(
            error.contains("has 4 values, the circuit declares 3"),
            "{error}"
        );
    }

    #[test]
    fn an_absurd_input_table_is_an_error() {
        let mut graph = graph_with_unused_trailing_inputs();
        graph.input_mapping[1].signalsize = u64::MAX;
        assert_eq!(get_inputs_size(&graph), usize::MAX);
        let inputs = HashMap::from([("a".to_owned(), vec![U256::from(5)])]);
        assert!(calculate_witness(inputs, &graph).is_err());
    }

    #[test]
    fn propagation_leaves_operations_that_cannot_be_evaluated() {
        use graph::Operation;
        let mut nodes = vec![
            Node::Constant(U256::from(5)),
            Node::Constant(U256::ZERO),
            Node::Op(Operation::Div, 0, 1),
            Node::Op(Operation::IDiv, 0, 1),
            Node::Op(Operation::Mod, 0, 1),
            Node::Constant(U256::from(300)),
            Node::Op(Operation::Shl, 0, 5),
            Node::Op(Operation::Add, 0, 1),
        ];
        graph::propagate(&mut nodes);
        assert!(matches!(nodes[2], Node::Op(Operation::Div, ..)));
        assert!(matches!(nodes[3], Node::Op(Operation::IDiv, ..)));
        assert!(matches!(nodes[4], Node::Op(Operation::Mod, ..)));
        assert!(matches!(nodes[6], Node::Op(Operation::Shl, ..)));
        assert_eq!(nodes[7], Node::Constant(U256::from(5)));
    }

    #[test]
    fn evaluation_failures_are_errors() {
        use graph::Operation;
        let cases = [
            (Operation::Div, U256::ZERO, "division or modulus by zero"),
            (Operation::IDiv, U256::ZERO, "division or modulus by zero"),
            (Operation::Mod, U256::ZERO, "division or modulus by zero"),
            (
                Operation::Shl,
                U256::from(256),
                "shift amount outside 0..256",
            ),
            (
                Operation::Shr,
                M - U256::from(1),
                "shift amount outside 0..256",
            ),
        ];
        for (op, b, reason) in cases {
            let nodes = vec![Node::Input(0), Node::Input(1), Node::Op(op, 0, 1)];
            let error = graph::evaluate(&nodes, &[U256::from(5), b], &[2])
                .unwrap_err()
                .to_string();
            assert_eq!(error, format!("{reason} at graph node 2"), "{op:?}");
        }
    }

    #[test]
    fn an_input_outside_the_buffer_is_an_error() {
        let graph = graph_with_unused_trailing_inputs();
        let error = graph::evaluate(&graph.nodes, &[U256::from(1)], &graph.signals)
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("outside the 1-entry input buffer"),
            "{error}"
        );
    }
}
