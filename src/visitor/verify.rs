use std::path::Path;

use crate::cache::DomainCache;
use substrate::{CmnCapsuleEntry, CmnEntry};

use super::crypto::{fetch_spore_manifest, verify_content_hash, verify_spore_with_key_trust};
use super::fetch::fetch_opts;

pub(crate) fn decode_spore_manifest(
    payload: &serde_json::Value,
) -> Result<substrate::Spore, crate::HyphaError> {
    substrate::decode_spore(payload).map_err(|e| {
        crate::HyphaError::new("manifest_failed", format!("Invalid spore manifest: {e}"))
    })
}

/// Remove a directory, emitting a warning if the removal itself fails.
pub(super) fn warn_remove_dir(sink: &dyn crate::EventSink, path: &Path) {
    if let Err(e) = std::fs::remove_dir_all(path) {
        sink.emit(crate::HyphaEvent::Warn {
            message: format!("Failed to clean up directory {}: {}", path.display(), e),
        });
    }
}

/// Verify downloaded content against the expected spore hash.
///
/// A content mismatch is treated as an untrusted delivery failure, not as a
/// durable taste verdict. Only bytes already proven to match the signed content
/// hash should ever influence an automatic toxic verdict.
pub(crate) fn verify_downloaded_content(
    sink: &dyn crate::EventSink,
    cleanup_path: &Path,
    content_path: &Path,
    manifest: &serde_json::Value,
    hash: &str,
    _domain_cache: &DomainCache,
) -> Result<(), crate::HyphaError> {
    if let Err(e) = verify_content_hash(content_path, hash, manifest) {
        warn_remove_dir(sink, cleanup_path);
        let msg = e.to_string();
        if e.code == "hash_compute_failed" {
            return Err(crate::HyphaError::new("hash_compute_failed", msg));
        }
        return Err(crate::HyphaError::new("hash_mismatch", msg));
    }
    Ok(())
}

pub(super) fn mtime_epoch_ms(path: impl AsRef<Path>) -> Option<u64> {
    std::fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as u64)
}

pub(super) fn primary_capsule(entry: &CmnEntry) -> Result<&CmnCapsuleEntry, crate::HyphaError> {
    entry
        .primary_capsule()
        .map_err(|e| crate::HyphaError::new("cmn_invalid", format!("Invalid cmn.json: {}", e)))
}

/// Check whether synapse fallback is allowed for data fetching.
///
/// Allowed when:
/// - The domain has a trusted key in cache (any data from synapse is safe — signature verification
///   will catch tampering), OR
/// - `require_domain_first_key` is false (user accepts the risk of initial key from synapse).
pub(super) fn can_synapse_fallback(
    domain_cache: &DomainCache,
    public_key: &str,
    cfg: &crate::config::CacheConfig,
) -> bool {
    let has_trusted_key = domain_cache.is_key_trusted(
        public_key,
        cfg.key_trust_ttl_s * 1000,
        cfg.clock_skew_tolerance_s * 1000,
    );
    has_trusted_key || !cfg.require_domain_first_key
}

/// Resolve the default synapse to a URL, if configured.
/// Returns None if no synapse is configured or resolution fails.
pub(super) fn resolve_default_synapse_url(
    cfg: &crate::config::HyphaConfig,
) -> Option<(String, Option<String>)> {
    let synapse_domain = cfg.defaults.synapse.as_deref()?;
    let resolved = crate::config::resolve_synapse(Some(synapse_domain), None).ok()?;
    Some((resolved.url, resolved.token_secret))
}

/// Fetch a spore manifest with default Synapse fallback, then verify it with
/// the full key-trust model and decode it.
pub(crate) async fn fetch_verified_spore(
    sink: &dyn crate::EventSink,
    capsule: &substrate::CmnCapsuleEntry,
    hash: &str,
    domain_cache: &DomainCache,
    cmn_ttl_ms: u64,
) -> Result<(serde_json::Value, substrate::Spore), crate::HyphaError> {
    let cfg = crate::config::HyphaConfig::load()?;
    let public_key = capsule.key.as_str();

    let (manifest, from_synapse) = match fetch_spore_manifest(capsule, hash).await {
        Ok(manifest) => (manifest, false),
        Err(domain_err) if can_synapse_fallback(domain_cache, public_key, &cfg.cache) => {
            if let Some((synapse_url, synapse_token)) = resolve_default_synapse_url(&cfg) {
                sink.emit(crate::HyphaEvent::Warn {
                    message: format!(
                        "Domain unreachable for spore manifest, trying synapse: {}",
                        domain_err
                    ),
                });
                let client = substrate::client::http_client(30).map_err(|e| {
                    crate::HyphaError::new("manifest_failed", format!("HTTP client error: {e}"))
                })?;
                let resp = substrate::client::fetch_synapse_spore(
                    &client,
                    &synapse_url,
                    hash,
                    fetch_opts(synapse_token.as_deref()),
                )
                .await
                .map_err(|e| {
                    crate::HyphaError::new(
                        "manifest_failed",
                        format!("Domain: {domain_err}; Synapse: {e}"),
                    )
                })?;
                (resp.result.spore, true)
            } else {
                return Err(domain_err);
            }
        }
        Err(e) => return Err(e),
    };

    let key_trust_ttl_ms = cfg.cache.key_trust_ttl_s * 1000;
    let clock_skew_tolerance_ms = cfg.cache.clock_skew_tolerance_s * 1000;
    let key_trust_refresh_mode = cfg.cache.key_trust_refresh_mode;
    let key_trust_synapse_witness_mode = cfg.cache.key_trust_synapse_witness_mode;
    let resolved_synapse = resolve_default_synapse_url(&cfg);
    let synapse_url = resolved_synapse.as_ref().map(|(url, _)| url.as_str());
    let synapse_token = resolved_synapse
        .as_ref()
        .and_then(|(_, token)| token.as_deref());

    verify_spore_with_key_trust(
        sink,
        &manifest,
        public_key,
        domain_cache,
        cmn_ttl_ms,
        key_trust_ttl_ms,
        clock_skew_tolerance_ms,
        key_trust_refresh_mode,
        key_trust_synapse_witness_mode,
        from_synapse,
        synapse_url,
        synapse_token,
    )
    .await?;

    let spore = decode_spore_manifest(&manifest)?;
    Ok((manifest, spore))
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    /// Set up a temp CMN_HOME and return (dir, DomainCache) for the given domain.
    fn setup_domain_cache(domain: &str) -> (tempfile::TempDir, crate::cache::DomainCache) {
        let dir = tempfile::tempdir().unwrap();
        std::env::set_var("CMN_HOME", dir.path().to_str().unwrap());
        let cache = crate::cache::CacheDir::new().unwrap();
        let dc = cache.domain(domain);
        (dir, dc)
    }

    fn default_cache_config() -> crate::config::CacheConfig {
        crate::config::CacheConfig::default()
    }

    #[test]
    fn test_can_synapse_fallback_no_key_require_domain_true() {
        let _lock = crate::config::ENV_LOCK.lock().unwrap();
        let (_dir, dc) = setup_domain_cache("example.com");

        let cfg = default_cache_config();
        assert!(cfg.require_domain_first_key);
        assert!(!can_synapse_fallback(&dc, "ed25519.test_key", &cfg));

        std::env::remove_var("CMN_HOME");
    }

    #[test]
    fn test_can_synapse_fallback_no_key_require_domain_false() {
        let _lock = crate::config::ENV_LOCK.lock().unwrap();
        let (_dir, dc) = setup_domain_cache("example.com");

        let mut cfg = default_cache_config();
        cfg.require_domain_first_key = false;
        assert!(can_synapse_fallback(&dc, "ed25519.test_key", &cfg));

        std::env::remove_var("CMN_HOME");
    }

    #[test]
    fn test_can_synapse_fallback_with_trusted_key() {
        let _lock = crate::config::ENV_LOCK.lock().unwrap();
        let (_dir, dc) = setup_domain_cache("example.com");

        let key = "ed25519.test_key";
        dc.save_key_trust(key).unwrap();

        let cfg = default_cache_config();
        assert!(cfg.require_domain_first_key);
        assert!(can_synapse_fallback(&dc, key, &cfg));

        std::env::remove_var("CMN_HOME");
    }

    #[test]
    fn test_can_synapse_fallback_wrong_key_not_trusted() {
        let _lock = crate::config::ENV_LOCK.lock().unwrap();
        let (_dir, dc) = setup_domain_cache("example.com");

        dc.save_key_trust("ed25519.key_a").unwrap();

        let cfg = default_cache_config();
        assert!(!can_synapse_fallback(&dc, "ed25519.key_b", &cfg));

        std::env::remove_var("CMN_HOME");
    }

    #[test]
    fn test_verify_downloaded_content_bad_then_good_does_not_mark_toxic() {
        let _lock = crate::config::ENV_LOCK.lock().unwrap();
        let (_dir, dc) = setup_domain_cache("example.com");
        let cleanup = tempfile::tempdir().unwrap();
        let content = cleanup.path().join("content");
        std::fs::create_dir_all(&content).unwrap();
        std::fs::write(content.join("README.md"), "tampered").unwrap();

        let manifest = serde_json::json!({
            "$schema": "https://cmn.dev/schemas/v1/spore.json",
            "capsule": {
                "uri": "cmn://example.com/b3.expected",
                "core": {
                    "name": "test",
                    "domain": "example.com",
                    "key": "ed25519.5XmkQ9vZP8nL",
                    "synopsis": "Test",
                    "intent": ["Testing"],
                    "license": "MIT",
                    "mutations": [],
                    "size_bytes": 8,
                    "updated_at_epoch_ms": 1234567890000_u64,
                    "bonds": [],
                    "tree": {
                        "algorithm": "blob_tree_blake3_nfc",
                        "exclude_names": [],
                        "follow_rules": []
                    }
                },
                "core_signature": "ed25519.fakesig",
                "dist": []
            },
            "capsule_signature": "ed25519.fakesig"
        });

        let err = verify_downloaded_content(
            &crate::NoopSink,
            cleanup.path(),
            &content,
            &manifest,
            "b3.expected",
            &dc,
        )
        .unwrap_err();
        assert_eq!(err.code, "hash_mismatch");

        assert!(
            dc.load_taste("b3.expected").is_none(),
            "unverified delivery failures must not persist a toxic verdict"
        );

        let good_cleanup = tempfile::tempdir().unwrap();
        let good_content = good_cleanup.path().join("content");
        std::fs::create_dir_all(&good_content).unwrap();
        std::fs::write(good_content.join("README.md"), "expected").unwrap();

        let spore = substrate::decode_spore(&manifest).unwrap();
        let entries = crate::tree::walk_dir(
            &good_content,
            &spore.tree().exclude_names,
            &spore.tree().follow_rules,
        )
        .unwrap();
        let tree_hash = spore.tree().compute_hash(&entries).unwrap();
        let valid_hash = spore.computed_uri_hash_from_tree_hash(&tree_hash).unwrap();

        verify_downloaded_content(
            &crate::NoopSink,
            good_cleanup.path(),
            &good_content,
            &manifest,
            &valid_hash,
            &dc,
        )
        .unwrap();
        assert!(
            dc.load_taste(&valid_hash).is_none(),
            "a later valid delivery should not inherit a toxic verdict"
        );

        std::env::remove_var("CMN_HOME");
    }
}
