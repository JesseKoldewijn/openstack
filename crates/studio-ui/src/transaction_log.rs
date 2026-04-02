use std::collections::VecDeque;

/// Global transaction log for the Studio runtime.
///
/// Captures every HTTP request/response pair that flows through the gateway
/// so the Transactions tab can show a chronological, cross-service audit trail.
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Transaction record
// ---------------------------------------------------------------------------

/// Direction of a single transaction entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TransactionDirection {
    /// Outbound request sent to the gateway.
    Request,
    /// Inbound response received from the gateway.
    Response,
}

/// Outcome classification for quick filtering.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TransactionOutcome {
    Success,
    ClientError,
    ServerError,
    Pending,
}

impl TransactionOutcome {
    pub fn from_status(status: u16) -> Self {
        match status {
            0 => Self::Pending,
            200..=299 => Self::Success,
            400..=499 => Self::ClientError,
            500..=599 => Self::ServerError,
            _ => Self::ClientError,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Success => "success",
            Self::ClientError => "client_error",
            Self::ServerError => "server_error",
            Self::Pending => "pending",
        }
    }
}

/// One complete request/response cycle recorded in the transaction log.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TransactionRecord {
    /// Monotonically increasing identifier within this session.
    pub id: u64,
    /// Service slug (e.g. `"s3"`, `"sqs"`).
    pub service: String,
    /// AWS operation name if resolved (e.g. `"PutObject"`).
    pub operation: Option<String>,
    /// HTTP method.
    pub method: String,
    /// Request path.
    pub path: String,
    /// HTTP status code (0 = pending / no response yet).
    pub status: u16,
    /// Request body (truncated to `MAX_BODY_PREVIEW` bytes for display).
    pub request_body_preview: Option<String>,
    /// Response body (truncated to `MAX_BODY_PREVIEW` bytes for display).
    pub response_body_preview: Option<String>,
    /// Unix timestamp (milliseconds) when the request was initiated.
    pub started_at_ms: u64,
    /// Duration in milliseconds, if complete.
    pub duration_ms: Option<u64>,
    /// Outcome classification derived from `status`.
    pub outcome: TransactionOutcome,
    /// Whether this transaction originated from a guided flow.
    pub from_guided_flow: bool,
}

impl TransactionRecord {
    const MAX_BODY_PREVIEW: usize = 1024;

    pub fn new(
        id: u64,
        service: impl Into<String>,
        method: impl Into<String>,
        path: impl Into<String>,
        started_at_ms: u64,
    ) -> Self {
        Self {
            id,
            service: service.into(),
            method: method.into(),
            path: path.into(),
            status: 0,
            operation: None,
            request_body_preview: None,
            response_body_preview: None,
            started_at_ms,
            duration_ms: None,
            outcome: TransactionOutcome::Pending,
            from_guided_flow: false,
        }
    }

    pub fn with_operation(mut self, op: impl Into<String>) -> Self {
        self.operation = Some(op.into());
        self
    }

    pub fn with_request_body(mut self, body: impl AsRef<str>) -> Self {
        self.request_body_preview = Some(truncate(body.as_ref(), Self::MAX_BODY_PREVIEW));
        self
    }

    pub fn with_guided(mut self) -> Self {
        self.from_guided_flow = true;
        self
    }

    /// Finalise the record with a response status, body, and duration.
    pub fn complete(
        mut self,
        status: u16,
        response_body: impl AsRef<str>,
        duration_ms: u64,
    ) -> Self {
        self.status = status;
        self.outcome = TransactionOutcome::from_status(status);
        self.response_body_preview = Some(truncate(response_body.as_ref(), Self::MAX_BODY_PREVIEW));
        self.duration_ms = Some(duration_ms);
        self
    }
}

// ---------------------------------------------------------------------------
// Transaction log
// ---------------------------------------------------------------------------

/// Circular transaction log with configurable capacity.
///
/// Oldest entries are evicted when the log is full.  Designed for real-time
/// display in the Studio Transactions tab.
#[derive(Debug, Clone)]
pub struct TransactionLog {
    max_entries: usize,
    entries: VecDeque<TransactionRecord>,
    next_id: u64,
}

impl TransactionLog {
    /// Create a new log with the given maximum number of retained entries.
    pub fn new(max_entries: usize) -> Self {
        Self {
            max_entries: max_entries.max(1),
            entries: VecDeque::new(),
            next_id: 1,
        }
    }

    /// Allocate the next transaction ID without inserting a record.
    pub fn next_id(&self) -> u64 {
        self.next_id
    }

    /// Append a new transaction record.  Evicts the oldest entry if at
    /// capacity.  Returns the assigned ID.
    pub fn push(&mut self, mut record: TransactionRecord) -> u64 {
        let id = self.next_id;
        record.id = id;
        self.next_id += 1;

        self.entries.push_front(record);
        while self.entries.len() > self.max_entries {
            self.entries.pop_back();
        }
        id
    }

    /// Iterate entries in newest-first order.
    pub fn list(&self) -> impl Iterator<Item = &TransactionRecord> {
        self.entries.iter()
    }

    /// Look up a single record by ID.
    pub fn get(&self, id: u64) -> Option<&TransactionRecord> {
        self.entries.iter().find(|r| r.id == id)
    }

    /// Update an existing record in-place (e.g. to finalise a pending entry).
    pub fn update<F>(&mut self, id: u64, f: F) -> bool
    where
        F: FnOnce(&mut TransactionRecord),
    {
        if let Some(record) = self.entries.iter_mut().find(|r| r.id == id) {
            f(record);
            true
        } else {
            false
        }
    }

    /// All transactions for one service, newest first.
    pub fn for_service<'a>(
        &'a self,
        service: &'a str,
    ) -> impl Iterator<Item = &'a TransactionRecord> {
        self.entries.iter().filter(move |r| r.service == service)
    }

    /// Filter by outcome.
    pub fn by_outcome(
        &self,
        outcome: TransactionOutcome,
    ) -> impl Iterator<Item = &TransactionRecord> {
        self.entries.iter().filter(move |r| r.outcome == outcome)
    }

    /// Number of stored entries.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Clear all entries (does not reset the ID counter).
    pub fn clear(&mut self) {
        self.entries.clear();
    }

    /// Clear all entries belonging to a specific service.
    ///
    /// Returns the number of removed records.
    pub fn clear_service(&mut self, service: &str) -> usize {
        let before = self.entries.len();
        self.entries.retain(|r| r.service != service);
        before.saturating_sub(self.entries.len())
    }

    /// Summary statistics for display in the Transactions tab header.
    pub fn summary(&self) -> TransactionSummary {
        let mut success = 0usize;
        let mut client_error = 0usize;
        let mut server_error = 0usize;
        let mut pending = 0usize;
        let mut total_duration_ms = 0u64;
        let mut completed = 0usize;

        for r in &self.entries {
            match r.outcome {
                TransactionOutcome::Success => success += 1,
                TransactionOutcome::ClientError => client_error += 1,
                TransactionOutcome::ServerError => server_error += 1,
                TransactionOutcome::Pending => pending += 1,
            }
            if let Some(d) = r.duration_ms {
                total_duration_ms += d;
                completed += 1;
            }
        }

        TransactionSummary {
            total: self.entries.len(),
            success,
            client_error,
            server_error,
            pending,
            avg_duration_ms: if completed > 0 {
                Some(total_duration_ms / completed as u64)
            } else {
                None
            },
        }
    }
}

/// Aggregated counts for display in the Transactions tab header badge.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransactionSummary {
    pub total: usize,
    pub success: usize,
    pub client_error: usize,
    pub server_error: usize,
    pub pending: usize,
    /// Mean duration in milliseconds across completed transactions.
    pub avg_duration_ms: Option<u64>,
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn truncate(s: &str, max_bytes: usize) -> String {
    if s.len() <= max_bytes {
        s.to_string()
    } else {
        format!("{}…", &s[..max_bytes])
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn make_record(service: &str, status: u16) -> TransactionRecord {
        TransactionRecord::new(0, service, "POST", "/", 1000).complete(status, "body", 42)
    }

    #[test]
    fn outcome_classification() {
        assert_eq!(
            TransactionOutcome::from_status(200),
            TransactionOutcome::Success
        );
        assert_eq!(
            TransactionOutcome::from_status(404),
            TransactionOutcome::ClientError
        );
        assert_eq!(
            TransactionOutcome::from_status(500),
            TransactionOutcome::ServerError
        );
        assert_eq!(
            TransactionOutcome::from_status(0),
            TransactionOutcome::Pending
        );
    }

    #[test]
    fn push_assigns_sequential_ids() {
        let mut log = TransactionLog::new(10);
        let id1 = log.push(make_record("s3", 200));
        let id2 = log.push(make_record("sqs", 200));
        assert_eq!(id1, 1);
        assert_eq!(id2, 2);
    }

    #[test]
    fn log_evicts_oldest_when_full() {
        let mut log = TransactionLog::new(3);
        for _ in 0..5 {
            log.push(make_record("s3", 200));
        }
        assert_eq!(log.len(), 3);
        // Newest entry should be id=5
        assert_eq!(log.list().next().map(|r| r.id), Some(5));
    }

    #[test]
    fn filter_by_service() {
        let mut log = TransactionLog::new(20);
        log.push(make_record("s3", 200));
        log.push(make_record("sqs", 200));
        log.push(make_record("s3", 404));

        let s3_count = log.for_service("s3").count();
        assert_eq!(s3_count, 2);
    }

    #[test]
    fn update_finalises_pending_record() {
        let mut log = TransactionLog::new(10);
        let pending = TransactionRecord::new(0, "lambda", "POST", "/invoke", 0);
        let id = log.push(pending);

        let updated = log.update(id, |r| {
            r.status = 200;
            r.outcome = TransactionOutcome::Success;
            r.duration_ms = Some(17);
        });

        assert!(updated);
        let record = log.get(id).unwrap();
        assert_eq!(record.status, 200);
        assert_eq!(record.duration_ms, Some(17));
    }

    #[test]
    fn summary_aggregates_correctly() {
        let mut log = TransactionLog::new(20);
        log.push(make_record("s3", 200));
        log.push(make_record("s3", 200));
        log.push(make_record("sqs", 404));
        log.push(make_record("lambda", 500));

        let summary = log.summary();
        assert_eq!(summary.total, 4);
        assert_eq!(summary.success, 2);
        assert_eq!(summary.client_error, 1);
        assert_eq!(summary.server_error, 1);
        assert_eq!(summary.avg_duration_ms, Some(42));
    }

    #[test]
    fn clear_removes_all_entries_but_preserves_id_counter() {
        let mut log = TransactionLog::new(10);
        log.push(make_record("s3", 200));
        log.push(make_record("s3", 200));
        log.clear();
        assert!(log.is_empty());
        let next_id = log.push(make_record("s3", 200));
        assert_eq!(next_id, 3);
    }

    #[test]
    fn body_preview_truncates_long_content() {
        let large = "x".repeat(2048);
        let record = TransactionRecord::new(0, "s3", "GET", "/", 0).with_request_body(&large);
        let preview = record.request_body_preview.as_deref().unwrap();
        assert!(preview.len() < large.len());
        assert!(preview.ends_with('…'));
    }
}
