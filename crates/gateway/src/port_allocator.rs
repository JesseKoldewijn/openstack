/// External service port allocator.
///
/// Allocates TCP ports from the range 4510–4560 (inclusive) for external
/// services that need dedicated ports (e.g., ElasticSearch, OpenSearch, etc.).
/// Ports are handed out sequentially and never reused within a process lifetime.
///
/// This matches LocalStack's behaviour where `EXTERNAL_SERVICE_PORTS_START` and
/// `EXTERNAL_SERVICE_PORTS_END` control the range.
use std::sync::atomic::{AtomicU16, Ordering};

/// Start of the external service port range (inclusive).
pub const EXTERNAL_SERVICE_PORTS_START: u16 = 4510;
/// End of the external service port range (inclusive).
pub const EXTERNAL_SERVICE_PORTS_END: u16 = 4560;

/// Global atomic counter tracking the next port to allocate.
static NEXT_PORT: AtomicU16 = AtomicU16::new(EXTERNAL_SERVICE_PORTS_START);

/// Allocate the next available external service port.
///
/// Returns `None` when the range is exhausted (all 51 ports have been allocated).
pub fn allocate_port() -> Option<u16> {
    loop {
        let current = NEXT_PORT.load(Ordering::Relaxed);
        if current > EXTERNAL_SERVICE_PORTS_END {
            return None;
        }
        // Try to atomically claim `current`.
        match NEXT_PORT.compare_exchange(current, current + 1, Ordering::SeqCst, Ordering::Relaxed)
        {
            Ok(port) => return Some(port),
            Err(_) => continue, // Another thread raced; retry.
        }
    }
}

/// Reset the allocator back to the start of the range.
///
/// **Only intended for use in tests** — calling this in production will
/// cause port reuse, which is almost certainly wrong.
#[cfg(test)]
pub fn reset_allocator() {
    NEXT_PORT.store(EXTERNAL_SERVICE_PORTS_START, Ordering::SeqCst);
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;

    static TEST_MUTEX: Mutex<()> = Mutex::new(());

    #[test]
    fn test_allocate_sequential() {
        let _guard = TEST_MUTEX
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        reset_allocator();
        assert_eq!(allocate_port(), Some(EXTERNAL_SERVICE_PORTS_START));
        assert_eq!(allocate_port(), Some(EXTERNAL_SERVICE_PORTS_START + 1));
        assert_eq!(allocate_port(), Some(EXTERNAL_SERVICE_PORTS_START + 2));
    }

    #[test]
    fn test_ports_in_range() {
        let _guard = TEST_MUTEX
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        reset_allocator();
        while let Some(port) = allocate_port() {
            assert!(port >= EXTERNAL_SERVICE_PORTS_START);
            assert!(port <= EXTERNAL_SERVICE_PORTS_END);
        }
    }

    #[test]
    fn test_exhausted_returns_none() {
        let _guard = TEST_MUTEX
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        reset_allocator();
        // Consume all ports.
        while allocate_port().is_some() {}
        assert!(allocate_port().is_none());
    }
}
