use rustc_hash::FxHashMap;

use super::types::SafetyEvent;

pub trait AlertingHook: Send + Sync {
    fn on_safety_event(&self, event: &SafetyEvent) -> anyhow::Result<()>;
    fn hook_id(&self) -> &str;
}

#[derive(Debug, Clone, Default)]
pub struct HookDeliveryStats {
    pub total_events: u64,
    pub successful: u64,
    pub failed: u64,
    pub last_event_timestamp: Option<i64>,
}

pub struct AlertingHookManager {
    hooks: Vec<Box<dyn AlertingHook>>,
    delivery_stats: FxHashMap<String, HookDeliveryStats>,
    events_emitted: Vec<SafetyEvent>,
}

impl Default for AlertingHookManager {
    fn default() -> Self {
        Self::new()
    }
}

impl AlertingHookManager {
    pub fn new() -> Self {
        Self {
            hooks: Vec::new(),
            delivery_stats: FxHashMap::default(),
            events_emitted: Vec::new(),
        }
    }

    pub fn register_hook(&mut self, hook: Box<dyn AlertingHook>) {
        let id = hook.hook_id().to_string();
        self.delivery_stats.entry(id).or_default();
        self.hooks.push(hook);
    }

    pub fn unregister_hook(&mut self, hook_id: &str) -> bool {
        let before = self.hooks.len();
        self.hooks.retain(|h| h.hook_id() != hook_id);
        self.hooks.len() < before
    }

    pub fn emit_event(&mut self, event: SafetyEvent) {
        for hook in &self.hooks {
            let id = hook.hook_id().to_string();
            let stats = self.delivery_stats.entry(id).or_default();
            stats.total_events += 1;
            match hook.on_safety_event(&event) {
                Ok(()) => {
                    stats.successful += 1;
                },
                Err(_) => {
                    stats.failed += 1;
                },
            }
        }
        self.events_emitted.push(event);
    }

    pub fn get_stats(&self) -> &FxHashMap<String, HookDeliveryStats> {
        &self.delivery_stats
    }

    pub fn recent_events(&self, limit: usize) -> &[SafetyEvent] {
        let start = self.events_emitted.len().saturating_sub(limit);
        &self.events_emitted[start..]
    }
}

#[cfg(test)]
mod tests {
    use super::super::types::HaltTrigger;
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU64, Ordering};

    struct TestHook {
        id: String,
        call_count: Arc<AtomicU64>,
    }

    impl AlertingHook for TestHook {
        fn on_safety_event(&self, _event: &SafetyEvent) -> anyhow::Result<()> {
            self.call_count.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }

        fn hook_id(&self) -> &str {
            &self.id
        }
    }

    struct FailingHook {
        id: String,
    }

    impl AlertingHook for FailingHook {
        fn on_safety_event(&self, _event: &SafetyEvent) -> anyhow::Result<()> {
            Err(anyhow::anyhow!("hook delivery failed"))
        }

        fn hook_id(&self) -> &str {
            &self.id
        }
    }

    #[test]
    fn test_registered_hook_receives_events() {
        let mut manager = AlertingHookManager::new();
        let count = Arc::new(AtomicU64::new(0));
        manager.register_hook(Box::new(TestHook {
            id: "test-hook".to_string(),
            call_count: count.clone(),
        }));

        manager.emit_event(SafetyEvent::BridgeHalted {
            reason: "test".to_string(),
            trigger: HaltTrigger::ManualHalt,
        });

        assert_eq!(count.load(Ordering::SeqCst), 1);
        let stats = manager.get_stats();
        let hook_stats = stats.get("test-hook").unwrap();
        assert_eq!(hook_stats.successful, 1);
        assert_eq!(hook_stats.failed, 0);
    }

    #[test]
    fn test_unregister_stops_delivery() {
        let mut manager = AlertingHookManager::new();
        let count = Arc::new(AtomicU64::new(0));
        manager.register_hook(Box::new(TestHook {
            id: "removable".to_string(),
            call_count: count.clone(),
        }));

        assert!(manager.unregister_hook("removable"));

        manager.emit_event(SafetyEvent::BridgeResumed {
            reason: "test".to_string(),
            operator: "op".to_string(),
        });

        assert_eq!(count.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn test_hook_failure_doesnt_block_others() {
        let mut manager = AlertingHookManager::new();
        let count = Arc::new(AtomicU64::new(0));

        manager.register_hook(Box::new(FailingHook {
            id: "failing".to_string(),
        }));
        manager.register_hook(Box::new(TestHook {
            id: "working".to_string(),
            call_count: count.clone(),
        }));

        manager.emit_event(SafetyEvent::BridgeHalted {
            reason: "test".to_string(),
            trigger: HaltTrigger::ManualHalt,
        });

        // The working hook should still receive the event despite the failing one
        assert_eq!(count.load(Ordering::SeqCst), 1);
        let stats = manager.get_stats();
        assert_eq!(stats.get("failing").unwrap().failed, 1);
        assert_eq!(stats.get("working").unwrap().successful, 1);
    }
}
