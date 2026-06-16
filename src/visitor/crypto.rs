use super::*;
use substrate::{
    decide_key_trust, needs_key_trust_refresh, DomainKeyConfirmation, KeyTrustDecision,
    KeyTrustFailure, KeyTrustRefreshPolicy, KeyTrustWarning, KeyTrustWitnessPolicy,
};
use subtle::ConstantTimeEq;

#[derive(Debug, Clone, Copy)]
struct ConfirmedKeyTrust {
    retired_at_epoch_ms: Option<u64>,
}

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
            match verify_cmn_entry_signature(&capsule) {
                Ok(()) => match domain_cache.validate_and_pin_cmn_state(&capsule) {
                    Ok(()) => return Ok(capsule),
                    Err(e) => {
                        sink.emit(crate::HyphaEvent::Warn {
                            message: format!(
                                "Cached cmn.json for {} failed domain-state verification; refetching: {}",
                                domain_cache.domain, e.message
                            ),
                        });
                        domain_cache.update_cmn_status(false, Some(&e.message));
                    }
                },
                Err(e) => {
                    sink.emit(crate::HyphaEvent::Warn {
                        message: format!(
                            "Cached cmn.json for {} failed signature verification; refetching: {}",
                            domain_cache.domain, e.message
                        ),
                    });
                    domain_cache.update_cmn_status(false, Some(&e.message));
                }
            }
        }
    }

    // Fetch from network
    let capsule = fetch_cmn_json(&domain_cache.domain)
        .await
        .inspect_err(|e| {
            domain_cache.update_cmn_status(false, Some(&e.message));
        })?;

    if let Err(e) = domain_cache.validate_and_pin_cmn_state(&capsule) {
        domain_cache.update_cmn_status(false, Some(&e.message));
        return Err(e);
    }

    // Save to cache
    if let Err(e) = domain_cache.save_cmn(&capsule) {
        sink.emit(crate::HyphaEvent::Warn {
            message: format!("Failed to cache capsule: {}", e),
        });
    }
    domain_cache.update_cmn_status(true, None);

    Ok(capsule)
}

pub(super) fn verify_cmn_entry_signature(entry: &CmnEntry) -> Result<(), crate::HyphaError> {
    let key = entry.primary_key().map_err(|e| {
        crate::HyphaError::new("cmn_signature_failed", format!("Invalid cmn.json: {}", e))
    })?;
    entry.verify_signature(key).map_err(|e| {
        crate::HyphaError::new(
            "cmn_signature_failed",
            format!("cmn.json signature verification failed: {}", e),
        )
    })
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
    substrate::client::fetch_spore_manifest(&client, capsule, hash, fetch_opts(None))
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

/// Verify a spore manifest with the two-key trust model.
///
/// `core.key` is mandatory:
///   1. Verify `core_signature` against `core.key`
///   2. Check if key is trusted (cache hit with valid TTL)
///   3. If not cached: try domain confirmation, then Synapse fallback
///   4. Verify `capsule_signature` against host_key
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
    // core.key is mandatory in the two-key model.
    let spore = substrate::decode_spore(manifest).map_err(|e| {
        crate::HyphaError::new("sig_failed", format!("Invalid spore manifest: {}", e))
    })?;
    let signed_at_epoch_ms = spore.timestamp_ms();
    let key = spore
        .embedded_core_key()
        .map(str::to_string)
        .ok_or_else(|| {
            crate::HyphaError::with_hint(
                "missing_core_key",
                "Spore manifest has no core.key — cannot verify author signature",
                "re-release the spore with a current version of hypha, which embeds core.key",
            )
        })?;
    let author_domain = spore.author_domain().to_string();
    let author_domain_cache;
    let key_domain_cache = if author_domain == domain_cache.domain {
        domain_cache
    } else {
        let cache = crate::cache::CacheDir::new()?;
        author_domain_cache = cache.domain(&author_domain);
        &author_domain_cache
    };

    let author_key = {
        let key = &key;
        // Verify core_signature against embedded key
        spore.verify_core_signature(key).map_err(|e| {
            crate::HyphaError::new(
                "sig_failed",
                format!("Core signature verification failed: {e}"),
            )
        })?;

        let key_trusted_in_cache = key_domain_cache.is_key_trusted_for_time(
            key,
            signed_at_epoch_ms,
            key_trust_ttl_ms,
            clock_skew_tolerance_ms,
        );

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
                        key_domain_cache.domain
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
            let mut confirmed_key_trust = None;
            let domain_confirmation = match try_confirm_key_from_domain(
                sink,
                key_domain_cache,
                key,
                signed_at_epoch_ms,
                cmn_ttl_ms,
            )
            .await
            {
                Ok(Some(confirmation)) => {
                    confirmed_key_trust = Some(confirmation);
                    DomainKeyConfirmation::Confirmed
                }
                Ok(None) => DomainKeyConfirmation::Rejected,
                Err(_) => DomainKeyConfirmation::Unreachable,
            };

            let synapse_confirms_key =
                if matches!(domain_confirmation, DomainKeyConfirmation::Unreachable)
                    && !from_synapse
                {
                    if let Some(url) = synapse_url {
                        Some(
                            ask_synapse_key_trust(
                                url,
                                key,
                                &key_domain_cache.domain,
                                synapse_token,
                            )
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
                        let retired_at_epoch_ms = confirmed_key_trust
                            .and_then(|confirmation| confirmation.retired_at_epoch_ms);
                        let _ = key_domain_cache
                            .save_key_trust_with_retirement(key, retired_at_epoch_ms);
                    }
                    match warning {
                        Some(KeyTrustWarning::SynapseSource) => {
                            sink.emit(crate::HyphaEvent::Warn {
                                message: format!(
                                    "Domain {} unreachable, trusting Synapse source (second-class)",
                                    key_domain_cache.domain
                                ),
                            });
                        }
                        Some(KeyTrustWarning::SynapseWitness) => {
                            sink.emit(crate::HyphaEvent::Warn {
                                message: format!(
                                    "Key trusted via Synapse witness (second-class) for {}",
                                    key_domain_cache.domain
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
                                key, key_domain_cache.domain
                            ),
                        ),
                        KeyTrustFailure::DomainUnreachableWitnessDisabled => {
                            crate::HyphaError::with_hint(
                                "key_untrusted",
                                format!(
                                    "Cannot verify key trust for domain {}: domain offline and Synapse witness fallback is disabled",
                                    key_domain_cache.domain
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
                                    key_domain_cache.domain
                                ),
                            )
                        }
                        KeyTrustFailure::DomainUnreachableNoSynapse => {
                            crate::HyphaError::new(
                                "key_untrusted",
                                format!(
                                    "Cannot verify key trust for domain {}: domain offline and no Synapse configured",
                                    key_domain_cache.domain
                                ),
                            )
                        }
                        KeyTrustFailure::OfflineCacheRequired => {
                            crate::HyphaError::with_hint(
                                "key_untrusted",
                                format!(
                                    "Offline key trust mode requires a valid cached key binding for domain {}",
                                    key_domain_cache.domain
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
    };

    // Verify both signatures (core + capsule)
    spore
        .verify_signatures(host_key, &author_key)
        .map_err(|e| {
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
///   - `Ok(Some(_))` if cmn.json confirms the key for the signed timestamp
///   - `Ok(None)` if the domain is reachable but the key is absent/revoked/expired
///   - `Err(...)` if the domain is unreachable
async fn try_confirm_key_from_domain(
    sink: &dyn crate::EventSink,
    domain_cache: &DomainCache,
    key: &str,
    signed_at_epoch_ms: u64,
    _cmn_ttl_ms: u64,
) -> Result<Option<ConfirmedKeyTrust>, crate::HyphaError> {
    // Fetch cmn.json directly (bypass cache to get fresh data for trust)
    let entry = match fetch_cmn_json(&domain_cache.domain).await {
        Ok(e) => e,
        Err(e) => {
            sink.emit(crate::HyphaEvent::Warn {
                message: format!(
                    "Cannot reach {} for key confirmation: {}",
                    domain_cache.domain, e.message
                ),
            });
            return Err(e);
        }
    };
    domain_cache.validate_and_pin_cmn_state(&entry)?;

    entry
        .primary_key_confirmation_at(key, signed_at_epoch_ms)
        .map(|confirmation| {
            confirmation.map(|confirmation| ConfirmedKeyTrust {
                retired_at_epoch_ms: confirmation.retired_at_epoch_ms(),
            })
        })
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
    Ok(bool::from(capsule.key.as_bytes().ct_eq(key.as_bytes())))
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
    // Decode/walk failures are environmental (transient): they are reported
    // with `hash_compute_failed` so callers don't blacklist the spore as toxic.
    // Only a genuine content/size mismatch yields `hash_mismatch`.
    let spore = substrate::decode_spore(manifest).map_err(|e| {
        crate::HyphaError::new(
            "hash_compute_failed",
            format!("Invalid spore manifest: {}", e),
        )
    })?;
    let entries = crate::tree::walk_dir(
        content_path,
        &spore.tree().exclude_names,
        &spore.tree().follow_rules,
    )
    .map_err(|e| {
        crate::HyphaError::new(
            "hash_compute_failed",
            format!("Failed to walk directory: {}", e),
        )
    })?;
    spore
        .verify_content_hash_and_size(&entries, expected_hash)
        .map_err(|e| crate::HyphaError::new("hash_mismatch", e.to_string()))
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn keypair(seed: u8) -> ([u8; 32], String) {
        let private_key = [seed; 32];
        let signing_key = ed25519_dalek::SigningKey::from_bytes(&private_key);
        let public_key = substrate::format_key(
            substrate::KeyAlgorithm::Ed25519,
            &signing_key.verifying_key().to_bytes(),
        );
        (private_key, public_key)
    }

    fn signed_cmn_entry() -> substrate::CmnEntry {
        let (private_key, public_key) = keypair(7);
        let capsules = vec![substrate::CmnCapsuleEntry {
            uri: "cmn://example.com".to_string(),
            serial: 1,
            key: public_key,
            history: vec![],
            endpoints: vec![substrate::CmnEndpoint {
                kind: "spore".to_string(),
                url: "https://example.com/cmn/spore/{hash}.json".to_string(),
                hash: String::new(),
                hashes: vec![],
                format: None,
                delta_url: None,
            }],
        }];
        let capsule_signature = substrate::compute_signature(
            &capsules,
            substrate::SignatureAlgorithm::Ed25519,
            &private_key,
        )
        .unwrap();
        substrate::CmnEntry {
            schema: substrate::CMN_SCHEMA.to_string(),
            capsules,
            capsule_signature,
        }
    }

    fn signed_spore_manifest(
        author_seed: u8,
        host_seed: u8,
    ) -> (serde_json::Value, String, String) {
        signed_spore_manifest_for_domains("example.com", "example.com", author_seed, host_seed)
    }

    fn signed_spore_manifest_for_domains(
        author_domain: &str,
        host_domain: &str,
        author_seed: u8,
        host_seed: u8,
    ) -> (serde_json::Value, String, String) {
        let (author_private, author_key) = keypair(author_seed);
        let (host_private, host_key) = keypair(host_seed);

        let mut spore = substrate::Spore::new(
            author_domain,
            "signed-spore",
            "Signature test",
            vec!["Verify tampering rejection".to_string()],
            "MIT",
        );
        spore.capsule.uri = format!("cmn://{host_domain}/b3.fake");
        spore.capsule.core.key = author_key.clone();
        spore.capsule.core.updated_at_epoch_ms = 1_700_000_000_000;

        spore.capsule.core_signature = substrate::compute_signature(
            &spore.capsule.core,
            substrate::SignatureAlgorithm::Ed25519,
            &author_private,
        )
        .unwrap();
        spore.capsule_signature = substrate::compute_signature(
            &spore.capsule,
            substrate::SignatureAlgorithm::Ed25519,
            &host_private,
        )
        .unwrap();

        (serde_json::to_value(spore).unwrap(), author_key, host_key)
    }

    fn tamper_signature(signature: &str) -> String {
        let mut chars: Vec<char> = signature.chars().collect();
        let last = chars.last_mut().unwrap();
        *last = if *last == '1' { '2' } else { '1' };
        chars.into_iter().collect()
    }

    #[test]
    fn verify_cmn_entry_signature_accepts_valid_entry() {
        let entry = signed_cmn_entry();
        verify_cmn_entry_signature(&entry).unwrap();
    }

    #[test]
    fn verify_cmn_entry_signature_rejects_tampered_cache_entry() {
        let mut entry = signed_cmn_entry();
        entry.capsules[0].endpoints[0].url =
            "https://evil.example/cmn/spore/{hash}.json".to_string();
        let err = verify_cmn_entry_signature(&entry).unwrap_err();
        assert_eq!(err.code, "cmn_signature_failed");
    }

    #[test]
    fn verify_manifest_core_signature_rejects_tampered_signature() {
        let (mut manifest, author_key, _) = signed_spore_manifest(11, 12);
        let original = manifest["capsule"]["core_signature"]
            .as_str()
            .unwrap()
            .to_string();
        manifest["capsule"]["core_signature"] =
            serde_json::Value::String(tamper_signature(&original));

        let err = verify_manifest_core_signature(&manifest, &author_key).unwrap_err();
        assert_eq!(err.code, "sig_failed");
    }

    #[test]
    fn verify_manifest_core_signature_rejects_tampered_core_content() {
        let (mut manifest, author_key, _) = signed_spore_manifest(11, 12);
        manifest["capsule"]["core"]["synopsis"] = serde_json::Value::String("Tampered".into());

        let err = verify_manifest_core_signature(&manifest, &author_key).unwrap_err();
        assert_eq!(err.code, "sig_failed");
    }

    #[test]
    fn verify_manifest_two_key_signatures_rejects_wrong_domain_key() {
        let (manifest, author_key, _) = signed_spore_manifest(11, 12);
        let (_, wrong_host_key) = keypair(13);

        let err = verify_manifest_two_key_signatures(&manifest, &wrong_host_key, &author_key)
            .unwrap_err();
        assert_eq!(err.code, "sig_failed");
    }

    #[tokio::test(flavor = "current_thread")]
    #[allow(clippy::await_holding_lock)]
    async fn verify_spore_with_key_trust_rejects_unconfirmed_embedded_key() {
        let _lock = crate::config::ENV_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        std::env::set_var("CMN_HOME", dir.path().to_str().unwrap());

        let cache = crate::cache::CacheDir::new().unwrap();
        let domain_cache = cache.domain("example.com");
        let (manifest, _author_key, host_key) = signed_spore_manifest(11, 12);
        domain_cache.save_key_trust(&host_key).unwrap();

        let err = verify_spore_with_key_trust(
            &crate::NoopSink,
            &manifest,
            &host_key,
            &domain_cache,
            0,
            60_000,
            0,
            crate::config::KeyTrustRefreshMode::Offline,
            crate::config::SynapseWitnessMode::RequireDomain,
            false,
            None,
            None,
        )
        .await
        .unwrap_err();
        assert_eq!(err.code, "key_untrusted");

        std::env::remove_var("CMN_HOME");
    }

    #[tokio::test(flavor = "current_thread")]
    #[allow(clippy::await_holding_lock)]
    async fn verify_spore_with_key_trust_accepts_cached_retired_key_before_cutoff() {
        let _lock = crate::config::ENV_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        std::env::set_var("CMN_HOME", dir.path().to_str().unwrap());

        let cache = crate::cache::CacheDir::new().unwrap();
        let domain_cache = cache.domain("example.com");
        let (manifest, author_key, host_key) = signed_spore_manifest(11, 12);
        domain_cache
            .save_key_trust_with_retirement(&author_key, Some(1_700_000_000_000))
            .unwrap();

        let verified_author_key = verify_spore_with_key_trust(
            &crate::NoopSink,
            &manifest,
            &host_key,
            &domain_cache,
            0,
            60_000,
            0,
            crate::config::KeyTrustRefreshMode::Offline,
            crate::config::SynapseWitnessMode::RequireDomain,
            false,
            None,
            None,
        )
        .await
        .unwrap();
        assert_eq!(verified_author_key, author_key);

        std::env::remove_var("CMN_HOME");
    }

    #[tokio::test(flavor = "current_thread")]
    #[allow(clippy::await_holding_lock)]
    async fn verify_spore_with_key_trust_checks_author_domain_for_replicates() {
        let _lock = crate::config::ENV_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        std::env::set_var("CMN_HOME", dir.path().to_str().unwrap());

        let cache = crate::cache::CacheDir::new().unwrap();
        let host_domain_cache = cache.domain("mirror.example");
        let author_domain_cache = cache.domain("author.example");
        let (manifest, author_key, host_key) =
            signed_spore_manifest_for_domains("author.example", "mirror.example", 21, 22);
        author_domain_cache.save_key_trust(&author_key).unwrap();

        let verified_author_key = verify_spore_with_key_trust(
            &crate::NoopSink,
            &manifest,
            &host_key,
            &host_domain_cache,
            0,
            60_000,
            0,
            crate::config::KeyTrustRefreshMode::Offline,
            crate::config::SynapseWitnessMode::RequireDomain,
            false,
            None,
            None,
        )
        .await
        .unwrap();
        assert_eq!(verified_author_key, author_key);

        std::env::remove_var("CMN_HOME");
    }

    #[tokio::test(flavor = "current_thread")]
    #[allow(clippy::await_holding_lock)]
    async fn verify_spore_with_key_trust_rejects_cached_retired_key_after_cutoff() {
        let _lock = crate::config::ENV_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        std::env::set_var("CMN_HOME", dir.path().to_str().unwrap());

        let cache = crate::cache::CacheDir::new().unwrap();
        let domain_cache = cache.domain("example.com");
        let (manifest, author_key, host_key) = signed_spore_manifest(11, 12);
        domain_cache
            .save_key_trust_with_retirement(&author_key, Some(1_699_999_999_999))
            .unwrap();

        let err = verify_spore_with_key_trust(
            &crate::NoopSink,
            &manifest,
            &host_key,
            &domain_cache,
            0,
            60_000,
            0,
            crate::config::KeyTrustRefreshMode::Offline,
            crate::config::SynapseWitnessMode::RequireDomain,
            false,
            None,
            None,
        )
        .await
        .unwrap_err();
        assert_eq!(err.code, "key_untrusted");

        std::env::remove_var("CMN_HOME");
    }
}
