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
