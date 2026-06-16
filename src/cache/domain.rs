use std::path::PathBuf;

use substrate::CmnEntry;

use crate::sink::HyphaError;

use super::{
    locked_update_file, locked_write_file, CacheStatus, DomainStatePin, FetchStatus, KeyTrustEntry,
    TasteVerdictCache,
};

pub const DOMAIN_STATE_JUMP_THRESHOLD: u64 = 1000;

/// Domain-specific cache operations
pub struct DomainCache {
    pub root: PathBuf,
    pub domain: String,
}

impl DomainCache {
    /// Get the mycelium metadata directory
    pub fn mycelium_dir(&self) -> PathBuf {
        self.root.join("mycelium")
    }

    /// Get the spore cache directory
    pub fn spore_dir(&self) -> PathBuf {
        self.root.join("spore")
    }

    /// Get the cache path for a specific spore
    pub fn spore_path(&self, hash: &str) -> PathBuf {
        self.spore_dir().join(hash)
    }

    /// Get the repos cache directory for this domain
    pub fn repos_dir(&self) -> PathBuf {
        self.root.join("repos")
    }

    /// Get the cache path for a specific repository by root_commit
    pub fn repo_path(&self, root_commit: &str) -> PathBuf {
        self.repos_dir().join(root_commit)
    }

    /// Get path to cached cmn.json
    pub fn cmn_path(&self) -> PathBuf {
        self.mycelium_dir().join("cmn.json")
    }

    /// Get path to the local cmn.json domain-state pin.
    pub fn domain_state_path(&self) -> PathBuf {
        self.mycelium_dir().join("domain_state.json")
    }

    /// Load cached cmn.json entry
    pub fn load_cmn(&self) -> Option<CmnEntry> {
        let path = self.cmn_path();
        if path.exists() {
            std::fs::read_to_string(&path)
                .ok()
                .and_then(|s| serde_json::from_str(&s).ok())
        } else {
            None
        }
    }

    /// Save cmn.json entry to cache
    pub fn save_cmn(&self, entry: &CmnEntry) -> Result<(), crate::sink::HyphaError> {
        let dir = self.mycelium_dir();
        std::fs::create_dir_all(&dir).map_err(|e| {
            HyphaError::new(
                "cache_write_failed",
                format!("Failed to create mycelium dir: {}", e),
            )
        })?;

        let content = serde_json::to_string_pretty(entry).map_err(|e| {
            HyphaError::new(
                "cache_write_failed",
                format!("Failed to serialize cmn entry: {}", e),
            )
        })?;

        locked_write_file(&self.cmn_path(), &content)
    }

    /// Load the local anti-rollback pin for this domain.
    pub fn load_domain_state(&self) -> Option<DomainStatePin> {
        let path = self.domain_state_path();
        if path.exists() {
            std::fs::read_to_string(&path)
                .ok()
                .and_then(|s| serde_json::from_str(&s).ok())
        } else {
            None
        }
    }

    /// Verify the fetched cmn.json against the local pin and advance the pin.
    pub fn validate_and_pin_cmn_state(
        &self,
        entry: &CmnEntry,
    ) -> Result<(), crate::sink::HyphaError> {
        let capsule = entry.primary_capsule().map_err(|e| {
            HyphaError::new("domain_state_invalid", format!("Invalid cmn.json: {e}"))
        })?;
        let expected_uri = substrate::build_domain_uri(&self.domain);
        if capsule.uri != expected_uri {
            return Err(HyphaError::new(
                "domain_state_invalid",
                format!(
                    "cmn.json primary capsule uri {} does not match domain {}",
                    capsule.uri, self.domain
                ),
            ));
        }
        let digest = entry.capsules_digest().map_err(|e| {
            HyphaError::new(
                "domain_state_invalid",
                format!("Failed to digest cmn.json capsules: {e}"),
            )
        })?;

        if let Some(pin) = self.load_domain_state() {
            if capsule.serial < pin.serial {
                return Err(HyphaError::new(
                    "domain_state_rollback",
                    format!(
                        "cmn.json serial {} is lower than pinned serial {} for {}",
                        capsule.serial, pin.serial, self.domain
                    ),
                ));
            }
            if capsule.serial == pin.serial && digest != pin.capsules_digest {
                return Err(HyphaError::new(
                    "domain_state_equivocation",
                    format!(
                        "cmn.json serial {} for {} has a different capsules digest than the local pin",
                        capsule.serial, self.domain
                    ),
                ));
            }
            if capsule.serial.saturating_sub(pin.serial) > DOMAIN_STATE_JUMP_THRESHOLD {
                return Err(HyphaError::new(
                    "domain_state_jump",
                    format!(
                        "cmn.json serial jumped from {} to {} for {}",
                        pin.serial, capsule.serial, self.domain
                    ),
                ));
            }
            if capsule.key != pin.current_key {
                capsule
                    .verify_rotation_chain_from(&pin.current_key)
                    .map_err(|e| {
                        HyphaError::new(
                            "domain_key_rotation_unproven",
                            format!(
                                "cmn.json key changed for {} without a valid rotation chain: {e}",
                                self.domain
                            ),
                        )
                    })?;
            }
        }

        self.save_domain_state(&DomainStatePin {
            serial: capsule.serial,
            capsules_digest: digest,
            current_key: capsule.key.clone(),
            pinned_at_epoch_ms: crate::time::now_epoch_ms(),
        })
    }

    fn save_domain_state(&self, pin: &DomainStatePin) -> Result<(), crate::sink::HyphaError> {
        let dir = self.mycelium_dir();
        std::fs::create_dir_all(&dir).map_err(|e| {
            HyphaError::new(
                "cache_write_failed",
                format!("Failed to create mycelium dir: {}", e),
            )
        })?;

        let content = serde_json::to_string_pretty(pin).map_err(|e| {
            HyphaError::new(
                "cache_write_failed",
                format!("Failed to serialize domain state: {}", e),
            )
        })?;

        locked_write_file(&self.domain_state_path(), &content)
    }

    /// Get path to mycelium.json (complete manifest with spores list)
    pub fn mycelium_path(&self) -> PathBuf {
        self.mycelium_dir().join("mycelium.json")
    }

    /// Load cached full mycelium manifest
    pub fn load_mycelium(&self) -> Option<serde_json::Value> {
        let path = self.mycelium_path();
        if path.exists() {
            std::fs::read_to_string(&path)
                .ok()
                .and_then(|s| serde_json::from_str(&s).ok())
        } else {
            None
        }
    }

    /// Save full mycelium manifest to cache
    pub fn save_mycelium(
        &self,
        mycelium: &serde_json::Value,
    ) -> Result<(), crate::sink::HyphaError> {
        let dir = self.mycelium_dir();
        std::fs::create_dir_all(&dir).map_err(|e| {
            HyphaError::new(
                "cache_write_failed",
                format!("Failed to create mycelium dir: {}", e),
            )
        })?;

        let content = crate::mycelium::format_mycelium(mycelium).map_err(|e| {
            HyphaError::new(
                "cache_write_failed",
                format!("Failed to serialize mycelium: {}", e),
            )
        })?;

        locked_write_file(&self.mycelium_path(), &content)
    }

    /// Get path to status.json
    pub fn status_path(&self) -> PathBuf {
        self.mycelium_dir().join("status.json")
    }

    /// Load cache status
    pub fn load_status(&self) -> CacheStatus {
        let path = self.status_path();
        if path.exists() {
            std::fs::read_to_string(&path)
                .ok()
                .and_then(|s| serde_json::from_str(&s).ok())
                .unwrap_or_default()
        } else {
            CacheStatus::default()
        }
    }

    /// Save cache status
    pub fn save_status(&self, status: &CacheStatus) -> Result<(), crate::sink::HyphaError> {
        let dir = self.mycelium_dir();
        std::fs::create_dir_all(&dir).map_err(|e| {
            HyphaError::new(
                "cache_write_failed",
                format!("Failed to create mycelium dir: {}", e),
            )
        })?;

        let content = serde_json::to_string_pretty(status).map_err(|e| {
            HyphaError::new(
                "cache_write_failed",
                format!("Failed to serialize status: {}", e),
            )
        })?;

        locked_write_file(&self.status_path(), &content)
    }

    /// Get path to domain-level taste.json
    pub fn domain_taste_path(&self) -> PathBuf {
        self.mycelium_dir().join("taste.json")
    }

    /// Load cached taste verdict for the domain itself
    pub fn load_domain_taste(&self) -> Option<TasteVerdictCache> {
        let path = self.domain_taste_path();
        if path.exists() {
            std::fs::read_to_string(&path)
                .ok()
                .and_then(|s| serde_json::from_str(&s).ok())
        } else {
            None
        }
    }

    /// Save taste verdict for the domain itself
    pub fn save_domain_taste(
        &self,
        verdict: &TasteVerdictCache,
    ) -> Result<(), crate::sink::HyphaError> {
        let dir = self.mycelium_dir();
        std::fs::create_dir_all(&dir).map_err(|e| {
            HyphaError::new(
                "cache_write_failed",
                format!("Failed to create mycelium dir: {}", e),
            )
        })?;

        let content = serde_json::to_string_pretty(verdict).map_err(|e| {
            HyphaError::new(
                "cache_write_failed",
                format!("Failed to serialize domain taste verdict: {}", e),
            )
        })?;

        locked_write_file(&self.domain_taste_path(), &content)
    }

    /// Get path to taste.json for a spore
    pub fn taste_path(&self, hash: &str) -> PathBuf {
        self.spore_path(hash).join("taste.json")
    }

    /// Load cached taste verdict for a spore
    pub fn load_taste(&self, hash: &str) -> Option<TasteVerdictCache> {
        let path = self.taste_path(hash);
        if path.exists() {
            std::fs::read_to_string(&path)
                .ok()
                .and_then(|s| serde_json::from_str(&s).ok())
        } else {
            None
        }
    }

    /// Save taste verdict for a spore
    pub fn save_taste(
        &self,
        hash: &str,
        verdict: &TasteVerdictCache,
    ) -> Result<(), crate::sink::HyphaError> {
        let dir = self.spore_path(hash);
        std::fs::create_dir_all(&dir).map_err(|e| {
            HyphaError::new(
                "cache_write_failed",
                format!("Failed to create spore dir: {}", e),
            )
        })?;

        let content = serde_json::to_string_pretty(verdict).map_err(|e| {
            HyphaError::new(
                "cache_write_failed",
                format!("Failed to serialize taste verdict: {}", e),
            )
        })?;

        locked_write_file(&self.taste_path(hash), &content)
    }

    /// Get path to key_trust.json for this domain
    pub fn key_trust_path(&self) -> PathBuf {
        self.mycelium_dir().join("key_trust.json")
    }

    /// Load cached key trust entries
    pub fn load_key_trust(&self) -> Vec<KeyTrustEntry> {
        let path = self.key_trust_path();
        if path.exists() {
            std::fs::read_to_string(&path)
                .ok()
                .and_then(|s| serde_json::from_str(&s).ok())
                .unwrap_or_default()
        } else {
            Vec::new()
        }
    }

    /// Save a key trust entry (domain confirmed this key).
    ///
    /// Read-modify-write happens under the directory lock so concurrent
    /// confirmations can't clobber each other's entries.
    pub fn save_key_trust(&self, key: &str) -> Result<(), crate::sink::HyphaError> {
        self.save_key_trust_with_retirement(key, None)
    }

    /// Save a key trust entry with the retirement cutoff advertised by cmn.json.
    pub fn save_key_trust_with_retirement(
        &self,
        key: &str,
        retired_at_epoch_ms: Option<u64>,
    ) -> Result<(), crate::sink::HyphaError> {
        locked_update_file(&self.key_trust_path(), |existing| {
            let mut entries: Vec<KeyTrustEntry> = existing
                .and_then(|s| serde_json::from_str(&s).ok())
                .unwrap_or_default();
            if let Some(entry) = entries.iter_mut().find(|e| e.key == key) {
                entry.confirmed_at_epoch_ms = crate::time::now_epoch_ms();
                entry.retired_at_epoch_ms = retired_at_epoch_ms;
            } else {
                entries.push(KeyTrustEntry {
                    key: key.to_string(),
                    confirmed_at_epoch_ms: crate::time::now_epoch_ms(),
                    retired_at_epoch_ms,
                });
            }
            serde_json::to_string_pretty(&entries).map_err(|e| {
                HyphaError::new(
                    "cache_write_failed",
                    format!("Failed to serialize key trust: {}", e),
                )
            })
        })
    }

    /// Check if a key is trusted within the given TTL (milliseconds).
    /// Applies clock skew tolerance to prevent false negatives from clock drift.
    pub fn is_key_trusted(&self, key: &str, ttl_ms: u64, clock_skew_tolerance_ms: u64) -> bool {
        self.is_key_trusted_entry(key, ttl_ms, clock_skew_tolerance_ms)
            .is_some()
    }

    /// Check if a key is trusted for content signed at the given timestamp.
    pub fn is_key_trusted_for_time(
        &self,
        key: &str,
        signed_at_epoch_ms: u64,
        ttl_ms: u64,
        clock_skew_tolerance_ms: u64,
    ) -> bool {
        self.is_key_trusted_entry(key, ttl_ms, clock_skew_tolerance_ms)
            .map(|entry| {
                entry
                    .retired_at_epoch_ms
                    .map(|retired_at| signed_at_epoch_ms <= retired_at)
                    .unwrap_or(true)
            })
            .unwrap_or(false)
    }

    fn is_key_trusted_entry(
        &self,
        key: &str,
        ttl_ms: u64,
        clock_skew_tolerance_ms: u64,
    ) -> Option<KeyTrustEntry> {
        let entries = self.load_key_trust();
        let now = crate::time::now_epoch_ms();
        let effective_ttl = ttl_ms.saturating_add(clock_skew_tolerance_ms);
        entries
            .into_iter()
            .find(|e| e.key == key && now.saturating_sub(e.confirmed_at_epoch_ms) < effective_ttl)
    }

    /// Update cmn.json fetch status (locked read-modify-write).
    pub fn update_cmn_status(&self, success: bool, error: Option<&str>) {
        let _ = locked_update_file(&self.status_path(), |existing| {
            let mut status: CacheStatus = existing
                .and_then(|s| serde_json::from_str(&s).ok())
                .unwrap_or_default();
            if success {
                status.cmn = FetchStatus::success();
            } else {
                status.cmn =
                    FetchStatus::failure(error.unwrap_or("Unknown error"), Some(&status.cmn));
            }
            serde_json::to_string_pretty(&status).map_err(|e| {
                HyphaError::new(
                    "cache_write_failed",
                    format!("Failed to serialize status: {}", e),
                )
            })
        });
    }
}
