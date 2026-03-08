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
        let key = AccountRegionKey::new(account_id, region);
        self.stores.entry(key).or_default()
    }

    /// Get an immutable reference to the store for a given account + region, if it exists.
    pub fn get(
        &self,
        account_id: &str,
        region: &str,
    ) -> Option<dashmap::mapref::one::Ref<'_, AccountRegionKey, S>> {
        let key = AccountRegionKey::new(account_id, region);
        self.stores.get(&key)
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
        self.stores.clear();
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
        self.stores.clear();
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
}
