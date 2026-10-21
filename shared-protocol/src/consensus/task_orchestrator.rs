use crate::blockchain::state::TaskAssignerPolicy;
use crate::blockchain::transaction::ForecastArtifactManifest;
use crate::consensus::data_desert::DataDesertPolicy;
use rustc_hash::FxHashMap;
use serde::{Deserialize, Serialize};

/// Policy controlling geographic weighting in task assignment.
///
/// When enabled, nodes in the same region as a task receive a scoring bonus,
/// and per-region capacity can be tracked for load-aware routing.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeoRoutingPolicy {
    /// Whether geographic routing is enabled.
    pub enabled: bool,
    /// Multiplier applied when task region matches node region.
    pub same_region_bonus: f64,
    /// Weight factor for distance-based scoring (0.0 = disabled).
    pub distance_weight: f64,
    /// Per-region capacity tracking: (current_load, max_capacity)
    pub region_capacity: FxHashMap<String, (u32, u32)>,
}

impl Default for GeoRoutingPolicy {
    fn default() -> Self {
        Self {
            enabled: true,
            same_region_bonus: 2.0,
            distance_weight: 0.5,
            region_capacity: FxHashMap::default(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TaskStatus {
    Pending,
    Assigned,
    Running,
    Completed,
    Failed,
    TimedOut,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ForecastTask {
    pub task_id: String,
    pub manifest: ForecastArtifactManifest,
    pub status: TaskStatus,
    pub assigned_node: Option<String>,
    pub created_at: i64,
    pub assigned_at: Option<i64>,
    pub completed_at: Option<i64>,
    pub retry_count: u32,
    pub max_retries: u32,
    pub region: String,
    pub failure_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComputeNode {
    pub node_id: String,
    pub peer_id: String,
    pub capabilities: Vec<String>,
    pub region: String,
    pub last_heartbeat: i64,
    pub active_tasks: u32,
    pub max_concurrent_tasks: u32,
    /// Observation density (observations per km² per day) reported by the node.
    /// Used by `DataDesertPolicy` to boost assignment weight for sparse regions.
    pub observation_density: Option<f64>,
    /// Discrete weather-station count for this node's grid cell.
    /// Compared against `DataDesertPolicy::min_station_density`.
    pub station_count: Option<u32>,
}

impl ComputeNode {
    fn has_capacity(&self) -> bool {
        self.active_tasks < self.max_concurrent_tasks
    }
}

#[derive(Debug, thiserror::Error)]
pub enum TaskOrchestratorError {
    #[error("duplicate node_id: {0}")]
    DuplicateNode(String),
    #[error("unknown node_id: {0}")]
    UnknownNode(String),
    #[error("unknown task_id: {0}")]
    UnknownTask(String),
    #[error("invalid manifest: {0}")]
    InvalidManifest(String),
    #[error("invalid policy: {0}")]
    InvalidPolicy(String),
}

pub struct TaskOrchestrator {
    tasks: FxHashMap<String, ForecastTask>,
    nodes: FxHashMap<String, ComputeNode>,
    policy: TaskAssignerPolicy,
    geo_routing_policy: GeoRoutingPolicy,
    data_desert_policy: DataDesertPolicy,
    task_timeout_secs: i64,
    max_retries: u32,
    next_task_id: u64,
}

impl TaskOrchestrator {
    pub fn new(policy: TaskAssignerPolicy) -> Self {
        Self {
            tasks: FxHashMap::default(),
            nodes: FxHashMap::default(),
            policy,
            geo_routing_policy: GeoRoutingPolicy::default(),
            data_desert_policy: DataDesertPolicy::default(),
            task_timeout_secs: 600,
            max_retries: 3,
            next_task_id: 1,
        }
    }

    pub fn register_node(&mut self, node: ComputeNode) -> Result<(), TaskOrchestratorError> {
        if self.nodes.contains_key(&node.node_id) {
            return Err(TaskOrchestratorError::DuplicateNode(node.node_id));
        }
        self.nodes.insert(node.node_id.clone(), node);
        Ok(())
    }

    pub fn deregister_node(&mut self, node_id: &str) -> Result<(), TaskOrchestratorError> {
        if self.nodes.remove(node_id).is_none() {
            return Err(TaskOrchestratorError::UnknownNode(node_id.to_string()));
        }
        // Reassign tasks from deregistered node back to Pending
        for task in self.tasks.values_mut() {
            if task.assigned_node.as_deref() == Some(node_id)
                && matches!(task.status, TaskStatus::Assigned | TaskStatus::Running)
            {
                task.status = TaskStatus::Pending;
                task.assigned_node = None;
                task.assigned_at = None;
            }
        }
        Ok(())
    }

    pub fn submit_task(
        &mut self,
        manifest: ForecastArtifactManifest,
        region: &str,
        current_time: i64,
    ) -> Result<String, TaskOrchestratorError> {
        manifest
            .validate()
            .map_err(|e| TaskOrchestratorError::InvalidManifest(e.to_string()))?;
        let task_id = format!("task-{}", self.next_task_id);
        self.next_task_id += 1;
        let task = ForecastTask {
            task_id: task_id.clone(),
            manifest,
            status: TaskStatus::Pending,
            assigned_node: None,
            created_at: current_time,
            assigned_at: None,
            completed_at: None,
            retry_count: 0,
            max_retries: self.max_retries,
            region: region.to_string(),
            failure_reason: None,
        };
        self.tasks.insert(task_id.clone(), task);
        Ok(task_id)
    }

    /// Assign the highest-priority pending task to the best available node.
    /// Returns (task_id, node_id) if an assignment was made.
    pub fn assign_next(&mut self, current_time: i64) -> Option<(String, String)> {
        if !self.policy.enabled {
            return None;
        }

        let pending_tasks: Vec<String> = self
            .tasks
            .iter()
            .filter(|(_, t)| t.status == TaskStatus::Pending)
            .map(|(id, _)| id.clone())
            .collect();

        if pending_tasks.is_empty() {
            return None;
        }

        let eligible_nodes: Vec<String> = self
            .nodes
            .iter()
            .filter(|(_, n)| n.has_capacity() && self.is_heartbeat_fresh(n, current_time))
            .map(|(id, _)| id.clone())
            .collect();

        if eligible_nodes.is_empty() {
            return None;
        }

        let mut best: Option<(String, String, f64)> = None;

        for task_id in &pending_tasks {
            let task = &self.tasks[task_id];
            for node_id in &eligible_nodes {
                let node = &self.nodes[node_id];
                let weight = self.compute_assignment_weight(task, node);
                if best.as_ref().map_or(true, |(_, _, w)| weight > *w) {
                    best = Some((task_id.clone(), node_id.clone(), weight));
                }
            }
        }

        if let Some((task_id, node_id, _)) = best {
            let task = self.tasks.get_mut(&task_id)?;
            task.status = TaskStatus::Assigned;
            task.assigned_node = Some(node_id.clone());
            task.assigned_at = Some(current_time);

            let node = self.nodes.get_mut(&node_id)?;
            node.active_tasks += 1;

            Some((task_id, node_id))
        } else {
            None
        }
    }

    pub fn mark_running(&mut self, task_id: &str) -> Result<(), TaskOrchestratorError> {
        let task = self
            .tasks
            .get_mut(task_id)
            .ok_or_else(|| TaskOrchestratorError::UnknownTask(task_id.to_string()))?;
        task.status = TaskStatus::Running;
        Ok(())
    }

    pub fn mark_completed(
        &mut self,
        task_id: &str,
        current_time: i64,
    ) -> Result<(), TaskOrchestratorError> {
        let task = self
            .tasks
            .get_mut(task_id)
            .ok_or_else(|| TaskOrchestratorError::UnknownTask(task_id.to_string()))?;
        task.status = TaskStatus::Completed;
        task.completed_at = Some(current_time);
        if let Some(node_id) = task.assigned_node.as_ref() {
            if let Some(node) = self.nodes.get_mut(node_id) {
                node.active_tasks = node.active_tasks.saturating_sub(1);
            }
        }
        Ok(())
    }

    pub fn mark_failed(
        &mut self,
        task_id: &str,
        reason: &str,
    ) -> Result<(), TaskOrchestratorError> {
        let task = self
            .tasks
            .get_mut(task_id)
            .ok_or_else(|| TaskOrchestratorError::UnknownTask(task_id.to_string()))?;
        task.failure_reason = Some(reason.to_string());
        if let Some(node_id) = task.assigned_node.as_ref() {
            if let Some(node) = self.nodes.get_mut(node_id) {
                node.active_tasks = node.active_tasks.saturating_sub(1);
            }
        }
        task.retry_count += 1;
        if task.retry_count >= task.max_retries {
            task.status = TaskStatus::Failed;
        } else {
            task.status = TaskStatus::Pending;
            task.assigned_node = None;
            task.assigned_at = None;
        }
        Ok(())
    }

    /// Check for timed-out tasks and requeue them. Returns the IDs of requeued tasks.
    pub fn check_timeouts(&mut self, current_time: i64) -> Vec<String> {
        let mut timed_out = Vec::with_capacity(self.tasks.len());
        let timeout = self.task_timeout_secs;
        for (task_id, task) in &self.tasks {
            if matches!(task.status, TaskStatus::Assigned | TaskStatus::Running) {
                if let Some(assigned_at) = task.assigned_at {
                    if current_time - assigned_at > timeout {
                        timed_out.push(task_id.clone());
                    }
                }
            }
        }
        for task_id in &timed_out {
            if let Some(task) = self.tasks.get_mut(task_id) {
                if let Some(node_id) = task.assigned_node.as_ref() {
                    if let Some(node) = self.nodes.get_mut(node_id) {
                        node.active_tasks = node.active_tasks.saturating_sub(1);
                    }
                }
                task.retry_count += 1;
                if task.retry_count >= task.max_retries {
                    task.status = TaskStatus::TimedOut;
                } else {
                    task.status = TaskStatus::Pending;
                    task.assigned_node = None;
                    task.assigned_at = None;
                }
            }
        }
        timed_out
    }

    pub fn get_task(&self, task_id: &str) -> Option<&ForecastTask> {
        self.tasks.get(task_id)
    }

    fn is_heartbeat_fresh(&self, node: &ComputeNode, current_time: i64) -> bool {
        current_time - node.last_heartbeat < self.task_timeout_secs
    }

    /// Set the geographic routing policy.
    pub fn set_geo_routing_policy(
        &mut self,
        policy: GeoRoutingPolicy,
    ) -> Result<(), TaskOrchestratorError> {
        if policy.same_region_bonus < 0.0 {
            return Err(TaskOrchestratorError::InvalidPolicy(
                "same_region_bonus must be non-negative".to_string(),
            ));
        }
        if policy.distance_weight < 0.0 {
            return Err(TaskOrchestratorError::InvalidPolicy(
                "distance_weight must be non-negative".to_string(),
            ));
        }
        self.geo_routing_policy = policy;
        Ok(())
    }

    /// Set the data desert policy.
    pub fn set_data_desert_policy(
        &mut self,
        policy: DataDesertPolicy,
    ) -> Result<(), TaskOrchestratorError> {
        if policy.desert_bonus_bps < 100 {
            return Err(TaskOrchestratorError::InvalidPolicy(
                "desert_bonus_bps must be >= 100".to_string(),
            ));
        }
        if policy.min_station_density == 0 {
            return Err(TaskOrchestratorError::InvalidPolicy(
                "min_station_density must be >= 1".to_string(),
            ));
        }
        self.data_desert_policy = policy;
        Ok(())
    }

    /// Return a count of pending tasks grouped by region.
    pub fn pending_by_region(&self) -> FxHashMap<String, usize> {
        let mut counts = FxHashMap::default();
        for task in self.tasks.values() {
            if task.status == TaskStatus::Pending {
                *counts.entry(task.region.clone()).or_insert(0) += 1;
            }
        }
        counts
    }

    /// Update the per-region capacity tracking in the geo routing policy.
    pub fn update_region_capacity(&mut self, region: &str, current_load: u32, max_capacity: u32) {
        self.geo_routing_policy
            .region_capacity
            .insert(region.to_string(), (current_load, max_capacity));
    }

    fn compute_assignment_weight(&self, task: &ForecastTask, node: &ComputeNode) -> f64 {
        let mut weight = 1.0;

        // Region match bonus — use geo routing policy when enabled,
        // otherwise fall back to the legacy underserved_bonus_multiplier.
        if task.region == node.region {
            if self.geo_routing_policy.enabled {
                weight *= self.geo_routing_policy.same_region_bonus;
            } else {
                weight *= self.policy.underserved_bonus_multiplier;
            }
        }

        // Same-region overlap penalty: count how many running tasks this node has in the same region
        let same_region_running = self
            .tasks
            .values()
            .filter(|t| {
                t.assigned_node.as_deref() == Some(&node.node_id)
                    && matches!(t.status, TaskStatus::Assigned | TaskStatus::Running)
                    && t.region == task.region
            })
            .count();

        if same_region_running > 0 {
            weight *= self.policy.redundancy_penalty_multiplier;
        }

        // Data desert bonus: boost weight for nodes in sparse regions
        if let Some(count) = node.station_count {
            if count < self.data_desert_policy.min_station_density {
                weight *= self.data_desert_policy.desert_bonus_bps as f64 / 100.0;
            }
        }

        weight
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn default_policy() -> TaskAssignerPolicy {
        TaskAssignerPolicy::default()
    }

    fn test_manifest() -> ForecastArtifactManifest {
        ForecastArtifactManifest {
            schema_version: 1,
            model_id: "wrf-gfs-v3".to_string(),
            input_hash: "a".repeat(64),
            output_hash: "b".repeat(64),
            produced_at: 1_700_000_000,
            region: "us-east".to_string(),
            horizon_hours: 48,
            input_variables: vec![
                "humidity".to_string(),
                "pressure".to_string(),
                "temperature".to_string(),
            ],
            output_variables: vec!["precipitation".to_string(), "temperature".to_string()],
        }
    }

    fn test_node(id: &str, region: &str, max_tasks: u32) -> ComputeNode {
        ComputeNode {
            node_id: id.to_string(),
            peer_id: format!("peer-{id}"),
            capabilities: vec!["forecast".to_string()],
            region: region.to_string(),
            last_heartbeat: 1_700_000_000,
            active_tasks: 0,
            max_concurrent_tasks: max_tasks,
            observation_density: None,
            station_count: None,
        }
    }

    #[test]
    fn register_node_and_submit_task() {
        let mut orch = TaskOrchestrator::new(default_policy());
        orch.register_node(test_node("node-1", "us-east", 5))
            .unwrap();

        let task_id = orch
            .submit_task(test_manifest(), "us-east", 1_700_000_000)
            .unwrap();
        assert_eq!(task_id, "task-1");

        let task = orch.get_task(&task_id).unwrap();
        assert_eq!(task.status, TaskStatus::Pending);
    }

    #[test]
    fn assign_next_picks_matching_region() {
        let mut orch = TaskOrchestrator::new(default_policy());
        orch.register_node(test_node("node-east", "us-east", 5))
            .unwrap();
        orch.register_node(test_node("node-west", "us-west", 5))
            .unwrap();

        orch.submit_task(test_manifest(), "us-east", 1_700_000_000)
            .unwrap();

        let (task_id, node_id) = orch.assign_next(1_700_000_000).unwrap();
        assert_eq!(task_id, "task-1");
        assert_eq!(node_id, "node-east");
    }

    #[test]
    fn assign_next_applies_redundancy_penalty() {
        let mut orch = TaskOrchestrator::new(default_policy());
        orch.register_node(test_node("node-a", "us-east", 5))
            .unwrap();
        orch.register_node(test_node("node-b", "us-east", 5))
            .unwrap();

        // Submit and assign first task to node-a
        orch.submit_task(test_manifest(), "us-east", 1_700_000_000)
            .unwrap();
        let (_, first_node) = orch.assign_next(1_700_000_000).unwrap();

        // Submit second task in same region
        orch.submit_task(test_manifest(), "us-east", 1_700_000_001)
            .unwrap();
        let (_, second_node) = orch.assign_next(1_700_000_001).unwrap();

        // Second task should go to the OTHER node due to redundancy penalty
        assert_ne!(first_node, second_node);
    }

    #[test]
    fn task_lifecycle_pending_to_completed() {
        let mut orch = TaskOrchestrator::new(default_policy());
        orch.register_node(test_node("node-1", "us-east", 5))
            .unwrap();
        orch.submit_task(test_manifest(), "us-east", 1_700_000_000)
            .unwrap();

        let (task_id, _) = orch.assign_next(1_700_000_000).unwrap();
        assert_eq!(
            orch.get_task(&task_id).unwrap().status,
            TaskStatus::Assigned
        );

        orch.mark_running(&task_id).unwrap();
        assert_eq!(orch.get_task(&task_id).unwrap().status, TaskStatus::Running);

        orch.mark_completed(&task_id, 1_700_000_100).unwrap();
        assert_eq!(
            orch.get_task(&task_id).unwrap().status,
            TaskStatus::Completed
        );
    }

    #[test]
    fn task_lifecycle_failed_retries() {
        let mut orch = TaskOrchestrator::new(default_policy());
        orch.register_node(test_node("node-1", "us-east", 5))
            .unwrap();
        orch.submit_task(test_manifest(), "us-east", 1_700_000_000)
            .unwrap();

        let (task_id, _) = orch.assign_next(1_700_000_000).unwrap();
        orch.mark_running(&task_id).unwrap();
        orch.mark_failed(&task_id, "oom").unwrap();

        // Should be back to Pending after first failure (retry_count < max_retries)
        assert_eq!(orch.get_task(&task_id).unwrap().status, TaskStatus::Pending);
        assert_eq!(orch.get_task(&task_id).unwrap().retry_count, 1);
    }

    #[test]
    fn check_timeouts_requeues() {
        let mut orch = TaskOrchestrator::new(default_policy());
        orch.register_node(test_node("node-1", "us-east", 5))
            .unwrap();
        orch.submit_task(test_manifest(), "us-east", 1_700_000_000)
            .unwrap();

        let (task_id, _) = orch.assign_next(1_700_000_000).unwrap();
        orch.mark_running(&task_id).unwrap();

        // Advance time past timeout (default 600s)
        let timed_out = orch.check_timeouts(1_700_000_700);
        assert_eq!(timed_out.len(), 1);
        assert_eq!(timed_out[0], task_id);
        assert_eq!(orch.get_task(&task_id).unwrap().status, TaskStatus::Pending);
    }

    #[test]
    fn deregister_node_reassigns_tasks() {
        let mut orch = TaskOrchestrator::new(default_policy());
        orch.register_node(test_node("node-1", "us-east", 5))
            .unwrap();
        orch.submit_task(test_manifest(), "us-east", 1_700_000_000)
            .unwrap();

        let (task_id, _) = orch.assign_next(1_700_000_000).unwrap();
        assert_eq!(
            orch.get_task(&task_id).unwrap().status,
            TaskStatus::Assigned
        );

        orch.deregister_node("node-1").unwrap();
        assert_eq!(orch.get_task(&task_id).unwrap().status, TaskStatus::Pending);
        assert!(orch.get_task(&task_id).unwrap().assigned_node.is_none());
    }

    #[test]
    fn duplicate_node_rejected() {
        let mut orch = TaskOrchestrator::new(default_policy());
        orch.register_node(test_node("node-1", "us-east", 5))
            .unwrap();
        let result = orch.register_node(test_node("node-1", "us-west", 3));
        assert!(result.is_err());
    }

    #[test]
    fn geo_routing_same_region_bonus() {
        let mut orch = TaskOrchestrator::new(default_policy());
        // Set a distinctive bonus so we can verify it is used
        let mut geo = GeoRoutingPolicy::default();
        geo.same_region_bonus = 5.0;
        orch.set_geo_routing_policy(geo).unwrap();

        orch.register_node(test_node("node-east", "us-east", 5))
            .unwrap();
        orch.register_node(test_node("node-west", "us-west", 5))
            .unwrap();

        orch.submit_task(test_manifest(), "us-east", 1_700_000_000)
            .unwrap();

        // The same-region node should win because of the 5x bonus
        let (_, node_id) = orch.assign_next(1_700_000_000).unwrap();
        assert_eq!(node_id, "node-east");

        // Verify the weight difference: same-region node gets 5.0, other gets 1.0
        let task = orch.get_task("task-1").unwrap();
        let east_node = ComputeNode {
            node_id: "probe-east".into(),
            peer_id: "peer-probe-east".into(),
            capabilities: vec!["forecast".into()],
            region: "us-east".into(),
            last_heartbeat: 1_700_000_000,
            active_tasks: 0,
            max_concurrent_tasks: 5,
            observation_density: None,
            station_count: None,
        };
        let west_node = ComputeNode {
            node_id: "probe-west".into(),
            peer_id: "peer-probe-west".into(),
            capabilities: vec!["forecast".into()],
            region: "us-west".into(),
            last_heartbeat: 1_700_000_000,
            active_tasks: 0,
            max_concurrent_tasks: 5,
            observation_density: None,
            station_count: None,
        };
        let w_east = orch.compute_assignment_weight(task, &east_node);
        let w_west = orch.compute_assignment_weight(task, &west_node);
        assert!(
            w_east > w_west,
            "same-region node should have higher weight"
        );
        assert!((w_east - 5.0).abs() < f64::EPSILON);
        assert!((w_west - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn pending_by_region_counts() {
        let mut orch = TaskOrchestrator::new(default_policy());
        orch.register_node(test_node("node-1", "us-east", 5))
            .unwrap();

        orch.submit_task(test_manifest(), "us-east", 1_700_000_000)
            .unwrap();
        orch.submit_task(test_manifest(), "us-east", 1_700_000_001)
            .unwrap();
        orch.submit_task(test_manifest(), "eu-west", 1_700_000_002)
            .unwrap();

        let counts = orch.pending_by_region();
        assert_eq!(counts.get("us-east"), Some(&2));
        assert_eq!(counts.get("eu-west"), Some(&1));
        assert_eq!(counts.get("ap-south"), None);
    }

    #[test]
    fn data_desert_boosts_sparse_region() {
        let mut orch = TaskOrchestrator::new(default_policy());
        // min_station_density=3, desert_bonus_bps=150 by default

        let mut sparse_node = test_node("node-sparse", "us-east", 5);
        sparse_node.observation_density = Some(0.0); // kept for analytics; no longer affects behavior
        sparse_node.station_count = Some(0); // below threshold → desert bonus

        let mut dense_node = test_node("node-dense", "us-east", 5);
        dense_node.observation_density = Some(5.0); // kept for analytics; no longer affects behavior
        dense_node.station_count = Some(5); // above threshold → no bonus

        orch.register_node(sparse_node).unwrap();
        orch.register_node(dense_node).unwrap();

        orch.submit_task(test_manifest(), "us-east", 1_700_000_000)
            .unwrap();

        // Sparse node should win due to 3x data desert boost
        let (_, node_id) = orch.assign_next(1_700_000_000).unwrap();
        assert_eq!(node_id, "node-sparse");
    }

    #[test]
    fn geo_routing_rejects_negative_bonus() {
        let mut orch = TaskOrchestrator::new(default_policy());
        let mut geo = GeoRoutingPolicy::default();
        geo.same_region_bonus = -1.0;
        assert!(orch.set_geo_routing_policy(geo).is_err());
    }

    #[test]
    fn data_desert_rejects_low_multiplier() {
        let mut orch = TaskOrchestrator::new(default_policy());
        let policy = DataDesertPolicy {
            desert_bonus_bps: 50,
            ..DataDesertPolicy::default()
        };
        assert!(orch.set_data_desert_policy(policy).is_err());
    }

    #[test]
    fn geo_routing_disabled() {
        let mut orch = TaskOrchestrator::new(default_policy());
        let mut geo = GeoRoutingPolicy::default();
        geo.enabled = false;
        geo.same_region_bonus = 10.0; // should NOT be used
        orch.set_geo_routing_policy(geo).unwrap();

        orch.register_node(test_node("node-east", "us-east", 5))
            .unwrap();

        orch.submit_task(test_manifest(), "us-east", 1_700_000_000)
            .unwrap();

        let task = orch.get_task("task-1").unwrap();
        let node = &orch.nodes["node-east"];
        let weight = orch.compute_assignment_weight(task, node);

        // When geo routing is disabled, fall back to underserved_bonus_multiplier (default 1.5)
        let expected = orch.policy.underserved_bonus_multiplier;
        assert!(
            (weight - expected).abs() < f64::EPSILON,
            "when geo routing disabled, should use legacy multiplier, got {weight} expected {expected}"
        );
    }
}
