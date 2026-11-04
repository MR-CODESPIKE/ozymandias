use serde::{Deserialize, Serialize};
use std::collections::VecDeque;

#[derive(Debug, Clone)]
pub struct MemoryStore {
    capacity: usize,
    entries: VecDeque<(String, String)>,
}

impl MemoryStore {
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity: capacity.max(1),
            entries: VecDeque::new(),
        }
    }

    pub fn remember(&mut self, key: &str, value: &str) {
        self.entries.retain(|(k, _)| k != key);
        self.entries.push_back((key.to_string(), value.to_string()));
        while self.entries.len() > self.capacity {
            self.entries.pop_front();
        }
    }

    pub fn search(&self, query: &str, limit: usize) -> Vec<(String, String)> {
        let q = query.to_ascii_lowercase();
        self.entries
            .iter()
            .filter(|(k, v)| {
                k.to_ascii_lowercase().contains(&q) || v.to_ascii_lowercase().contains(&q)
            })
            .take(limit)
            .cloned()
            .collect()
    }

    pub fn prune_context(&self, text: &str, max_chars: usize) -> String {
        text.chars().take(max_chars).collect()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum MemoryKind {
    Fact,
    Decision,
    Failure,
    Evidence,
    Progress,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LongMemoryEntry {
    pub key: String,
    pub value: String,
    pub kind: MemoryKind,
    pub importance: u8,
}

#[derive(Debug, Clone)]
pub struct LongContextMemory {
    capacity: usize,
    entries: VecDeque<LongMemoryEntry>,
}

impl LongContextMemory {
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity: capacity.max(1),
            entries: VecDeque::new(),
        }
    }

    pub fn remember(&mut self, key: &str, value: &str, kind: MemoryKind, importance: u8) {
        self.entries.retain(|entry| entry.key != key);
        self.entries.push_back(LongMemoryEntry {
            key: key.to_string(),
            value: value.to_string(),
            kind,
            importance,
        });
        self.prune_entries();
    }

    pub fn summarize(&self, max_chars: usize) -> String {
        let mut ranked: Vec<&LongMemoryEntry> = self.entries.iter().collect();
        ranked.sort_by_key(|entry| std::cmp::Reverse(entry.importance));
        let mut output = String::new();
        for entry in ranked {
            let line = format!(
                "[{:?}/{}] {}: {}\n",
                entry.kind, entry.importance, entry.key, entry.value
            );
            if output.chars().count() + line.chars().count() > max_chars {
                break;
            }
            output.push_str(&line);
        }
        output
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    fn prune_entries(&mut self) {
        while self.entries.len() > self.capacity {
            let least_important = self
                .entries
                .iter()
                .enumerate()
                .min_by_key(|(_, entry)| entry.importance)
                .map(|(index, _)| index)
                .unwrap_or(0);
            self.entries.remove(least_important);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{LongContextMemory, MemoryKind};

    #[test]
    fn summary_prioritizes_important_entries_and_stays_bounded() {
        let mut memory = LongContextMemory::new(3);
        memory.remember("low", "discardable", MemoryKind::Progress, 1);
        memory.remember("failure", "syntax error", MemoryKind::Failure, 10);
        memory.remember("evidence", "tests passed", MemoryKind::Evidence, 9);
        memory.remember("fact", "api requires token", MemoryKind::Fact, 8);
        assert_eq!(memory.len(), 3);
        let summary = memory.summarize(80);
        assert!(summary.contains("failure"));
        assert!(summary.chars().count() <= 80);
    }
}
