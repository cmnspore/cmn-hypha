use super::*;
use substrate::{
    decide_key_trust, needs_key_trust_refresh, DomainKeyConfirmation, KeyTrustDecision,
    KeyTrustFailure, KeyTrustRefreshPolicy, KeyTrustWarning, KeyTrustWitnessPolicy,
};

/// Get cmn.json for a domain (from cache or fresh fetch)
pub async fn get_cmn_entry(
    sink: &dyn crate::EventSink,
    domain_cache: &DomainCache,
    cmn_ttl_ms: u64,
) -> Result<CmnEntry, crate::HyphaError> {
    // Try cache first — return if still fresh
    let status = domain_cache.load_status();
    if status.cmn.is_fresh(cmn_ttl_ms) {
        if let Some(capsule) = domain_cache.load_cmn() {
            return Ok(capsule);
        }
    }

    // Fetch from network
    let capsule = fetch_cmn_json(&domain_cache.domain)
        .await
        .inspect_err(|e| {
            domain_cache.update_cmn_status(false, Some(&e.message));
        })?;

    // Save to cache
    if let Err(e) = domain_cache.save_cmn(&capsule) {
        sink.emit(crate::HyphaEvent::Warn {
            message: format!("Failed to cache capsule: {}", e),
        });
    }
    domain_cache.update_cmn_status(true, None);

    Ok(capsule)
}

/// Fetch and parse a spore manifest using endpoint template
pub async fn fetch_spore_manifest(
    capsule: &substrate::CmnCapsuleEntry,
    hash: &str,
) -> Result<serde_json::Value, crate::HyphaError> {
    let client = substrate::client::http_client(30).map_err(|e| {
        crate::HyphaError::new(
            "manifest_failed",
            format!("Failed to create HTTP client: {}", e),
        )
    })?;
    substrate::client::fetch_spore_manifest(&client, capsule, hash, Default::default())
        .await
        .map_err(|e| crate::HyphaError::new("manifest_failed", e.to_string()))
}

/// Verify `core_signature` for any signed capsule payload.
pub(super) fn verify_manifest_core_signature(
    manifest: &serde_json::Value,
    public_key: &str,
) -> Result<(), crate::HyphaError> {
    substrate::decode_spore(manifest)
        .map_err(|e| {
            crate::HyphaError::new("sig_failed", format!("Invalid spore manifest: {}", e))
        })?
        .verify_core_signature(public_key)
        .map_err(|e| {
            crate::HyphaError::new(
                "sig_failed",
                format!("Core signature verification failed: {}", e),
            )
        })
}

/// Verify `capsule_signature` for any signed capsule payload.
pub(super) fn verify_manifest_capsule_signature(
    manifest: &serde_json::Value,
    public_key: &str,
) -> Result<(), crate::HyphaError> {
    substrate::decode_spore(manifest)
        .map_err(|e| {
            crate::HyphaError::new("sig_failed", format!("Invalid spore manifest: {}", e))
        })?
        .verify_capsule_signature(public_key)
        .map_err(|e| {
            crate::HyphaError::new(
                "sig_failed",
                format!("Capsule signature verification failed: {}", e),
            )
        })
}

/// Verify both core_signature and capsule_signature using separate keys.
pub fn verify_manifest_two_key_signatures(
    manifest: &serde_json::Value,
    host_key: &str,
    author_key: &str,
) -> Result<(), crate::HyphaError> {
    substrate::decode_spore(manifest)
        .map_err(|e| {
            crate::HyphaError::new("sig_failed", format!("Invalid spore manifest: {}", e))
        })?
        .verify_signatures(host_key, author_key)
        .map_err(|e| crate::HyphaError::new("sig_failed", e.to_string()))
}

/// Verify both signatures using a single key (self-hosted convenience wrapper).
pub fn verify_manifest_both_signatures(
    manifest: &serde_json::Value,
    public_key: &str,
) -> Result<(), crate::HyphaError> {
    verify_manifest_two_key_signatures(manifest, public_key, public_key)
}

/// Read the embedded author key from a spore manifest.
pub fn embedded_spore_author_key(payload: &serde_json::Value) -> Option<String> {
    substrate::decode_spore(payload)
        .ok()
        .and_then(|spore| spore.embedded_core_key().map(str::to_string))
}

fn refresh_policy(mode: crate::config::KeyTrustRefreshMode) -> KeyTrustRefreshPolicy {
    match mode {
        crate::config::KeyTrustRefreshMode::Expired => KeyTrustRefreshPolicy::Expired,
        crate::config::KeyTrustRefreshMode::Always => KeyTrustRefreshPolicy::Always,
        crate::config::KeyTrustRefreshMode::Offline => KeyTrustRefreshPolicy::Offline,
    }
}

fn witness_policy(mode: crate::config::SynapseWitnessMode) -> KeyTrustWitnessPolicy {
    match mode {
        crate::config::SynapseWitnessMode::Allow => KeyTrustWitnessPolicy::Allow,
        crate::config::SynapseWitnessMode::RequireDomain => KeyTrustWitnessPolicy::RequireDomain,
    }
}

/// Verify a spore manifest with key trust model.
///
/// If `core.key` is present:
///   1. Verify `core_signature` against `core.key`
///   2. Check if key is trusted (cache hit with valid TTL)
///   3. If not cached: try domain confirmation, then Synapse fallback
///   4. Verify `capsule_signature` against host_key
///
/// If `core.key` is absent (legacy spore):
///   Falls back to the traditional cmn.json-first verification path.
///
/// Returns the author key used for verification.
#[allow(clippy::too_many_arguments)]
pub async fn verify_spore_with_key_trust(
    sink: &dyn crate::EventSink,
    manifest: &serde_json::Value,
    host_key: &str,
    domain_cache: &DomainCache,
    cmn_ttl_ms: u64,
    key_trust_ttl_ms: u64,
    clock_skew_tolerance_ms: u64,
    key_trust_refresh_mode: crate::config::KeyTrustRefreshMode,
    key_trust_synapse_witness_mode: crate::config::SynapseWitnessMode,
    from_synapse: bool,
    synapse_url: Option<&str>,
    synapse_token: Option<&str>,
) -> Result<String, crate::HyphaError> {
    // Try to extract core.key (new model)
    let core_key = embedded_spore_author_key(manifest);

    let author_key = if let Some(ref key) = core_key {
        // Verify core_signature against embedded key
        verify_manifest_core_signature(manifest, key).map_err(|e| {
            crate::HyphaError::new(
                "sig_failed",
                format!("Core signature verification failed: {}", e),
            )
        })?;

        let key_trusted_in_cache =
            domain_cache.is_key_trusted(key, key_trust_ttl_ms, clock_skew_tolerance_ms);

        let should_refresh_key_trust = match needs_key_trust_refresh(
            key_trusted_in_cache,
            refresh_policy(key_trust_refresh_mode),
        ) {
            Ok(should_refresh) => should_refresh,
            Err(KeyTrustFailure::OfflineCacheRequired) => {
                return Err(crate::HyphaError::with_hint(
                    "key_untrusted",
                    format!(
                        "Offline key trust mode requires a valid cached key binding for domain {}",
                        domain_cache.domain
                    ),
                    "Temporarily set cache.key_trust_refresh_mode=expired and verify once online to refresh key trust cache"
                        .to_string(),
                ));
            }
            Err(other) => {
                return Err(crate::HyphaError::new(
                    "key_untrusted",
                    format!("Unexpected key trust failure: {:?}", other),
                ));
            }
        };

        if should_refresh_key_trust {
            let domain_confirmation = match try_confirm_key_from_domain(
                sink,
                &domain_cache.domain,
                key,
                cmn_ttl_ms,
            )
            .await
            {
                Ok(true) => DomainKeyConfirmation::Confirmed,
                Ok(false) => DomainKeyConfirmation::Rejected,
                Err(_) => DomainKeyConfirmation::Unreachable,
            };

            let synapse_confirms_key =
                if matches!(domain_confirmation, DomainKeyConfirmation::Unreachable)
                    && !from_synapse
                {
                    if let Some(url) = synapse_url {
                        Some(
                            ask_synapse_key_trust(url, key, &domain_cache.domain, synapse_token)
                                .await
                                .unwrap_or(false),
                        )
                    } else {
                        None
                    }
                } else {
                    None
                };

            match decide_key_trust(
                domain_confirmation,
                witness_policy(key_trust_synapse_witness_mode),
                from_synapse,
                synapse_confirms_key,
            ) {
                KeyTrustDecision::Trusted {
                    cache_key, warning, ..
                } => {
                    if cache_key {
                        let _ = domain_cache.save_key_trust(key);
                    }
                    match warning {
                        Some(KeyTrustWarning::SynapseSource) => {
                            sink.emit(crate::HyphaEvent::Warn {
                                message: format!(
                                    "Domain {} unreachable, trusting Synapse source (second-class)",
                                    domain_cache.domain
                                ),
                            });
                        }
                        Some(KeyTrustWarning::SynapseWitness) => {
                            sink.emit(crate::HyphaEvent::Warn {
                                message: format!(
                                    "Key trusted via Synapse witness (second-class) for {}",
                                    domain_cache.domain
                                ),
                            });
                        }
                        None => {}
                    }
                }
                KeyTrustDecision::Untrusted { reason } => {
                    let error = match reason {
                        KeyTrustFailure::DomainRejected => crate::HyphaError::new(
                            "key_untrusted",
                            format!(
                                "Key {} not confirmed by domain {}",
                                key, domain_cache.domain
                            ),
                        ),
                        KeyTrustFailure::DomainUnreachableWitnessDisabled => {
                            crate::HyphaError::with_hint(
                                "key_untrusted",
                                format!(
                                    "Cannot verify key trust for domain {}: domain offline and Synapse witness fallback is disabled",
                                    domain_cache.domain
                                ),
                                "Set cache.key_trust_synapse_witness_mode=allow or refresh key trust cache while the domain is online"
                                    .to_string(),
                            )
                        }
                        KeyTrustFailure::DomainUnreachableWitnessRejected => {
                            crate::HyphaError::new(
                                "key_untrusted",
                                format!(
                                    "Cannot verify key trust for domain {}: domain offline and Synapse could not confirm",
                                    domain_cache.domain
                                ),
                            )
                        }
                        KeyTrustFailure::DomainUnreachableNoSynapse => {
                            crate::HyphaError::new(
                                "key_untrusted",
                                format!(
                                    "Cannot verify key trust for domain {}: domain offline and no Synapse configured",
                                    domain_cache.domain
                                ),
                            )
                        }
                        KeyTrustFailure::OfflineCacheRequired => {
                            crate::HyphaError::with_hint(
                                "key_untrusted",
                                format!(
                                    "Offline key trust mode requires a valid cached key binding for domain {}",
                                    domain_cache.domain
                                ),
                                "Temporarily set cache.key_trust_refresh_mode=expired and verify once online to refresh key trust cache"
                                    .to_string(),
                            )
                        }
                    };
                    return Err(error);
                }
            }
        }

        key.clone()
    } else {
        // Legacy path: no core.key, use host_key as author_key
        host_key.to_string()
    };

    // Verify both signatures (core + capsule)
    verify_manifest_two_key_signatures(manifest, host_key, &author_key).map_err(|e| {
        crate::HyphaError::new(
            "sig_failed",
            format!("Signature verification failed: {}", e),
        )
    })?;

    Ok(author_key)
}

/// Try to confirm a key belongs to a domain by fetching cmn.json.
///
/// Returns:
///   - `Ok(true)` if the domain's cmn.json confirms the key (current or previous)
///   - `Ok(false)` if the domain is reachable but the key is not found
///   - `Err(...)` if the domain is unreachable
async fn try_confirm_key_from_domain(
    sink: &dyn crate::EventSink,
    domain: &str,
    key: &str,
    _cmn_ttl_ms: u64,
) -> Result<bool, crate::HyphaError> {
    // Fetch cmn.json directly (bypass cache to get fresh data for trust)
    let entry = match fetch_cmn_json(domain).await {
        Ok(e) => e,
        Err(e) => {
            sink.emit(crate::HyphaEvent::Warn {
                message: format!(
                    "Cannot reach {} for key confirmation: {}",
                    domain, e.message
                ),
            });
            return Err(e);
        }
    };

    entry
        .primary_confirms_key(key)
        .map_err(|e| crate::HyphaError::new("key_untrusted", e.to_string()))
}

/// Ask a Synapse node whether it has seen a key for a domain.
///
/// Fetches cmn.json from synapse and extracts `capsules[0].key`.
/// Returns `Ok(true)` if the Synapse-reported key matches.
async fn ask_synapse_key_trust(
    synapse_url: &str,
    key: &str,
    domain: &str,
    token: Option<&str>,
) -> Result<bool, crate::HyphaError> {
    let client = substrate::client::http_client(10).map_err(|e| {
        crate::HyphaError::new(
            "synapse_error",
            format!("Failed to create HTTP client: {}", e),
        )
    })?;
    let resp =
        substrate::client::fetch_synapse_cmn(&client, synapse_url, domain, fetch_opts(token))
            .await
            .map_err(|e| crate::HyphaError::new("synapse_error", e.to_string()))?;
    let entry: substrate::CmnEntry = serde_json::from_value(resp.result.cmn).map_err(|e| {
        crate::HyphaError::new(
            "synapse_error",
            format!("Failed to parse synapse cmn.json: {}", e),
        )
    })?;
    let capsule = entry
        .primary_capsule()
        .map_err(|e| crate::HyphaError::new("synapse_error", e.to_string()))?;
    Ok(capsule.key == key)
}

/// Verify content hash and size_bytes match the expected values from the manifest.
///
/// The URI hash = blake3(JCS({tree_hash: ..., core: ..., core_signature: ...})).
/// This function recomputes that full hash from the extracted content + manifest,
/// and also verifies size_bytes if declared.
pub fn verify_content_hash(
    content_path: &Path,
    expected_hash: &str,
    manifest: &serde_json::Value,
) -> Result<(), crate::HyphaError> {
    let spore = substrate::decode_spore(manifest).map_err(|e| {
        crate::HyphaError::new("hash_mismatch", format!("Invalid spore manifest: {}", e))
    })?;
    let entries = crate::tree::walk_dir(
        content_path,
        &spore.tree().exclude_names,
        &spore.tree().follow_rules,
    )
    .map_err(|e| {
        crate::HyphaError::new("hash_mismatch", format!("Failed to walk directory: {}", e))
    })?;
    spore
        .verify_content_hash_and_size(&entries, expected_hash)
        .map_err(|e| crate::HyphaError::new("hash_mismatch", e.to_string()))
}
