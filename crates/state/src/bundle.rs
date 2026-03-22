use std::borrow::Borrow;
use std::sync::Arc;

use dashmap::DashMap;

use crate::scoping::{AccountId, AccountRegionKey};

/// A concurrent store keyed by (AccountId, Region) providing multi-tenancy isolation.
///
/// This is the Rust equivalent of LocalStack's `AccountRegionBundle`.
pub struct AccountRegionBundle<S: Default + Send + Sync + Clone + 'static> {
    stores: Arc<DashMap<AccountRegionKey, S>>,
}

impl<S: Default + Send + Sync + Clone + 'static> AccountRegionBundle<S> {
    pub fn new() -> Self {
        Self {
            stores: Arc::new(DashMap::new()),
        }
    }

    /// Get or create the store for a given account + region.
    pub fn get_or_create(
        &self,
        account_id: &str,
        region: &str,
    ) -> dashmap::mapref::one::RefMut<'_, AccountRegionKey, S> {
        // `AccountRegionKey::new` performs a single allocation for the combined
        // key — only incurred on the write path (insert-if-absent).
        let key = AccountRegionKey::new(account_id, region);
        self.stores.entry(key).or_default()
    }

    /// Get an immutable reference to the store for a given account + region, if it exists.
    ///
    /// Uses a stack-allocated buffer for the lookup key to avoid heap allocation
    /// on the read hot path.  AWS account IDs (12 chars) + regions (≤ 20 chars)
    /// fit comfortably within the 64-byte stack buffer.
    pub fn get(
        &self,
        account_id: &str,
        region: &str,
    ) -> Option<dashmap::mapref::one::Ref<'_, AccountRegionKey, S>> {
        let a = account_id.as_bytes();
        let r = region.as_bytes();
        let total = a.len() + 1 + r.len(); // separator is one null byte

        if total <= 63 {
            // Fast path: build the combined key on the stack.
            let mut buf = [0u8; 64];
            buf[..a.len()].copy_from_slice(a);
            buf[a.len()] = 0; // null byte separator
            buf[a.len() + 1..total].copy_from_slice(r);
            // SAFETY: `a` and `r` are valid UTF-8 (from `&str`); the null byte
            // is valid in a UTF-8 `str` (it is a one-byte sequence U+0000).
            let key_str = unsafe { std::str::from_utf8_unchecked(&buf[..total]) };
            self.stores.get(key_str)
        } else {
            // Slow path: fall back to a heap-allocated key for unusually long
            // account IDs or region strings (should not occur in practice).
            let key = AccountRegionKey::new(account_id, region);
            self.stores.get(key.borrow() as &str)
        }
    }

    /// Get a mutable reference to the store for a given account + region, if it exists.
    ///
    /// Uses the same stack-allocated lookup key optimization as [`get`] to
    /// avoid heap allocation on the write hot path.
    pub fn get_mut(
        &self,
        account_id: &str,
        region: &str,
    ) -> Option<dashmap::mapref::one::RefMut<'_, AccountRegionKey, S>> {
        let a = account_id.as_bytes();
        let r = region.as_bytes();
        let total = a.len() + 1 + r.len(); // separator is one null byte

        if total <= 63 {
            // Fast path: build the combined key on the stack.
            let mut buf = [0u8; 64];
            buf[..a.len()].copy_from_slice(a);
            buf[a.len()] = 0; // null byte separator
            buf[a.len() + 1..total].copy_from_slice(r);
            // SAFETY: `a` and `r` are valid UTF-8 (from `&str`); the null byte
            // is valid in a UTF-8 `str` (it is a one-byte sequence U+0000).
            let key_str = unsafe { std::str::from_utf8_unchecked(&buf[..total]) };
            self.stores.get_mut(key_str)
        } else {
            // Slow path: fall back to a heap-allocated key for unusually long
            // account IDs or region strings (should not occur in practice).
            let key = AccountRegionKey::new(account_id, region);
            self.stores.get_mut(key.borrow() as &str)
        }
    }

    /// Returns all (key, store) pairs (for iteration/serialization).
    pub fn iter(
        &self,
    ) -> impl Iterator<Item = dashmap::mapref::multiple::RefMulti<'_, AccountRegionKey, S>> {
        self.stores.iter()
    }

    /// Returns the number of account+region combinations with state.
    pub fn len(&self) -> usize {
        self.stores.len()
    }

    /// Returns true if there is no state stored.
    pub fn is_empty(&self) -> bool {
        self.stores.is_empty()
    }

    /// Clears all state from all accounts and regions.
    pub fn clear(&self) {
        self.stores.clear()
    }
}

impl<S: Default + Send + Sync + Clone + 'static> Default for AccountRegionBundle<S> {
    fn default() -> Self {
        Self::new()
    }
}

impl<S: Default + Send + Sync + Clone + 'static> Clone for AccountRegionBundle<S> {
    fn clone(&self) -> Self {
        Self {
            stores: Arc::clone(&self.stores),
        }
    }
}

/// A concurrent store keyed by AccountId only (cross-region state).
pub struct AccountBundle<S: Default + Send + Sync + Clone + 'static> {
    stores: Arc<DashMap<AccountId, S>>,
}

impl<S: Default + Send + Sync + Clone + 'static> AccountBundle<S> {
    pub fn new() -> Self {
        Self {
            stores: Arc::new(DashMap::new()),
        }
    }

    pub fn get_or_create(
        &self,
        account_id: &str,
    ) -> dashmap::mapref::one::RefMut<'_, AccountId, S> {
        self.stores.entry(account_id.to_string()).or_default()
    }

    pub fn get(&self, account_id: &str) -> Option<dashmap::mapref::one::Ref<'_, AccountId, S>> {
        self.stores.get(account_id)
    }

    pub fn clear(&self) {
        self.stores.clear()
    }
}

impl<S: Default + Send + Sync + Clone + 'static> Default for AccountBundle<S> {
    fn default() -> Self {
        Self::new()
    }
}

impl<S: Default + Send + Sync + Clone + 'static> Clone for AccountBundle<S> {
    fn clone(&self) -> Self {
        Self {
            stores: Arc::clone(&self.stores),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default, Clone)]
    struct TestStore {
        count: u32,
    }

    #[test]
    fn test_account_region_isolation() {
        let bundle: AccountRegionBundle<TestStore> = AccountRegionBundle::new();

        bundle.get_or_create("account-a", "us-east-1").count = 1;
        bundle.get_or_create("account-a", "eu-west-1").count = 2;
        bundle.get_or_create("account-b", "us-east-1").count = 3;

        assert_eq!(bundle.get("account-a", "us-east-1").unwrap().count, 1);
        assert_eq!(bundle.get("account-a", "eu-west-1").unwrap().count, 2);
        assert_eq!(bundle.get("account-b", "us-east-1").unwrap().count, 3);
        // Different account+region should return None (not yet created)
        assert!(bundle.get("account-b", "eu-west-1").is_none());
    }

    #[test]
    fn test_clear() {
        let bundle: AccountRegionBundle<TestStore> = AccountRegionBundle::new();
        bundle.get_or_create("account-a", "us-east-1").count = 42;
        assert_eq!(bundle.len(), 1);
        bundle.clear();
        assert_eq!(bundle.len(), 0);
    }

    #[test]
    fn test_get_is_zero_alloc_compatible() {
        // Verify that get() finds keys inserted via get_or_create().
        let bundle: AccountRegionBundle<TestStore> = AccountRegionBundle::new();
        bundle.get_or_create("123456789012", "us-east-1").count = 99;
        let found = bundle.get("123456789012", "us-east-1");
        assert!(found.is_some());
        assert_eq!(found.unwrap().count, 99);
    }

    #[test]
    fn test_get_mut_is_zero_alloc_compatible() {
        let bundle: AccountRegionBundle<TestStore> = AccountRegionBundle::new();
        bundle.get_or_create("123456789012", "us-east-1").count = 1;

        {
            let mut found = bundle.get_mut("123456789012", "us-east-1");
            assert!(found.is_some());
            found.as_mut().unwrap().count = 7;
        }

        assert_eq!(bundle.get("123456789012", "us-east-1").unwrap().count, 7);
    }
}
