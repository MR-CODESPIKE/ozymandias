use anyhow::{anyhow, Result};
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone)]
pub struct SubTask {
    pub id: String,
    pub description: String,
    pub depends_on: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct Dag {
    tasks: HashMap<String, SubTask>,
}

impl Dag {
    pub fn new(tasks: Vec<SubTask>) -> Result<Self> {
        let mut map = HashMap::new();
        for task in tasks {
            if map.insert(task.id.clone(), task).is_some() {
                return Err(anyhow!("duplicate task id"));
            }
        }
        for task in map.values() {
            for dep in &task.depends_on {
                if !map.contains_key(dep) {
                    return Err(anyhow!("unknown dependency: {dep}"));
                }
            }
        }
        let dag = Self { tasks: map };
        let mut visiting = HashSet::new();
        let mut done = HashSet::new();
        for id in dag.tasks.keys() {
            dag.visit(id, &mut visiting, &mut done)?;
        }
        Ok(dag)
    }
    fn visit(
        &self,
        id: &str,
        visiting: &mut HashSet<String>,
        done: &mut HashSet<String>,
    ) -> Result<()> {
        if done.contains(id) {
            return Ok(());
        }
        if !visiting.insert(id.to_string()) {
            return Err(anyhow!("cycle detected"));
        }
        for dep in &self.tasks[id].depends_on {
            self.visit(dep, visiting, done)?;
        }
        visiting.remove(id);
        done.insert(id.to_string());
        Ok(())
    }
    pub fn ready(&self, completed: &HashSet<String>) -> Vec<SubTask> {
        self.tasks
            .values()
            .filter(|t| {
                !completed.contains(&t.id) && t.depends_on.iter().all(|d| completed.contains(d))
            })
            .cloned()
            .collect()
    }
}
