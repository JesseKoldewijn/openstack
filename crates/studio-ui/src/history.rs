use std::collections::VecDeque;

use crate::api::RawRequest;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InteractionEntry {
    pub id: u64,
    pub timestamp_unix_ms: u64,
    pub service: String,
    pub status: u16,
    pub request: RawRequest,
}

#[derive(Debug, Clone)]
pub struct InteractionHistory {
    max_entries: usize,
    entries: VecDeque<InteractionEntry>,
}

impl InteractionHistory {
    pub fn new(max_entries: usize) -> Self {
        Self {
            max_entries,
            entries: VecDeque::new(),
        }
    }

    pub fn push(&mut self, entry: InteractionEntry) {
        self.entries.push_front(entry);
        while self.entries.len() > self.max_entries {
            self.entries.pop_back();
        }
    }

    pub fn list(&self) -> impl Iterator<Item = &InteractionEntry> {
        self.entries.iter()
    }

    pub fn replay_request(&self, id: u64) -> Option<RawRequest> {
        self.entries
            .iter()
            .find(|entry| entry.id == id)
            .map(|entry| entry.request.clone())
    }

    pub fn filter_by_service<'a>(
        &'a self,
        service: &'a str,
    ) -> impl Iterator<Item = &'a InteractionEntry> {
        self.entries
            .iter()
            .filter(move |entry| entry.service == service)
    }
}
