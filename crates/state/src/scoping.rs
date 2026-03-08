use std::hash::Hash;

pub type AccountId = String;
pub type Region = String;

/// A key for per-account, per-region state isolation.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct AccountRegionKey {
    pub account_id: AccountId,
    pub region: Region,
}

impl AccountRegionKey {
    pub fn new(account_id: impl Into<AccountId>, region: impl Into<Region>) -> Self {
        Self {
            account_id: account_id.into(),
            region: region.into(),
        }
    }
}

/// Scoping traits for service state attributes.
///
/// Marker for state that is scoped to a specific account+region.
/// This is the most common scoping -- each account/region pair has its own independent state.
pub struct LocalAttribute;

/// Marker for state that is shared across all regions within an account.
pub struct CrossRegionAttribute;

/// Marker for state that is shared across all accounts and regions (truly global).
pub struct CrossAccountAttribute;
