//! Immutable execution indices. Connection vectors retain authored order.
use crate::graph::{Edge, Graph, Node};
use std::collections::HashMap;
pub struct PreparedGraph {
    graph: Graph,
    nodes: HashMap<String, usize>,
    inputs: HashMap<(String, String), usize>,
    outputs: HashMap<(String, String), Vec<usize>>,
}
impl PreparedGraph {
    pub fn new(graph: Graph) -> Self {
        crate::metrics::count(|c| c.graph_nodes_copied += graph.nodes.len() as u64);
        let nodes = graph
            .nodes
            .iter()
            .enumerate()
            .map(|(i, n)| (n.id.clone(), i))
            .collect();
        let mut inputs = HashMap::new();
        let mut outputs: HashMap<_, Vec<_>> = HashMap::new();
        for (i, e) in graph.edges.iter().enumerate() {
            inputs
                .entry((e.to_node.clone(), e.to_port.clone()))
                .or_insert(i);
            outputs
                .entry((e.from_node.clone(), e.from_port.clone()))
                .or_default()
                .push(i);
        }
        Self {
            graph,
            nodes,
            inputs,
            outputs,
        }
    }
    pub fn node(&self, id: &str) -> Option<&Node> {
        self.nodes.get(id).map(|&i| &self.graph.nodes[i])
    }
    pub fn input(&self, node: &str, port: &str) -> Option<&Edge> {
        self.inputs
            .get(&(node.into(), port.into()))
            .map(|&i| &self.graph.edges[i])
    }
    pub fn outputs(&self, node: &str, port: &str) -> impl Iterator<Item = &Edge> {
        self.outputs
            .get(&(node.into(), port.into()))
            .into_iter()
            .flatten()
            .map(|&i| &self.graph.edges[i])
    }
}
