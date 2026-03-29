use super::*;

/// Build FetchOptions with optional Bearer token.
pub(super) fn fetch_opts(token: Option<&str>) -> substrate::client::FetchOptions {
    match token {
        Some(t) => substrate::client::FetchOptions::with_bearer_token(t),
        None => Default::default(),
    }
}

pub(crate) fn decode_spore_manifest(
    payload: &serde_json::Value,
) -> Result<substrate::Spore, crate::HyphaError> {
    substrate::decode_spore(payload).map_err(|e| {
        crate::HyphaError::new("manifest_failed", format!("Invalid spore manifest: {e}"))
    })
}

pub(super) fn dist_git_url(entry: &substrate::SporeDist) -> Option<&str> {
    entry.git_url()
}

pub(super) fn dist_git_ref(entry: &substrate::SporeDist) -> Option<&str> {
    entry.git_ref()
}

pub(super) fn dist_has_type(entry: &substrate::SporeDist, expected: &str) -> bool {
    entry.kind.as_str() == expected
}

pub(super) fn build_archive_url_from_endpoint(
    endpoint: &CmnEndpoint,
    hash: &str,
) -> Result<String, crate::HyphaError> {
    endpoint.resolve_url(hash).map_err(|e| {
        crate::HyphaError::new(
            "url_error",
            format!(
                "Invalid archive endpoint for format {:?}: {}",
                endpoint.format, e
            ),
        )
    })
}

pub(super) fn build_archive_delta_url_from_endpoint(
    endpoint: &CmnEndpoint,
    hash: &str,
    old_hash: &str,
) -> Result<Option<String>, crate::HyphaError> {
    endpoint.resolve_delta_url(hash, old_hash).map_err(|e| {
        crate::HyphaError::new(
            "url_error",
            format!(
                "Invalid archive delta endpoint for format {:?}: {}",
                endpoint.format, e
            ),
        )
    })
}

/// Validate a bond directory segment used under `.cmn/bonds/`.
/// Accepts either a safe local path segment or a normalized CMN hash.
pub(super) fn is_safe_bond_dir_name(name: &str) -> bool {
    (!name.is_empty() && substrate::local_dir_name(Some(name), None, "") == name)
        || substrate::parse_hash(name).is_ok()
}

/// Fetch cmn.json from network with retry for transient errors.
///
/// Returns `cmn_not_found` when the domain has no cmn.json (HTTP 404),
/// `cmn_failed` for all other errors (network, parse, non-404 HTTP status).
/// Retries up to 2 times with exponential backoff for non-404 errors.
pub(super) async fn fetch_cmn_json(domain: &str) -> Result<CmnEntry, crate::HyphaError> {
    let client = substrate::client::http_client(30)
        .map_err(|e| crate::HyphaError::new("cmn_failed", format!("HTTP client error: {e}")))?;

    let mut last_err = None;
    for attempt in 0..3u32 {
        if attempt > 0 {
            tokio::time::sleep(std::time::Duration::from_millis(
                500 * 2u64.pow(attempt - 1),
            ))
            .await;
        }

        match substrate::client::fetch_cmn_entry(&client, domain, Default::default()).await {
            Ok(entry) => return Ok(entry),
            Err(e) => {
                let msg = e.to_string();
                // Don't retry 404 — the resource genuinely doesn't exist
                if msg.contains("returned 404") {
                    return Err(crate::HyphaError::with_hint(
                        "cmn_not_found",
                        msg,
                        "The domain must serve a cmn.json at /.well-known/cmn.json. Use 'hypha mycelium root' to initialize a CMN site, then deploy the public/ directory.",
                    ));
                }
                last_err = Some(msg);
            }
        }
    }

    Err(crate::HyphaError::new(
        "cmn_failed",
        last_err.unwrap_or_else(|| "Unknown error".to_string()),
    ))
}

pub(super) fn mtime_epoch_ms(path: impl AsRef<std::path::Path>) -> Option<u64> {
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
    domain_cache: &crate::cache::DomainCache,
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

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {

    use super::*;

    /// Set up a temp CMN_HOME and return (dir, DomainCache) for the given domain.
    fn setup_domain_cache(domain: &str) -> (tempfile::TempDir, crate::cache::DomainCache) {
        let dir = tempfile::tempdir().unwrap();
        std::env::set_var("CMN_HOME", dir.path().to_str().unwrap());
        let cache = crate::cache::CacheDir::new();
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

        // No trusted key + require_domain_first_key = true → fallback denied
        assert!(!can_synapse_fallback(&dc, "ed25519.test_key", &cfg));

        std::env::remove_var("CMN_HOME");
    }

    #[test]
    fn test_can_synapse_fallback_no_key_require_domain_false() {
        let _lock = crate::config::ENV_LOCK.lock().unwrap();
        let (_dir, dc) = setup_domain_cache("example.com");

        let mut cfg = default_cache_config();
        cfg.require_domain_first_key = false;

        // No trusted key + require_domain_first_key = false → fallback allowed
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

        // Has trusted key → fallback allowed regardless of require_domain_first_key
        assert!(can_synapse_fallback(&dc, key, &cfg));

        std::env::remove_var("CMN_HOME");
    }

    #[test]
    fn test_can_synapse_fallback_wrong_key_not_trusted() {
        let _lock = crate::config::ENV_LOCK.lock().unwrap();
        let (_dir, dc) = setup_domain_cache("example.com");

        // Save trust for key A, but ask about key B
        dc.save_key_trust("ed25519.key_a").unwrap();

        let cfg = default_cache_config();

        // Key B is not trusted → depends on require_domain_first_key
        assert!(!can_synapse_fallback(&dc, "ed25519.key_b", &cfg));

        std::env::remove_var("CMN_HOME");
    }
}
