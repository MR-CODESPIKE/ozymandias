use crate::{sandbox::Sandbox, tools::ToolRegistry};
use anyhow::Result;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IterationRecord {
    pub step: usize,
    pub phase: String,
    pub detail: String,
}
#[derive(Debug, Clone)]
pub struct IterationEngine {
    max_iterations: usize,
}
impl IterationEngine {
    pub fn bounded(max_iterations: usize) -> Self {
        Self {
            max_iterations: max_iterations.max(1),
        }
    }
    pub async fn execute(
        &self,
        objective: &str,
        tools: &ToolRegistry,
        sandbox: &Sandbox,
    ) -> Result<String> {
        let mut log = Vec::new();
        for step in 0..self.max_iterations {
            log.push(IterationRecord {
                step,
                phase: "Analyze".into(),
                detail: objective.into(),
            });
            log.push(IterationRecord {
                step,
                phase: "Plan".into(),
                detail: "Use bounded local inspection".into(),
            });
            match tools.shell(sandbox, "uname -a").await {
                Ok(out) => {
                    log.push(IterationRecord {
                        step,
                        phase: "Observe".into(),
                        detail: out.trim().into(),
                    });
                    log.push(IterationRecord {
                        step,
                        phase: "Iterate".into(),
                        detail: "completed".into(),
                    });
                    return Ok(serde_json::to_string(&log)?);
                }
                Err(e) => {
                    log.push(IterationRecord {
                        step,
                        phase: "Observe".into(),
                        detail: e.to_string(),
                    });
                    log.push(IterationRecord {
                        step,
                        phase: "Iterate".into(),
                        detail: "retry with bounded budget".into(),
                    });
                }
            }
        }
        Ok(serde_json::to_string(&log)?)
    }
}
