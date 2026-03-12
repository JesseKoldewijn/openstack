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
    /// Creates a new InteractionHistory configured to hold up to `max_entries`.
    ///
    /// The history will keep the most recent entries and drop older ones when the limit is exceeded.
    ///
    /// # Examples
    ///
    /// ```
    /// let mut history = InteractionHistory::new(10);
    /// assert_eq!(history.list().count(), 0);
    /// ```
    pub fn new(max_entries: usize) -> Self {
        Self {
            max_entries,
            entries: VecDeque::new(),
        }
    }

    /// Inserts an interaction entry into the history and enforces the configured maximum size.
    ///
    /// The provided entry becomes the newest (front) entry; if adding it causes the history to
    /// exceed `max_entries`, the oldest entries are removed until the size limit is satisfied.
    ///
    /// # Examples
    ///
    /// ```
    /// let mut hist = InteractionHistory::new(2);
    /// hist.push(InteractionEntry { id: 1, timestamp_unix_ms: 0, service: "svc".into(), status: 200, request: Default::default() });
    /// hist.push(InteractionEntry { id: 2, timestamp_unix_ms: 1, service: "svc".into(), status: 200, request: Default::default() });
    /// hist.push(InteractionEntry { id: 3, timestamp_unix_ms: 2, service: "svc".into(), status: 200, request: Default::default() });
    /// assert_eq!(hist.list().count(), 2);
    /// assert_eq!(hist.list().next().unwrap().id, 3);
    /// ```
    pub fn push(&mut self, entry: InteractionEntry) {
        self.entries.push_front(entry);
        while self.entries.len() > self.max_entries {
            self.entries.pop_back();
        }
    }

    /// Iterates over stored interaction entries in their current order (newest first).
    ///
    /// The iterator yields references to `InteractionEntry` values from newest to oldest.
    ///
    /// # Examples
    ///
    /// ```
    /// let history = InteractionHistory::new(10);
    /// for entry in history.list() {
    ///     println!("{}", entry.id);
    /// }
    /// ```
    pub fn list(&self) -> impl Iterator<Item = &InteractionEntry> {
        self.entries.iter()
    }

    /// Get a cloned `RawRequest` for the interaction with the given id, if present.
    ///
    /// # Returns
    ///
    /// `Some(RawRequest)` containing a clone of the stored request when an entry with the
    /// specified `id` exists, `None` otherwise.
    ///
    /// # Examples
    ///
    /// ```
    /// let hist = InteractionHistory::new(10);
    /// assert!(hist.replay_request(1).is_none());
    /// ```
    pub fn replay_request(&self, id: u64) -> Option<RawRequest> {
        self.entries
            .iter()
            .find(|entry| entry.id == id)
            .map(|entry| entry.request.clone())
    }

    /// Iterates over stored interactions that belong to the specified service.
    ///
    /// # Examples
    ///
    /// ```
    /// let history = InteractionHistory::new(10);
    /// let entries: Vec<_> = history.filter_by_service("auth").collect();
    /// assert!(entries.is_empty());
    /// ```
    pub fn filter_by_service<'a>(
        &'a self,
        service: &'a str,
    ) -> impl Iterator<Item = &'a InteractionEntry> {
        self.entries
            .iter()
            .filter(move |entry| entry.service == service)
    }
}
