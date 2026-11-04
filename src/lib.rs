pub mod agent;
pub mod evaluate;
pub mod iteration;
pub mod memory;
pub mod orchestrator;
pub mod sandbox;
pub mod tools;

#[cfg(test)]
mod tests {
    use super::memory::MemoryStore;
    use super::orchestrator::Dag;
    use super::orchestrator::SubTask;
    use std::collections::HashSet;

    #[test]
    fn dag_rejects_cycles() {
        let result = Dag::new(vec![
            SubTask {
                id: "a".into(),
                description: "a".into(),
                depends_on: vec!["b".into()],
            },
            SubTask {
                id: "b".into(),
                description: "b".into(),
                depends_on: vec!["a".into()],
            },
        ]);
        assert!(result.is_err());
    }

    #[test]
    fn memory_is_bounded_and_searchable() {
        let mut m = MemoryStore::new(1);
        m.remember("a", "alpha");
        m.remember("b", "beta");
        assert_eq!(m.search("beta", 5).len(), 1);
        assert!(m.prune_context("abcdef", 3).len() <= 3);
    }

    #[tokio::test]
    async fn shell_policy_blocks_destructive_commands() {
        let sandbox = super::sandbox::Sandbox::detect().await;
        let tools = super::tools::ToolRegistry::new();
        assert!(tools.shell(&sandbox, "echo safe").await.is_ok());
        assert!(tools
            .shell(&sandbox, "echo safe; rm -rf /tmp/nope")
            .await
            .is_err());
    }

    #[test]
    fn ready_nodes_respect_dependencies() {
        let d = Dag::new(vec![
            SubTask {
                id: "a".into(),
                description: "a".into(),
                depends_on: vec![],
            },
            SubTask {
                id: "b".into(),
                description: "b".into(),
                depends_on: vec!["a".into()],
            },
        ])
        .unwrap();
        assert_eq!(d.ready(&HashSet::new()).len(), 1);
    }
}
