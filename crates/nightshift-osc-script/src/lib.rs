use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use tokio::task::JoinHandle;

pub const CURRENT_SCRIPT_VERSION: u32 = 1;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct OscScript {
    #[serde(default = "default_script_version")]
    pub version: u32,
    #[serde(default)]
    pub actions: Vec<OscAction>,
}

impl Default for OscScript {
    fn default() -> Self {
        Self {
            version: CURRENT_SCRIPT_VERSION,
            actions: Vec::new(),
        }
    }
}

fn default_script_version() -> u32 {
    CURRENT_SCRIPT_VERSION
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum OscAction {
    Send {
        address: String,
        value: OscValue,
    },
    Delay {
        milliseconds: u64,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
pub enum OscValue {
    Bool(bool),
    Int(i32),
    Float(f32),
    String(String),
}

#[derive(Default)]
pub struct ScriptTaskRegistry {
    tasks: HashMap<String, JoinHandle<()>>,
}

impl ScriptTaskRegistry {
    pub fn replace(&mut self, group: impl Into<String>, task: JoinHandle<()>) {
        let group = group.into();
        self.abort(&group);
        self.tasks.insert(group, task);
    }

    pub fn abort(&mut self, group: &str) {
        if let Some(task) = self.tasks.remove(group) {
            task.abort();
        }
    }

    pub fn abort_all(&mut self) {
        for (_, task) in self.tasks.drain() {
            task.abort();
        }
    }

    pub fn contains(&self, group: &str) -> bool {
        self.tasks.contains_key(group)
    }
}

impl Drop for ScriptTaskRegistry {
    fn drop(&mut self) {
        self.abort_all();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn old_script_without_version_defaults_to_current_version() {
        let script: OscScript = serde_json::from_str(
            r#"{
                "actions": [
                    {
                        "type": "send",
                        "address": "/avatar/parameters/Test",
                        "value": { "type": "bool", "value": true }
                    }
                ]
            }"#,
        )
        .unwrap();

        assert_eq!(script.version, CURRENT_SCRIPT_VERSION);
        assert_eq!(script.actions.len(), 1);
    }

    #[tokio::test]
    async fn replacing_group_aborts_previous_task_only_in_same_group() {
        use std::sync::{Arc, atomic::{AtomicBool, Ordering}};
        use std::time::Duration;

        let first_completed = Arc::new(AtomicBool::new(false));
        let other_completed = Arc::new(AtomicBool::new(false));

        let mut registry = ScriptTaskRegistry::default();

        let first_flag = first_completed.clone();
        registry.replace(
            "sleep_pose",
            tokio::spawn(async move {
                tokio::time::sleep(Duration::from_millis(50)).await;
                first_flag.store(true, Ordering::Release);
            }),
        );

        let other_flag = other_completed.clone();
        registry.replace(
            "custom",
            tokio::spawn(async move {
                tokio::time::sleep(Duration::from_millis(20)).await;
                other_flag.store(true, Ordering::Release);
            }),
        );

        registry.replace("sleep_pose", tokio::spawn(async {}));

        tokio::time::sleep(Duration::from_millis(80)).await;
        assert!(!first_completed.load(Ordering::Acquire));
        assert!(other_completed.load(Ordering::Acquire));
    }

    #[tokio::test]
    async fn dropping_registry_aborts_remaining_tasks() {
        use std::sync::{Arc, atomic::{AtomicBool, Ordering}};
        use std::time::Duration;

        let completed = Arc::new(AtomicBool::new(false));
        {
            let mut registry = ScriptTaskRegistry::default();
            let flag = completed.clone();
            registry.replace(
                "sleep_pose",
                tokio::spawn(async move {
                    tokio::time::sleep(Duration::from_millis(50)).await;
                    flag.store(true, Ordering::Release);
                }),
            );
        }

        tokio::time::sleep(Duration::from_millis(80)).await;
        assert!(!completed.load(Ordering::Acquire));
    }
}
