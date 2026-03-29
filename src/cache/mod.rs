//! Cache management for downloaded spores and domain metadata.
//!
//! Cache location: `$CMN_HOME/hypha/cache/`
//!
//! Cache structure:
//! ```text
//! $CMN_HOME/hypha/cache/
//! └── {domain}/
//!     ├── mycelium/
//!     │   ├── cmn.json             # cached cmn.json entry
//!     │   ├── mycelium.json       # full mycelium manifest
//!     │   └── status.json         # cache status for all items
//!     │
//!     ├── repos/                  # Bare git repositories for spawn/pull
//!     │   └── {root_commit}/      # Repository identified by first commit SHA
//!     │
//!     └── spore/
//!         └── {hash}/
//!             ├── spore.json
//!             └── content/
//! ```

use serde::{Deserialize, Serialize};
use serde_json::json;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use crate::api::Output;
use substrate::{CmnEntry, CmnUri};

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
}

/// Cache status for domain metadata (cmn, mycelium)
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CacheStatus {
    #[serde(default)]
    pub cmn: FetchStatus,
    #[serde(default)]
    pub mycelium: FetchStatus,
}

/// Cache directory structure
pub struct CacheDir {
    pub root: PathBuf,
    pub cmn_ttl_ms: u64,
    pub max_download_bytes: u64,
    pub max_extract_bytes: u64,
    pub max_extract_files: u64,
    pub max_extract_file_bytes: u64,
}

impl CacheDir {
    /// Create a new CacheDir under $CMN_HOME/hypha/cache/ (or [cache] path from config.toml)
    pub fn new() -> Self {
        let cfg = crate::config::HyphaConfig::load();

        let root = match &cfg.cache.path {
            Some(p) => PathBuf::from(p),
            None => crate::config::hypha_dir().join("cache"),
        };

        // Ensure cache root exists with restricted permissions
        if !root.exists() {
            let _ = std::fs::create_dir_all(&root);
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ = std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700));
            }
        }

        Self {
            root,
            cmn_ttl_ms: cfg.cache.cmn_ttl_s * 1000,
            max_download_bytes: cfg.cache.max_download_bytes,
            max_extract_bytes: cfg.cache.max_extract_bytes,
            max_extract_files: cfg.cache.max_extract_files,
            max_extract_file_bytes: cfg.cache.max_extract_file_bytes,
        }
    }

    /// Get the domain cache helper
    pub fn domain(&self, domain: &str) -> DomainCache {
        DomainCache {
            root: self.root.join(domain),
            domain: domain.to_string(),
        }
    }

    /// Get the cache path for a specific spore (legacy compatibility)
    /// Structure: ~/.cmn/cache/{domain}/spore/{hash}/
    pub fn spore_path(&self, domain: &str, hash: &str) -> PathBuf {
        self.domain(domain).spore_path(hash)
    }

    /// List all cached spores
    pub fn list_all(&self) -> Vec<CachedSpore> {
        let mut spores = Vec::new();

        if !self.root.exists() {
            return spores;
        }

        // Iterate through domain directories
        if let Ok(domains) = std::fs::read_dir(&self.root) {
            for domain_entry in domains.filter_map(|e| e.ok()) {
                let domain_path = domain_entry.path();
                if !domain_path.is_dir() {
                    continue;
                }

                let domain = domain_entry.file_name().to_string_lossy().to_string();
                let domain_cache = self.domain(&domain);

                // Iterate through spore directories
                let spore_dir = domain_cache.spore_dir();
                if let Ok(hashes) = std::fs::read_dir(&spore_dir) {
                    for hash_entry in hashes.filter_map(|e| e.ok()) {
                        let hash_path = hash_entry.path();
                        if !hash_path.is_dir() {
                            continue;
                        }

                        let hash_dir = hash_entry.file_name().to_string_lossy().to_string();
                        let hash = hash_dir.replace('_', ":");

                        // Try to read spore.json for metadata
                        let manifest_path = hash_path.join("spore.json");
                        let (name, synopsis) = read_spore_metadata(&manifest_path);

                        // Read taste verdict if present
                        let verdict = {
                            let taste_path = hash_path.join("taste.json");
                            if taste_path.exists() {
                                std::fs::read_to_string(&taste_path)
                                    .ok()
                                    .and_then(|s| {
                                        serde_json::from_str::<TasteVerdictCache>(&s).ok()
                                    })
                                    .map(|v| v.verdict)
                            } else {
                                None
                            }
                        };

                        // Get directory size
                        let size = dir_size(&hash_path);

                        spores.push(CachedSpore {
                            domain: domain.clone(),
                            hash,
                            name,
                            synopsis,
                            path: hash_path,
                            size,
                            verdict,
                        });
                    }
                }
            }
        }

        spores
    }

    /// Remove all cached items
    pub fn clean_all(&self) -> Result<usize, crate::sink::HyphaError> {
        use crate::sink::HyphaError;
        if !self.root.exists() {
            return Ok(0);
        }

        let spores = self.list_all();
        let count = spores.len();

        std::fs::remove_dir_all(&self.root).map_err(|e| {
            HyphaError::new(
                "cache_clean_failed",
                format!("Failed to remove cache directory: {}", e),
            )
        })?;

        Ok(count)
    }
}

impl Default for CacheDir {
    fn default() -> Self {
        Self::new()
    }
}

impl CacheDir {
    /// Create a CacheDir with explicit TTL values (for testing)
    #[cfg(test)]
    pub fn with_root(root: PathBuf) -> Self {
        Self {
            root,
            cmn_ttl_ms: 300 * 1000,
            max_download_bytes: 1024 * 1024 * 1024,
            max_extract_bytes: 512 * 1024 * 1024,
            max_extract_files: 100_000,
            max_extract_file_bytes: 256 * 1024 * 1024,
        }
    }
}

/// Write content atomically with an exclusive file lock to prevent concurrent corruption.
/// Acquires a per-directory `.lock` file before performing the atomic write.
fn locked_write_file(path: &std::path::Path, content: &str) -> Result<(), crate::sink::HyphaError> {
    use crate::sink::HyphaError;
    use fs2::FileExt;

    let parent = path.parent().ok_or_else(|| {
        HyphaError::new("cache_write_failed", "Cannot determine parent directory")
    })?;
    std::fs::create_dir_all(parent).map_err(|e| {
        HyphaError::new(
            "cache_write_failed",
            format!("Failed to create directory: {}", e),
        )
    })?;

    let lock_path = parent.join(".lock");
    let lock_file = std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(&lock_path)
        .map_err(|e| {
            HyphaError::new(
                "cache_write_failed",
                format!("Failed to open lock file: {}", e),
            )
        })?;

    lock_file.lock_exclusive().map_err(|e| {
        HyphaError::new(
            "cache_write_failed",
            format!("Failed to acquire lock: {}", e),
        )
    })?;

    let result = atomic_write_file(path, content);

    let _ = lock_file.unlock();
    result
}

/// Write content to a file atomically: write to a temp file in the same directory,
/// then rename to the final path. This prevents partial/corrupt reads on crash.
fn atomic_write_file(path: &std::path::Path, content: &str) -> Result<(), crate::sink::HyphaError> {
    use crate::sink::HyphaError;
    use std::io::Write;

    let parent = path.parent().ok_or_else(|| {
        HyphaError::new("cache_write_failed", "Cannot determine parent directory")
    })?;

    let tmp_path = parent.join(format!(
        ".tmp.{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    ));

    let mut f = std::fs::File::create(&tmp_path).map_err(|e| {
        HyphaError::new(
            "cache_write_failed",
            format!("Failed to create temp file: {}", e),
        )
    })?;
    f.write_all(content.as_bytes()).map_err(|e| {
        let _ = std::fs::remove_file(&tmp_path);
        HyphaError::new(
            "cache_write_failed",
            format!("Failed to write temp file: {}", e),
        )
    })?;
    f.sync_all().map_err(|e| {
        let _ = std::fs::remove_file(&tmp_path);
        HyphaError::new(
            "cache_write_failed",
            format!("Failed to sync temp file: {}", e),
        )
    })?;
    drop(f);

    std::fs::rename(&tmp_path, path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp_path);
        HyphaError::new(
            "cache_write_failed",
            format!("Failed to rename temp file: {}", e),
        )
    })
}

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

    // --- Repos (bare git repositories for spawn/pull) ---

    /// Get the repos cache directory for this domain
    pub fn repos_dir(&self) -> PathBuf {
        self.root.join("repos")
    }

    /// Get the cache path for a specific repository by root_commit
    ///
    /// The root_commit is the SHA of the first commit in the repository,
    /// which serves as a stable identifier for the repository.
    pub fn repo_path(&self, root_commit: &str) -> PathBuf {
        self.repos_dir().join(root_commit)
    }

    // --- CMN Entry (cmn.json) ---

    /// Get path to cached cmn.json
    pub fn cmn_path(&self) -> PathBuf {
        self.mycelium_dir().join("cmn.json")
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
        use crate::sink::HyphaError;
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

    // --- Full Mycelium manifest ---
    // cmn.json is the lightweight entry point (at /.well-known/cmn.json)
    // mycelium.json is the complete manifest with spores list (fetched via the type:"mycelium" endpoint)

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
        use crate::sink::HyphaError;
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

    // --- Status ---

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
        use crate::sink::HyphaError;
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

    // --- Domain-level taste verdict ---

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
        use crate::sink::HyphaError;
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

    // --- Taste verdict ---

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
        use crate::sink::HyphaError;
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

    // --- Key trust ---

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

    /// Save a key trust entry (domain confirmed this key)
    pub fn save_key_trust(&self, key: &str) -> Result<(), crate::sink::HyphaError> {
        use crate::sink::HyphaError;
        let dir = self.mycelium_dir();
        std::fs::create_dir_all(&dir).map_err(|e| {
            HyphaError::new(
                "cache_write_failed",
                format!("Failed to create mycelium dir: {}", e),
            )
        })?;

        let mut entries = self.load_key_trust();
        // Update existing or add new
        if let Some(entry) = entries.iter_mut().find(|e| e.key == key) {
            entry.confirmed_at_epoch_ms = crate::time::now_epoch_ms();
        } else {
            entries.push(KeyTrustEntry {
                key: key.to_string(),
                confirmed_at_epoch_ms: crate::time::now_epoch_ms(),
            });
        }

        let content = serde_json::to_string_pretty(&entries).map_err(|e| {
            HyphaError::new(
                "cache_write_failed",
                format!("Failed to serialize key trust: {}", e),
            )
        })?;

        locked_write_file(&self.key_trust_path(), &content)
    }

    /// Check if a key is trusted within the given TTL (milliseconds).
    /// Applies clock skew tolerance to prevent false negatives from clock drift.
    pub fn is_key_trusted(&self, key: &str, ttl_ms: u64, clock_skew_tolerance_ms: u64) -> bool {
        let entries = self.load_key_trust();
        let now = crate::time::now_epoch_ms();
        let effective_ttl = ttl_ms.saturating_add(clock_skew_tolerance_ms);
        entries
            .iter()
            .any(|e| e.key == key && now.saturating_sub(e.confirmed_at_epoch_ms) < effective_ttl)
    }

    /// Update cmn.json fetch status
    pub fn update_cmn_status(&self, success: bool, error: Option<&str>) {
        let mut status = self.load_status();
        if success {
            status.cmn = FetchStatus::success();
        } else {
            status.cmn = FetchStatus::failure(error.unwrap_or("Unknown error"), Some(&status.cmn));
        }
        let _ = self.save_status(&status);
    }
}

/// Read spore metadata from manifest file
fn read_spore_metadata(manifest_path: &PathBuf) -> (String, String) {
    if manifest_path.exists() {
        if let Ok(content) = std::fs::read_to_string(manifest_path) {
            if let Ok(manifest) = serde_json::from_str::<substrate::Spore>(&content) {
                return (manifest.capsule.core.name, manifest.capsule.core.synopsis);
            }
        }
    }
    ("unknown".to_string(), String::new())
}

/// Information about a cached spore
pub struct CachedSpore {
    pub domain: String,
    pub hash: String,
    pub name: String,
    pub synopsis: String,
    pub path: PathBuf,
    pub size: u64,
    pub verdict: Option<substrate::TasteVerdict>,
}

/// Calculate directory size iteratively
fn dir_size(path: &Path) -> u64 {
    let mut size = 0;
    let mut stack = vec![path.to_path_buf()];
    while let Some(dir) = stack.pop() {
        if let Ok(entries) = std::fs::read_dir(&dir) {
            for entry in entries.filter_map(|e| e.ok()) {
                let path = entry.path();
                if path.is_file() {
                    size += entry.metadata().map(|m| m.len()).unwrap_or(0);
                } else if path.is_dir() {
                    stack.push(path);
                }
            }
        }
    }
    size
}

/// Handle the `cache list` command
pub fn handle_list(out: &Output) -> ExitCode {
    let cache = CacheDir::new();
    let spores = cache.list_all();

    if spores.is_empty() {
        let data = json!({
            "count": 0,
            "spores": [],
            "total_size": 0,
        });

        return out.ok(data);
    }

    let total_size: u64 = spores.iter().map(|s| s.size).sum();

    let spores_json: Vec<serde_json::Value> = spores
        .iter()
        .map(|s| {
            json!({
                "domain": s.domain,
                "hash": s.hash,
                "name": s.name,
                "synopsis": s.synopsis,
                "path": s.path.display().to_string(),
                "size": s.size,
                "verdict": s.verdict,
            })
        })
        .collect();

    let data = json!({
        "count": spores.len(),
        "spores": spores_json,
        "total_size": total_size,
    });

    out.ok(data)
}

/// Handle the `cache clean` command
pub fn handle_clean(out: &Output, all: bool) -> ExitCode {
    let cache = CacheDir::new();

    if all {
        match cache.clean_all() {
            Ok(count) => {
                let data = json!({
                    "removed": count,
                });
                out.ok(data)
            }
            Err(e) => out.error_hypha(&e),
        }
    } else {
        // For now, just clean all. Later we can add age-based cleanup.
        out.error(
            "invalid_args",
            "Use --all to remove all cached items. Age-based cleanup not yet implemented.",
        )
    }
}

/// Handle the `cache path` command
pub fn handle_path(out: &Output, uri_str: &str) -> ExitCode {
    let uri = match CmnUri::parse(uri_str) {
        Ok(u) => u,
        Err(e) => return out.error("uri_error", &e),
    };

    let hash = match &uri.hash {
        Some(h) => h,
        None => return out.error("uri_error", "spore URI must include a hash"),
    };

    let cache = CacheDir::new();
    let path = cache.spore_path(&uri.domain, hash);

    if !path.exists() {
        return out.error_hint(
            "NOT_CACHED",
            "Spore not cached",
            Some(&format!("run: hypha taste {}", uri_str)),
        );
    }

    let content_path = path.join("content");
    let display_path = if content_path.exists() {
        content_path
    } else {
        path.clone()
    };

    let data = json!({
        "uri": uri_str,
        "path": display_path.display().to_string(),
    });

    out.ok(data)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {

    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_fetch_status_success() {
        let status = FetchStatus::success();
        assert!(status.fetched_at_epoch_ms.is_some());
        assert!(status.failed_at_epoch_ms.is_none());
        assert_eq!(status.retry_count, 0);
    }

    #[test]
    fn test_fetch_status_failure() {
        let status = FetchStatus::failure("connection timeout", None);
        assert!(status.failed_at_epoch_ms.is_some());
        assert_eq!(status.retry_count, 1);
        assert_eq!(status.error, Some("connection timeout".to_string()));
    }

    #[test]
    fn test_fetch_status_retry() {
        let first = FetchStatus::failure("error 1", None);
        let second = FetchStatus::failure("error 2", Some(&first));
        assert_eq!(second.retry_count, 2);
    }

    #[test]
    fn test_domain_cache_paths() {
        let temp = TempDir::new().unwrap();
        let cache = CacheDir::with_root(temp.path().to_path_buf());

        let domain = cache.domain("example.com");
        assert!(domain.cmn_path().ends_with("mycelium/cmn.json"));
        assert!(domain.status_path().ends_with("mycelium/status.json"));
    }

    #[test]
    fn test_spore_path_new_structure() {
        let temp = TempDir::new().unwrap();
        let cache = CacheDir::with_root(temp.path().to_path_buf());

        let path = cache.spore_path("example.com", "b3.3yMR7vZQ9hL");
        assert!(path.to_string_lossy().contains("spore/b3.3yMR7vZQ9hL"));
    }

    #[test]
    fn test_cache_dir_default_ttl_values() {
        let temp = TempDir::new().unwrap();
        let cache = CacheDir::with_root(temp.path().to_path_buf());
        assert_eq!(cache.cmn_ttl_ms, 300 * 1000);
    }

    #[test]
    fn test_cache_dir_custom_ttl() {
        let temp = TempDir::new().unwrap();
        let cache = CacheDir {
            root: temp.path().to_path_buf(),
            cmn_ttl_ms: 10_000,
            max_download_bytes: 1024 * 1024 * 1024,
            max_extract_bytes: 512 * 1024 * 1024,
            max_extract_files: 100_000,
            max_extract_file_bytes: 256 * 1024 * 1024,
        };
        assert_eq!(cache.cmn_ttl_ms, 10_000);
    }

    #[test]
    fn test_cache_dir_from_config_file() {
        // Write a config file with custom TTLs under $CMN_HOME/hypha/config.toml,
        // and verify CacheDir::new() picks them up.
        let _lock = crate::config::ENV_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let hypha_dir = dir.path().join("hypha");
        std::fs::create_dir_all(&hypha_dir).unwrap();
        std::fs::write(hypha_dir.join("config.toml"), "[cache]\ncmn_ttl_s = 30\n").unwrap();

        std::env::set_var("CMN_HOME", dir.path().to_str().unwrap());
        let cache = CacheDir::new();
        std::env::remove_var("CMN_HOME");

        assert_eq!(cache.cmn_ttl_ms, 30 * 1000);
    }

    #[test]
    fn test_cache_dir_from_config_custom_path() {
        let _lock = crate::config::ENV_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let custom_cache = dir.path().join("my-custom-cache");
        let hypha_dir = dir.path().join("hypha");
        std::fs::create_dir_all(&hypha_dir).unwrap();
        std::fs::write(
            hypha_dir.join("config.toml"),
            format!("[cache]\npath = \"{}\"\n", custom_cache.display()),
        )
        .unwrap();

        std::env::set_var("CMN_HOME", dir.path().to_str().unwrap());
        let cache = CacheDir::new();
        std::env::remove_var("CMN_HOME");

        assert_eq!(cache.root, custom_cache);
    }

    #[test]
    fn test_fetch_status_is_fresh_respects_ttl() {
        let status = FetchStatus::success();
        // Just created — should be fresh for any positive TTL
        assert!(status.is_fresh(1000));
        assert!(status.is_fresh(3_600_000));
        // Zero TTL means never fresh
        assert!(!status.is_fresh(0));
    }

    #[test]
    fn test_taste_verdict_roundtrip() {
        let temp = TempDir::new().unwrap();
        let cache = CacheDir::with_root(temp.path().to_path_buf());
        let domain = cache.domain("example.com");

        let verdict = TasteVerdictCache {
            verdict: substrate::TasteVerdict::Safe,
            notes: Some("Reviewed source code".to_string()),
            tasted_at_epoch_ms: 1700000000000,
        };

        domain.save_taste("b3.3yMR7vZQ9hL", &verdict).unwrap();
        let loaded = domain.load_taste("b3.3yMR7vZQ9hL").unwrap();

        assert_eq!(loaded.verdict, substrate::TasteVerdict::Safe);
        assert_eq!(loaded.notes, Some("Reviewed source code".to_string()));
        assert_eq!(loaded.tasted_at_epoch_ms, 1700000000000);
    }

    #[test]
    fn test_taste_verdict_not_found() {
        let temp = TempDir::new().unwrap();
        let cache = CacheDir::with_root(temp.path().to_path_buf());
        let domain = cache.domain("example.com");

        assert!(domain.load_taste("b3.nonexistent").is_none());
    }

    #[test]
    fn test_status_update() {
        let temp = TempDir::new().unwrap();
        let cache = CacheDir::with_root(temp.path().to_path_buf());
        let domain = cache.domain("example.com");

        domain.update_cmn_status(false, Some("404 not found"));
        let status = domain.load_status();
        assert!(status.cmn.failed_at_epoch_ms.is_some());
        assert_eq!(status.cmn.error, Some("404 not found".to_string()));
    }
}
