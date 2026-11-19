//! Server-side reusable artifacts and DAG workflow dependency orchestrator.
//!
//! Supports map/reduce dependency graphs, topological cycle rejection,
//! total pipeline CPT budget enforcement, cancellation propagation to
//! downstream nodes, and checkpoint-based resume.

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet, VecDeque};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DagNodeState {
    Pending,
    Ready,
    Running,
    Completed,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReusableArtifact {
    pub artifact_id: String,
    pub owner: String,
    pub sha256: String,
    pub size_bytes: u64,
    pub media_type: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DagNodeSpec {
    pub node_id: String,
    pub kind: String, // "map", "reduce", "wasm", "hcode", "llm"
    pub depends_on: Vec<String>,
    pub max_cpt: i64,
    pub input_artifact_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DagNodeStatus {
    pub spec: DagNodeSpec,
    pub state: DagNodeState,
    pub billed_cpt: i64,
    pub output_artifact_id: Option<String>,
    pub checkpoint_json: String,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum DagError {
    #[error("DAG pipeline must contain at least one node")]
    EmptyPipeline,
    #[error("duplicate node id `{0}`")]
    DuplicateNode(String),
    #[error("node `{node}` depends on unknown node `{dep}`")]
    UnknownDependency { node: String, dep: String },
    #[error("DAG contains a dependency cycle")]
    CycleDetected,
    #[error("budget exceeded: spent {spent} + reserved {reserved} + next {next} > cap {cap}")]
    BudgetExceeded {
        spent: i64,
        reserved: i64,
        next: i64,
        cap: i64,
    },
    #[error("node `{0}` not found")]
    NodeNotFound(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DagPipeline {
    pub pipeline_id: String,
    pub owner: String,
    pub name: String,
    pub budget_cpt: i64,
    pub spent_cpt: i64,
    pub cancelled: bool,
    pub nodes: HashMap<String, DagNodeStatus>,
    pub artifacts: HashMap<String, ReusableArtifact>,
}

impl DagPipeline {
    pub fn new(
        pipeline_id: impl Into<String>,
        owner: impl Into<String>,
        name: impl Into<String>,
        budget_cpt: i64,
        specs: Vec<DagNodeSpec>,
    ) -> Result<Self, DagError> {
        if specs.is_empty() {
            return Err(DagError::EmptyPipeline);
        }
        let mut nodes = HashMap::with_capacity(specs.len());
        for spec in specs {
            if nodes.contains_key(&spec.node_id) {
                return Err(DagError::DuplicateNode(spec.node_id));
            }
            nodes.insert(
                spec.node_id.clone(),
                DagNodeStatus {
                    spec,
                    state: DagNodeState::Pending,
                    billed_cpt: 0,
                    output_artifact_id: None,
                    checkpoint_json: "{}".into(),
                },
            );
        }

        // Validate dependencies & cycle freedom via Kahn's algorithm.
        let mut indegree: HashMap<String, usize> = HashMap::new();
        let mut dependents: HashMap<String, Vec<String>> = HashMap::new();
        for (id, node) in &nodes {
            indegree.entry(id.clone()).or_insert(0);
            for dep in &node.spec.depends_on {
                if !nodes.contains_key(dep) {
                    return Err(DagError::UnknownDependency {
                        node: id.clone(),
                        dep: dep.clone(),
                    });
                }
                *indegree.entry(id.clone()).or_insert(0) += 1;
                dependents.entry(dep.clone()).or_default().push(id.clone());
            }
        }
        let mut queue: VecDeque<String> = indegree
            .iter()
            .filter(|(_, &deg)| deg == 0)
            .map(|(id, _)| id.clone())
            .collect();
        let mut visited = 0usize;
        while let Some(id) = queue.pop_front() {
            visited += 1;
            if let Some(children) = dependents.get(&id) {
                for child in children {
                    if let Some(deg) = indegree.get_mut(child) {
                        *deg -= 1;
                        if *deg == 0 {
                            queue.push_back(child.clone());
                        }
                    }
                }
            }
        }
        if visited != nodes.len() {
            return Err(DagError::CycleDetected);
        }

        let mut pipeline = Self {
            pipeline_id: pipeline_id.into(),
            owner: owner.into(),
            name: name.into(),
            budget_cpt: budget_cpt.max(0),
            spent_cpt: 0,
            cancelled: false,
            nodes,
            artifacts: HashMap::new(),
        };
        pipeline.refresh_ready_nodes();
        Ok(pipeline)
    }

    pub fn register_artifact(&mut self, artifact: ReusableArtifact) {
        self.artifacts
            .insert(artifact.artifact_id.clone(), artifact);
    }

    #[must_use]
    pub fn reserved_cpt(&self) -> i64 {
        self.nodes
            .values()
            .filter(|n| n.state == DagNodeState::Running)
            .map(|n| n.spec.max_cpt.max(0))
            .sum()
    }

    /// Marks pending nodes whose dependencies are all `Completed` as `Ready`.
    pub fn refresh_ready_nodes(&mut self) {
        if self.cancelled {
            return;
        }
        let completed: HashSet<String> = self
            .nodes
            .iter()
            .filter(|(_, n)| n.state == DagNodeState::Completed)
            .map(|(id, _)| id.clone())
            .collect();
        for node in self.nodes.values_mut() {
            if node.state == DagNodeState::Pending
                && node.spec.depends_on.iter().all(|d| completed.contains(d))
            {
                node.state = DagNodeState::Ready;
            }
        }
    }

    /// Returns all currently runnable node IDs that fit within the remaining budget cap.
    #[must_use]
    pub fn next_dispatchable_nodes(&self) -> Vec<String> {
        if self.cancelled {
            return Vec::new();
        }
        let mut available = self
            .budget_cpt
            .saturating_sub(self.spent_cpt)
            .saturating_sub(self.reserved_cpt());
        let mut ready: Vec<&DagNodeStatus> = self
            .nodes
            .values()
            .filter(|n| n.state == DagNodeState::Ready)
            .collect();
        ready.sort_by(|a, b| a.spec.node_id.cmp(&b.spec.node_id));
        let mut picked = Vec::new();
        for n in ready {
            let cost = n.spec.max_cpt.max(0);
            if cost <= available {
                available = available.saturating_sub(cost);
                picked.push(n.spec.node_id.clone());
            }
        }
        picked
    }

    pub fn start_node(&mut self, node_id: &str) -> Result<(), DagError> {
        let reserved = self.reserved_cpt();
        let max_cpt = self
            .nodes
            .get(node_id)
            .ok_or_else(|| DagError::NodeNotFound(node_id.to_owned()))?
            .spec
            .max_cpt
            .max(0);
        if self
            .spent_cpt
            .saturating_add(reserved)
            .saturating_add(max_cpt)
            > self.budget_cpt
        {
            return Err(DagError::BudgetExceeded {
                spent: self.spent_cpt,
                reserved,
                next: max_cpt,
                cap: self.budget_cpt,
            });
        }
        let node = self
            .nodes
            .get_mut(node_id)
            .ok_or_else(|| DagError::NodeNotFound(node_id.to_owned()))?;
        node.state = DagNodeState::Running;
        Ok(())
    }

    pub fn complete_node(
        &mut self,
        node_id: &str,
        billed_cpt: i64,
        output_artifact: Option<ReusableArtifact>,
        checkpoint_json: impl Into<String>,
    ) -> Result<(), DagError> {
        let out_id = output_artifact.as_ref().map(|a| a.artifact_id.clone());
        if let Some(art) = output_artifact {
            self.register_artifact(art);
        }
        let node = self
            .nodes
            .get_mut(node_id)
            .ok_or_else(|| DagError::NodeNotFound(node_id.to_owned()))?;
        node.state = DagNodeState::Completed;
        node.billed_cpt = billed_cpt.max(0);
        node.output_artifact_id = out_id;
        node.checkpoint_json = checkpoint_json.into();
        self.spent_cpt = self.spent_cpt.saturating_add(billed_cpt.max(0));
        self.refresh_ready_nodes();
        Ok(())
    }

    /// Cancels `node_id` (or entire pipeline if `None`) and propagates cancellation
    /// to every transitive downstream dependent.
    pub fn cancel_with_propagation(&mut self, root_node_id: Option<&str>) -> Vec<String> {
        let mut cancelled_ids = Vec::new();
        if let Some(root) = root_node_id {
            let mut to_cancel = VecDeque::new();
            to_cancel.push_back(root.to_owned());
            let mut visited = HashSet::new();
            while let Some(cur) = to_cancel.pop_front() {
                if !visited.insert(cur.clone()) {
                    continue;
                }
                if let Some(n) = self.nodes.get_mut(&cur) {
                    if n.state != DagNodeState::Completed {
                        n.state = DagNodeState::Cancelled;
                        cancelled_ids.push(cur.clone());
                    }
                }
                let children: Vec<String> = self
                    .nodes
                    .values()
                    .filter(|n| n.spec.depends_on.iter().any(|d| d == &cur))
                    .map(|n| n.spec.node_id.clone())
                    .collect();
                for ch in children {
                    to_cancel.push_back(ch);
                }
            }
        } else {
            self.cancelled = true;
            for (id, n) in &mut self.nodes {
                if n.state != DagNodeState::Completed {
                    n.state = DagNodeState::Cancelled;
                    cancelled_ids.push(id.clone());
                }
            }
        }
        cancelled_ids.sort();
        cancelled_ids
    }

    /// Resumes from a serialized checkpoint: completed nodes retain their reusable
    /// output artifacts and are skipped; non-completed nodes are reset to re-run!
    pub fn resume_from_checkpoint(&mut self) {
        self.cancelled = false;
        for n in self.nodes.values_mut() {
            if n.state != DagNodeState::Completed {
                n.state = DagNodeState::Pending;
            }
        }
        self.refresh_ready_nodes();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dag_runs_map_reduce_enforces_budget_propagates_cancel_and_resumes_checkpoint() {
        let specs = vec![
            DagNodeSpec {
                node_id: "map-1".into(),
                kind: "map".into(),
                depends_on: vec![],
                max_cpt: 40,
                input_artifact_id: None,
            },
            DagNodeSpec {
                node_id: "map-2".into(),
                kind: "map".into(),
                depends_on: vec![],
                max_cpt: 40,
                input_artifact_id: None,
            },
            DagNodeSpec {
                node_id: "reduce".into(),
                kind: "reduce".into(),
                depends_on: vec!["map-1".into(), "map-2".into()],
                max_cpt: 30,
                input_artifact_id: None,
            },
        ];

        let mut dag = DagPipeline::new("p1", "alice", "batch-rag", 100, specs).unwrap();
        // Both map nodes fit in budget 100 (40 + 40 = 80)
        assert_eq!(dag.next_dispatchable_nodes(), vec!["map-1", "map-2"]);

        dag.start_node("map-1").unwrap();
        dag.complete_node(
            "map-1",
            25,
            Some(ReusableArtifact {
                artifact_id: "art-map-1".into(),
                owner: "alice".into(),
                sha256: "a".repeat(64),
                size_bytes: 4096,
                media_type: "application/json".into(),
            }),
            r#"{"rows":100}"#,
        )
        .unwrap();

        // Cancel map-2 -> propagates automatically to downstream `reduce` while keeping `map-1` completed!
        let cancelled = dag.cancel_with_propagation(Some("map-2"));
        assert_eq!(cancelled, vec!["map-2", "reduce"]);
        assert_eq!(dag.nodes["map-1"].state, DagNodeState::Completed);

        // Checkpoint resume keeps map-1's artifact and makes only map-2 ready!
        let snapshot = serde_json::to_string(&dag).unwrap();
        let mut resumed: DagPipeline = serde_json::from_str(&snapshot).unwrap();
        resumed.resume_from_checkpoint();
        assert_eq!(resumed.next_dispatchable_nodes(), vec!["map-2"]);
        assert!(resumed.artifacts.contains_key("art-map-1"));

        // Finish map-2 -> reduce becomes ready!
        resumed.start_node("map-2").unwrap();
        resumed.complete_node("map-2", 25, None, "{}").unwrap();
        assert_eq!(resumed.next_dispatchable_nodes(), vec!["reduce"]);
    }

    #[test]
    fn dag_rejects_cycles() {
        let specs = vec![
            DagNodeSpec {
                node_id: "a".into(),
                kind: "map".into(),
                depends_on: vec!["b".into()],
                max_cpt: 10,
                input_artifact_id: None,
            },
            DagNodeSpec {
                node_id: "b".into(),
                kind: "map".into(),
                depends_on: vec!["a".into()],
                max_cpt: 10,
                input_artifact_id: None,
            },
        ];
        assert_eq!(
            DagPipeline::new("p-cycle", "alice", "bad", 100, specs).unwrap_err(),
            DagError::CycleDetected
        );
    }
}
