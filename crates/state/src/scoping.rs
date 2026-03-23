use std::borrow::Borrow;
use std::hash::{Hash, Hasher};

pub type AccountId = String;
pub type Region = String;

/// A key for per-account, per-region state isolation.
///
/// Internally stores `"account_id\0region"` as a single `String` so that
/// `AccountRegionKey: Borrow<str>` can be implemented.  This lets
/// `DashMap::get` look up entries with a `&str` key built on the stack,
/// eliminating two heap allocations per read-only store lookup.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccountRegionKey(String);

impl AccountRegionKey {
    pub fn new(account_id: &str, region: &str) -> Self {
        let mut s = String::with_capacity(account_id.len() + 1 + region.len());
        s.push_str(account_id);
        s.push('\0'); // null byte separator (never appears in AWS IDs or regions)
        s.push_str(region);
        Self(s)
    }

    /// Returns the account ID portion of the key.
    #[inline]
    pub fn account_id(&self) -> &str {
        self.0
            .split_once('\0')
            .map(|(a, _)| a)
            .unwrap_or(self.0.as_str())
    }

    /// Returns the region portion of the key.
    #[inline]
    pub fn region(&self) -> &str {
        self.0.split_once('\0').map(|(_, r)| r).unwrap_or("")
    }
}

/// Allow `DashMap<AccountRegionKey, S>.get(combined_key_str)` lookups.
///
/// The hash of `AccountRegionKey` must match the hash of `str` for the same
/// combined key — both delegate to `str::hash`, which is consistent.
impl Borrow<str> for AccountRegionKey {
    fn borrow(&self) -> &str {
        self.0.as_str()
    }
}

impl Hash for AccountRegionKey {
    fn hash<H: Hasher>(&self, state: &mut H) {
        // Must match `<str as Hash>::hash` for Borrow<str> correctness.
        self.0.as_str().hash(state);
    }
}

/// Marker for state that is scoped to a specific account+region.
/// This is the most common scoping -- each account/region pair has its own independent state.
pub struct LocalAttribute;

/// Marker for state that is shared across all regions within an account.
pub struct CrossRegionAttribute;

/// Marker for state that is shared across all accounts and regions (truly global).
pub struct CrossAccountAttribute;
