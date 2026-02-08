//! Knowledge graph backend using petgraph.
//! Phase 1 stub — full implementation in Phase 2.

use petgraph::graph::DiGraph;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// A node in the knowledge graph.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphNode {
    pub id: String,
    pub label: String,
    pub node_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Value>,
    pub doc_ids: Vec<i64>,
}

/// An edge in the knowledge graph.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphEdge {
    pub source: String,
    pub target: String,
    pub relationship: String,
    pub weight: f64,
}

/// In-memory knowledge graph built from document metadata.
pub struct GraphBackend {
    graph: DiGraph<GraphNode, GraphEdge>,
    node_index: HashMap<String, petgraph::graph::NodeIndex>,
}

impl GraphBackend {
    pub fn new() -> Self {
        Self {
            graph: DiGraph::new(),
            node_index: HashMap::new(),
        }
    }

    /// Get graph statistics.
    pub fn stats(&self) -> GraphStats {
        GraphStats {
            node_count: self.graph.node_count(),
            edge_count: self.graph.edge_count(),
        }
    }
}

impl Default for GraphBackend {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphStats {
    pub node_count: usize,
    pub edge_count: usize,
}
