use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// Status of a single cached item
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct FetchStatus {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fetched_at_epoch_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failed_at_epoch_ms: Option<u64>,
    #[serde(default)]
    pub retry_count: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl FetchStatus {
    pub fn success() -> Self {
        Self {
            fetched_at_epoch_ms: Some(crate::time::now_epoch_ms()),
            failed_at_epoch_ms: None,
            retry_count: 0,
            error: None,
        }
    }

    pub fn failure(error: &str, previous: Option<&FetchStatus>) -> Self {
        Self {
            fetched_at_epoch_ms: previous.and_then(|p| p.fetched_at_epoch_ms),
            failed_at_epoch_ms: Some(crate::time::now_epoch_ms()),
            retry_count: previous.map(|p| p.retry_count + 1).unwrap_or(1),
            error: Some(error.to_string()),
        }
    }

    /// Check if this cache entry is still fresh within the given TTL (milliseconds)
    pub fn is_fresh(&self, ttl_ms: u64) -> bool {
        match self.fetched_at_epoch_ms {
            Some(ts) => crate::time::now_epoch_ms().saturating_sub(ts) < ttl_ms,
            None => false,
        }
    }
}

/// Cached taste verdict for a spore — alias for the substrate standard type.
pub type TasteVerdictCache = substrate::TasteVerdictRecord;

/// A cached key trust entry — records that a domain confirmed ownership of a key
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyTrustEntry {
    pub key: String,
    pub confirmed_at_epoch_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retired_at_epoch_ms: Option<u64>,
}

/// Pinned domain-state for cmn.json anti-rollback checks.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DomainStatePin {
    pub serial: u64,
    pub capsules_digest: String,
    pub current_key: String,
    pub pinned_at_epoch_ms: u64,
}

/// Cache status for domain metadata (cmn, mycelium)
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CacheStatus {
    #[serde(default)]
    pub cmn: FetchStatus,
    #[serde(default)]
    pub mycelium: FetchStatus,
}

/// Information about a cached spore
pub struct CachedSpore {
    pub domain: String,
    pub hash: String,
    pub name: String,
    pub synopsis: String,
    pub path: PathBuf,
    pub size_bytes: u64,
    pub verdict: Option<substrate::TasteVerdict>,
}
