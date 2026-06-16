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
use substrate::{CmnEntry, CmnUri, PrettyJson};

mod absorb;
mod bond;
mod crypto;
mod distribution;
pub(crate) mod extract;
mod fetch;
mod grow;
mod lineage;
mod search;
mod sense;
mod spawn;
mod taste;
mod verify;

/// Structured error for archive extraction and file copy operations.
#[derive(Debug, thiserror::Error)]
pub enum ExtractError {
    /// Content is actively dangerous (symlinks, path traversal, zip bombs).
    /// Treated as an unverified delivery failure; callers clean up without
    /// persisting a toxic taste verdict.
    #[error("MALICIOUS: {0}")]
    Malicious(String),
    /// Local receive/cache policy rejected otherwise valid content.
    #[error("{0}")]
    PolicyRejected(String),
    /// Non-malicious failure (I/O error, unsupported format, etc.).
    #[error("{0}")]
    Failed(String),
}

impl ExtractError {
    pub fn is_malicious(&self) -> bool {
        matches!(self, Self::Malicious(_))
    }

    pub fn is_policy_rejected(&self) -> bool {
        matches!(self, Self::PolicyRejected(_))
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
use extract::LimitedWriter;
pub(crate) use extract::{
    decode_delta_to_raw_tar_file, download_and_extract_to_dir, download_file,
    ensure_no_rejected_path_components, load_old_archive_dictionary, rejected_path_component,
    DeltaByteBudget, ExtractLimits,
};

// Re-export all public items so external callers don't break
pub use absorb::{absorb, handle_absorb};
pub use bond::{bond_fetch, handle_bond_fetch};
pub use crypto::{
    embedded_spore_author_key, fetch_spore_manifest, get_cmn_entry, verify_content_hash,
    verify_manifest_two_key_signatures, verify_spore_with_key_trust,
};
use distribution::{
    build_archive_delta_url_from_endpoint, build_archive_url_from_endpoint, dist_git_ref,
    dist_git_url, dist_has_type, is_safe_bond_dir_name,
};
pub(crate) use fetch::fetch_spore_to_cache;
use fetch::{clone_git_to_dir, fetch_bonds, fetch_cmn_json, fetch_opts};
pub use grow::{grow, handle_grow};
pub use lineage::{handle_lineage, lineage_in, lineage_out};
pub use search::{handle_search, search, search_with_bond};
pub use sense::{handle_sense, sense, sense_with_id};
pub use spawn::{handle_spawn, spawn};
pub use taste::{check_taste, check_taste_verdict_for_replicate, handle_taste, taste};
pub(crate) use verify::fetch_verified_spore;
use verify::{
    can_synapse_fallback, mtime_epoch_ms, primary_capsule, resolve_default_synapse_url,
    verify_downloaded_content, warn_remove_dir,
};

// Cross-submodule imports: these are brought into scope here so that
// submodules using `use super::*` can access sibling module functions.
use bond::bond_in_dir;
use crypto::{verify_manifest_capsule_signature, verify_manifest_core_signature};
use spawn::{cache_archive_raw_file, download_and_apply_delta, extract_archive};

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
