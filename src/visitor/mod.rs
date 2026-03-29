//! Visitor module for tasting and resolving spores from the network.
//!
//! Resolution flow:
//! 1. Parse CMN URI (cmn://domain/hash)
//! 2. Get cmn.json (from cache or fetch)
//! 3. Use endpoint template to build actual URL
//! 4. Fetch and verify spore manifest
//! 5. Verify signature against public key from cmn.json
//! 6. Download content and verify hash matches URI

use serde::Serialize;
use serde_json::json;
use std::path::Path;
use std::process::ExitCode;

use crate::api::Output;
use crate::cache::{CacheDir, DomainCache, TasteVerdictCache};
use substrate::{CmnCapsuleEntry, CmnEndpoint, CmnEntry, CmnUri, PrettyJson};

mod absorb;
mod bond;
mod common;
mod crypto;
pub(crate) mod extract;
mod grow;
mod lineage;
mod search;
mod sense;
mod spawn;
pub(crate) mod steps;
mod taste;

use common::*;

/// Structured error for archive extraction and file copy operations.
#[derive(Debug, thiserror::Error)]
pub enum ExtractError {
    /// Content is actively dangerous (symlinks, path traversal, zip bombs).
    /// Triggers automatic toxic verdict + cleanup.
    #[error("MALICIOUS: {0}")]
    Malicious(String),
    /// Non-malicious failure (I/O error, unsupported format, etc.).
    #[error("{0}")]
    Failed(String),
}

impl ExtractError {
    pub fn is_malicious(&self) -> bool {
        matches!(self, Self::Malicious(_))
    }
}

impl From<String> for ExtractError {
    fn from(s: String) -> Self {
        Self::Failed(s)
    }
}

impl From<substrate::archive::ExtractError> for ExtractError {
    fn from(e: substrate::archive::ExtractError) -> Self {
        match e {
            substrate::archive::ExtractError::Malicious(msg) => Self::Malicious(msg),
            substrate::archive::ExtractError::Failed(msg) => Self::Failed(msg),
        }
    }
}

// Re-export extract module items for internal use
pub(crate) use extract::{
    decode_delta_to_raw_tar_file, download_and_extract_to_dir, download_file,
    load_old_archive_dictionary, DeltaByteBudget, ExtractLimits,
};

// Re-export all public items so external callers don't break
pub use absorb::{absorb, handle_absorb};
pub use bond::{bond_fetch, handle_bond_fetch};
pub(crate) use common::decode_spore_manifest;
pub use crypto::{
    embedded_spore_author_key, fetch_spore_manifest, get_cmn_entry, verify_content_hash,
    verify_manifest_both_signatures, verify_manifest_two_key_signatures,
    verify_spore_with_key_trust,
};
pub use grow::{grow, handle_grow};
pub use lineage::{handle_lineage, lineage_in, lineage_out};
pub use search::{handle_search, search, search_with_bond};
pub use sense::{handle_sense, sense};
pub use spawn::{handle_spawn, spawn};
pub use taste::{check_taste, check_taste_verdict_for_replicate, handle_taste, taste};

// Cross-submodule imports: these are brought into scope here so that
// submodules using `use super::*` can access sibling module functions.
use bond::bond_in_dir;
use crypto::{verify_manifest_capsule_signature, verify_manifest_core_signature};
use spawn::{
    cache_archive_raw_file, download_and_apply_delta,
    download_and_extract_tarball_cached_with_progress, extract_archive,
};
use substrate::client::BondNode;

/// Thin wrapper around substrate::client::fetch_lineage for internal callers.
async fn fetch_bonds(
    synapse_url: &str,
    hash: &str,
    direction: &str,
    max_depth: u32,
    token: Option<&str>,
) -> Result<substrate::client::BondsResponse, crate::HyphaError> {
    let client = substrate::client::http_client(30).map_err(|e| {
        crate::HyphaError::new(
            "synapse_error",
            format!("Failed to create HTTP client: {}", e),
        )
    })?;
    substrate::client::fetch_lineage(
        &client,
        synapse_url,
        hash,
        direction,
        max_depth,
        fetch_opts(token),
    )
    .await
    .map_err(|e| crate::HyphaError::new("synapse_error", e.to_string()))
}

/// Clone git repository to a directory (shallow)
pub async fn clone_git_to_dir(
    url: &str,
    git_ref: Option<&str>,
    dest: &std::path::Path,
) -> Result<(), crate::git::GitError> {
    std::fs::create_dir_all(dest)?;

    let url = url.to_string();
    let git_ref = git_ref.map(|s| s.to_string());
    let dest = dest.to_path_buf();
    tokio::task::spawn_blocking(move || {
        crate::git::clone_repo(&url, &dest, true)?;
        if let Some(r) = git_ref.as_deref() {
            crate::git::checkout_ref(&dest, r)?;
        }
        Ok::<(), crate::git::GitError>(())
    })
    .await
    .map_err(|e| crate::git::GitError::Command(format!("Git clone task failed: {}", e)))??;

    Ok(())
}

/// Mark a spore as toxic in the local taste cache.
/// Called automatically when malicious archive content is detected.
fn mark_toxic(domain_cache: &crate::cache::DomainCache, hash: &str, reason: &str) {
    let verdict = TasteVerdictCache {
        verdict: substrate::TasteVerdict::Toxic,
        notes: Some(format!("Auto-detected: {}", reason)),
        tasted_at_epoch_ms: crate::time::now_epoch_ms(),
    };
    let _ = domain_cache.save_taste(hash, &verdict);
}

/// Remove a directory, emitting a warning if the removal itself fails.
fn warn_remove_dir(sink: &dyn crate::EventSink, path: &std::path::Path) {
    if let Err(e) = std::fs::remove_dir_all(path) {
        sink.emit(crate::HyphaEvent::Warn {
            message: format!("Failed to clean up directory {}: {}", path.display(), e),
        });
    }
}

/// Fetch a spore to cache — library-level helper.
///
/// Ensures the spore is downloaded, verified, and cached.
/// If already cached, returns immediately.
async fn fetch_spore_to_cache(
    sink: &dyn crate::EventSink,
    cache: &CacheDir,
    uri_str: &str,
) -> Result<(), crate::HyphaError> {
    let uri = CmnUri::parse(uri_str).map_err(|e| crate::HyphaError::new("invalid_uri", e))?;

    let hash = uri
        .hash
        .as_deref()
        .ok_or_else(|| crate::HyphaError::new("invalid_uri", "spore URI must include a hash"))?;

    let domain_cache = cache.domain(&uri.domain);
    let target_path = cache.spore_path(&uri.domain, hash);

    // Already cached — requires both the directory and content/ to exist
    if target_path.exists() {
        if target_path.join("content").exists() {
            sink.emit(crate::HyphaEvent::Progress {
                current: 6,
                total: 6,
                message: "Cached".to_string(),
            });
            return Ok(());
        }
        // Partial cache (e.g. spore.json saved but content download failed) — clean up
        let _ = std::fs::remove_dir_all(&target_path);
    }

    // Step 1: cmn.json
    sink.emit(crate::HyphaEvent::Progress {
        current: 1,
        total: 6,
        message: "Fetching cmn.json".to_string(),
    });
    let entry = get_cmn_entry(sink, &domain_cache, cache.cmn_ttl_ms).await?;

    let capsule = primary_capsule(&entry)?;
    let public_key = capsule.key.clone();
    let ep = &capsule.endpoints;

    // Step 2: Fetching manifest (domain → synapse fallback)
    sink.emit(crate::HyphaEvent::Progress {
        current: 2,
        total: 6,
        message: "Fetching spore manifest".to_string(),
    });
    let cfg = crate::config::HyphaConfig::load();
    let manifest = match fetch_spore_manifest(capsule, hash).await {
        Ok(m) => m,
        Err(domain_err) if can_synapse_fallback(&domain_cache, &public_key, &cfg.cache) => {
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
                resp.result.spore
            } else {
                return Err(domain_err);
            }
        }
        Err(e) => return Err(e),
    };

    // Step 3: Verifying spore with key trust
    sink.emit(crate::HyphaEvent::Progress {
        current: 3,
        total: 6,
        message: "Verifying spore".to_string(),
    });
    let key_trust_ttl_ms = cfg.cache.key_trust_ttl_s * 1000;
    let clock_skew_tolerance_ms = cfg.cache.clock_skew_tolerance_s * 1000;
    let key_trust_refresh_mode = cfg.cache.key_trust_refresh_mode;
    let key_trust_synapse_witness_mode = cfg.cache.key_trust_synapse_witness_mode;
    let resolved_synapse = resolve_default_synapse_url(&cfg);
    let synapse_url = resolved_synapse.as_ref().map(|(url, _)| url.as_str());
    let synapse_token = resolved_synapse
        .as_ref()
        .and_then(|(_, tok)| tok.as_deref());
    verify_spore_with_key_trust(
        sink,
        &manifest,
        &public_key,
        &domain_cache,
        cache.cmn_ttl_ms,
        key_trust_ttl_ms,
        clock_skew_tolerance_ms,
        key_trust_refresh_mode,
        key_trust_synapse_witness_mode,
        false,
        synapse_url,
        synapse_token,
    )
    .await?;
    let spore = decode_spore_manifest(&manifest)?;

    let dist = spore.distributions();
    if dist.is_empty() {
        return Err(crate::HyphaError::new(
            "manifest_failed",
            "No distribution options in spore manifest",
        ));
    }

    // Create target directory
    std::fs::create_dir_all(&target_path).map_err(|e| {
        crate::HyphaError::new("dir_error", format!("Failed to create directory: {}", e))
    })?;

    // Save manifest
    let manifest_path = target_path.join("spore.json");
    std::fs::write(
        &manifest_path,
        serde_json::to_string_pretty(&spore).unwrap_or_default(),
    )
    .map_err(|e| {
        crate::HyphaError::new("write_error", format!("Failed to save manifest: {}", e))
    })?;

    // Step 5: Downloading content
    sink.emit(crate::HyphaEvent::Progress {
        current: 5,
        total: 6,
        message: "Downloading content".to_string(),
    });
    let domain_cache = cache.domain(&uri.domain);

    let archive_endpoints = ep
        .iter()
        .filter(|endpoint| endpoint.kind == "archive")
        .collect::<Vec<_>>();
    let mut downloaded = false;
    for dist_entry in dist {
        if dist_has_type(dist_entry, "archive") {
            for archive_ep in &archive_endpoints {
                let archive_url = build_archive_url_from_endpoint(archive_ep, hash)?;
                match download_and_extract_tarball_cached_with_progress(
                    &archive_url,
                    &target_path,
                    cache,
                    &uri.domain,
                    hash,
                    archive_ep.format.as_deref(),
                    sink,
                )
                .await
                {
                    Ok(_) => {
                        downloaded = true;
                        break;
                    }
                    Err(e) if e.is_malicious() => {
                        warn_remove_dir(sink, &target_path);
                        let msg = e.to_string();
                        mark_toxic(&domain_cache, hash, &msg);
                        return Err(crate::HyphaError::new("TOXIC", msg));
                    }
                    Err(e) => {
                        sink.emit(crate::HyphaEvent::Warn {
                            message: format!("Failed to download from {}: {}", archive_url, e),
                        });
                    }
                }
            }
            if downloaded {
                break;
            }
        } else if let Some(git_url) = dist_git_url(dist_entry) {
            let git_ref = dist_git_ref(dist_entry);
            match clone_git_repo(git_url, git_ref, &target_path).await {
                Ok(_) => {
                    downloaded = true;
                    break;
                }
                Err(e) => {
                    sink.emit(crate::HyphaEvent::Warn {
                        message: format!("Failed to clone from {}: {}", git_url, e),
                    });
                }
            }
        }
    }

    if !downloaded {
        warn_remove_dir(sink, &target_path);
        return Err(crate::HyphaError::new(
            "fetch_failed",
            "Failed to download from any distribution source",
        ));
    }

    // Step 6: Verifying content hash
    sink.emit(crate::HyphaEvent::Progress {
        current: 6,
        total: 6,
        message: "Verifying content hash".to_string(),
    });
    let content_path = target_path.join("content");
    if let Err(e) = verify_content_hash(&content_path, hash, &manifest) {
        warn_remove_dir(sink, &target_path);
        let msg = e.to_string();
        mark_toxic(&domain_cache, hash, &msg);
        return Err(crate::HyphaError::new("TOXIC", msg));
    }

    Ok(())
}

/// Clone a git repository to the cache path (shallow clone for fetch)
async fn clone_git_repo(
    url: &str,
    git_ref: Option<&str>,
    dest: &std::path::Path,
) -> Result<(), crate::git::GitError> {
    let content_dir = dest.join("content");
    std::fs::create_dir_all(&content_dir)?;

    let url = url.to_string();
    let git_ref = git_ref.map(|s| s.to_string());
    tokio::task::spawn_blocking(move || {
        crate::git::clone_repo(&url, &content_dir, true)?;
        if let Some(r) = git_ref.as_deref() {
            crate::git::checkout_ref(&content_dir, r)?;
        }
        Ok::<(), crate::git::GitError>(())
    })
    .await
    .map_err(|e| crate::git::GitError::Command(format!("Git clone task failed: {}", e)))??;

    Ok(())
}

// URI parsing tests are in substrate/src/uri.rs

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {

    use super::*;

    fn sanitize_for_path(input: &str) -> String {
        substrate::local_dir_name(None, Some(input), "spore")
    }

    #[test]
    fn test_sanitize_for_path_basic() {
        assert_eq!(sanitize_for_path("cmn-spec"), "cmn-spec");
        assert_eq!(sanitize_for_path("my_project"), "my_project");
    }

    #[test]
    fn test_sanitize_for_path_spaces() {
        assert_eq!(
            sanitize_for_path("CMN Protocol Specification"),
            "CMN-Protocol-Specification"
        );
        assert_eq!(sanitize_for_path("a  b"), "a--b");
    }

    #[test]
    fn test_sanitize_for_path_forbidden_chars() {
        assert_eq!(sanitize_for_path("foo/bar"), "foo-bar");
        assert_eq!(sanitize_for_path("a:b*c?d"), "a-b-c-d");
    }

    #[test]
    fn test_sanitize_for_path_unicode_preserved() {
        assert_eq!(sanitize_for_path("CMN协议规范"), "CMN协议规范");
        assert_eq!(sanitize_for_path("数据库工具"), "数据库工具");
        assert_eq!(sanitize_for_path("cafe\u{301}-utils"), "cafe\u{301}-utils");
    }

    #[test]
    fn test_sanitize_for_path_empty_fallback() {
        assert_eq!(sanitize_for_path(""), "spore");
        assert_eq!(sanitize_for_path("---"), "spore");
    }

    #[test]
    fn test_sanitize_for_path_traversal_safe() {
        assert_eq!(sanitize_for_path(".."), "spore");
        assert_eq!(sanitize_for_path("."), "spore");
        assert_eq!(sanitize_for_path("../etc"), "-etc");
        assert_eq!(sanitize_for_path(".git"), "git");
        assert_eq!(sanitize_for_path(".cmn"), "cmn");
        assert_eq!(sanitize_for_path("...hidden"), "hidden");
    }

    #[test]
    fn test_sanitize_for_path_control_chars() {
        assert_eq!(sanitize_for_path("foo\0bar"), "foo-bar");
        assert_eq!(sanitize_for_path("\x01\x02"), "spore");
        assert_eq!(sanitize_for_path("ok\x7f"), "ok");
    }

    #[test]
    fn test_spawned_from_hash_present() {
        let manifest = serde_json::json!({
            "$schema": "https://cmn.dev/schemas/v1/spore.json",
            "capsule": {
                "uri": "cmn://example.com/b3.child",
                "core": {
                    "name": "test",
                    "domain": "example.com",
                    "key": "ed25519.5XmkQ9vZP8nL",
                    "synopsis": "Test",
                    "intent": ["Testing"],
                    "license": "MIT",
                    "mutations": [],
                    "size_bytes": 512,
                    "updated_at_epoch_ms": 1700000000000_u64,
                    "bonds": [
                        {"uri": "cmn://example.com/b3.3yMR7vZQ9hL", "relation": "spawned_from"}
                    ],
                    "tree": { "algorithm": "blob_tree_blake3_nfc", "exclude_names": [], "follow_rules": [] }
                },
                "core_signature": "sig",
                "dist": [{"type": "archive"}]
            },
            "capsule_signature": "sig"
        });
        assert_eq!(
            grow::spawned_from_hash(&manifest),
            Some("b3.3yMR7vZQ9hL".to_string())
        );
    }

    #[test]
    fn test_spawned_from_hash_missing() {
        let manifest = serde_json::json!({
            "$schema": "https://cmn.dev/schemas/v1/spore.json",
            "capsule": {
                "uri": "cmn://example.com/b3.child",
                "core": {
                    "name": "test",
                    "domain": "example.com",
                    "key": "ed25519.5XmkQ9vZP8nL",
                    "synopsis": "Test",
                    "intent": ["Testing"],
                    "license": "MIT",
                    "mutations": [],
                    "size_bytes": 512,
                    "updated_at_epoch_ms": 1700000000000_u64,
                    "bonds": [
                        {"uri": "cmn://example.com/b3.8cQnH4xPmZ2v", "relation": "depends_on"}
                    ],
                    "tree": { "algorithm": "blob_tree_blake3_nfc", "exclude_names": [], "follow_rules": [] }
                },
                "core_signature": "sig",
                "dist": [{"type": "archive"}]
            },
            "capsule_signature": "sig"
        });
        assert_eq!(grow::spawned_from_hash(&manifest), None);
    }

    #[test]
    fn test_spawned_from_hash_no_bonds() {
        let manifest = serde_json::json!({
            "$schema": "https://cmn.dev/schemas/v1/spore.json",
            "capsule": {
                "uri": "cmn://example.com/b3.child",
                "core": {
                    "name": "test",
                    "domain": "example.com",
                    "synopsis": "Test",
                    "intent": ["Testing"],
                    "license": "MIT"
                },
                "core_signature": "sig"
            },
            "capsule_signature": "sig"
        });
        assert_eq!(grow::spawned_from_hash(&manifest), None);
    }

    #[test]
    fn test_spawned_from_hash_empty_manifest() {
        let manifest = serde_json::json!({});
        assert_eq!(grow::spawned_from_hash(&manifest), None);
    }

    fn test_client() -> reqwest::Client {
        reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(1))
            .build()
            .unwrap()
    }

    /// Verify substrate::client::search accepts the bond_filter parameter.
    /// Uses a non-routable address so the HTTP call fails fast.
    #[tokio::test]
    async fn test_fetch_search_with_bond() {
        let result = substrate::client::search(
            &test_client(),
            "http://127.0.0.1:1",
            "test",
            None,
            None,
            Some("spawned_from:cmn://d.dev/b3.3yMR7vZQ9hL"),
            5,
            Default::default(),
        )
        .await;
        assert!(result.is_err());
    }

    /// Verify substrate::client::search works without bond_filter.
    #[tokio::test]
    async fn test_fetch_search_without_bond() {
        let result = substrate::client::search(
            &test_client(),
            "http://127.0.0.1:1",
            "test",
            Some("cmn.dev"),
            Some("MIT"),
            None,
            10,
            Default::default(),
        )
        .await;
        assert!(result.is_err());
    }

    /// Verify substrate::client::search with comma-separated bond filters.
    #[tokio::test]
    async fn test_fetch_search_with_multi_bond() {
        let result = substrate::client::search(
            &test_client(),
            "http://127.0.0.1:1",
            "tools",
            None,
            None,
            Some("spawned_from:cmn://a.dev/b3.3yMR7vZQ9hL,follows:cmn://b.dev/b3.8cQnH4xPmZ2v"),
            20,
            Default::default(),
        )
        .await;
        assert!(result.is_err());
    }

    /// search_with_bond with bond_filter=None delegates to the same path as search().
    /// Both should produce the same error when pointed at an unreachable synapse.
    #[tokio::test]
    async fn test_search_with_bond_none_delegates() {
        let result_with_ref = search_with_bond(
            "test",
            Some("http://127.0.0.1:1"),
            None,
            None,
            None,
            None,
            20,
            &crate::NoopSink,
        )
        .await;
        let result_plain = search(
            "test",
            Some("http://127.0.0.1:1"),
            None,
            None,
            None,
            20,
            &crate::NoopSink,
        )
        .await;
        assert!(result_with_ref.is_err());
        assert!(result_plain.is_err());
    }

    /// search_with_bond with a bond_filter should also fail at the HTTP level
    /// (not at argument handling).
    #[tokio::test]
    async fn test_search_with_bond_passes_bond_through() {
        let result = search_with_bond(
            "http client",
            Some("http://127.0.0.1:1"),
            None,
            Some("cmn.dev"),
            Some("MIT"),
            Some("spawned_from:cmn://cmn.dev/b3.3yMR7vZQ9hL"),
            10,
            &crate::NoopSink,
        )
        .await;
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        // Should fail at HTTP, not at bond parsing
        assert!(
            err.contains("synapse_error"),
            "should fail at HTTP level: {}",
            err
        );
    }
}
