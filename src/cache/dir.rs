use std::path::PathBuf;

use crate::sink::HyphaError;

use super::{
    dir_size, read_spore_metadata, CachedSpore, DomainCache, HyphaConfig, TasteVerdictCache,
};

/// Marker file written into cache directories that hypha created itself.
/// `clean --all` refuses to purge a directory lacking this marker (unless it is
/// the default cache location), so a misconfigured `cache.path` pointing at
/// unrelated data is never wiped.
const CACHE_SENTINEL: &str = ".cmn-cache";

/// Cache directory structure
pub struct CacheDir {
    pub root: PathBuf,
    pub cmn_ttl_ms: u64,
    pub spore_max_download_bytes: u64,
    pub spore_max_extract_bytes: u64,
    pub spore_max_extract_files: u64,
    pub spore_max_extract_file_bytes: u64,
    pub spore_reject_path_components: Vec<String>,
}

impl CacheDir {
    /// Create a new CacheDir under $CMN_HOME/hypha/cache/ (or [cache] path from config.toml)
    pub fn new() -> Result<Self, crate::sink::HyphaError> {
        let cfg = HyphaConfig::load()?;
        Self::from_config(&cfg)
    }

    fn from_config(cfg: &crate::config::HyphaConfig) -> Result<Self, crate::sink::HyphaError> {
        let root = match &cfg.cache.path {
            Some(p) => PathBuf::from(p),
            None => crate::config::hypha_dir().join("cache"),
        };

        if !root.exists() {
            std::fs::create_dir_all(&root).map_err(|e| {
                HyphaError::new(
                    "cache_dir_error",
                    format!("Failed to create cache directory {}: {}", root.display(), e),
                )
            })?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700)).map_err(
                    |e| {
                        HyphaError::new(
                            "cache_dir_error",
                            format!(
                                "Failed to protect cache directory {}: {}",
                                root.display(),
                                e
                            ),
                        )
                    },
                )?;
            }
            // Mark this directory as a hypha-created cache so clean_all can
            // safely purge it later. Best-effort: a missing marker only makes
            // clean more conservative.
            let _ = std::fs::write(root.join(CACHE_SENTINEL), b"cmn hypha cache\n");
        }

        Ok(Self {
            root,
            cmn_ttl_ms: cfg.cache.cmn_ttl_s * 1000,
            spore_max_download_bytes: cfg.cache.spore_max_download_bytes,
            spore_max_extract_bytes: cfg.cache.spore_max_extract_bytes,
            spore_max_extract_files: cfg.cache.spore_max_extract_files,
            spore_max_extract_file_bytes: cfg.cache.spore_max_extract_file_bytes,
            spore_reject_path_components: cfg.cache.spore_reject_path_components.clone(),
        })
    }

    /// Get the domain cache helper
    pub fn domain(&self, domain: &str) -> DomainCache {
        DomainCache {
            root: self.root.join(domain),
            domain: domain.to_string(),
        }
    }

    /// Get the cache path for a specific spore (legacy compatibility)
    pub fn spore_path(&self, domain: &str, hash: &str) -> PathBuf {
        self.domain(domain).spore_path(hash)
    }

    /// List all cached spores
    pub fn list_all(&self) -> Vec<CachedSpore> {
        let mut spores = Vec::new();

        if !self.root.exists() {
            return spores;
        }

        if let Ok(domains) = std::fs::read_dir(&self.root) {
            for domain_entry in domains.filter_map(|e| e.ok()) {
                let domain_path = domain_entry.path();
                if !domain_path.is_dir() {
                    continue;
                }

                let domain = domain_entry.file_name().to_string_lossy().to_string();
                let domain_cache = self.domain(&domain);

                let spore_dir = domain_cache.spore_dir();
                if let Ok(hashes) = std::fs::read_dir(&spore_dir) {
                    for hash_entry in hashes.filter_map(|e| e.ok()) {
                        let hash_path = hash_entry.path();
                        if !hash_path.is_dir() {
                            continue;
                        }

                        let hash_dir = hash_entry.file_name().to_string_lossy().to_string();
                        let hash = hash_dir.replace('_', ":");
                        let manifest_path = hash_path.join("spore.json");
                        let (name, synopsis) = read_spore_metadata(&manifest_path);

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

                        let size_bytes = dir_size(&hash_path);

                        spores.push(CachedSpore {
                            domain: domain.clone(),
                            hash,
                            name,
                            synopsis,
                            path: hash_path,
                            size_bytes,
                            verdict,
                        });
                    }
                }
            }
        }

        spores
    }

    /// Remove all cached items.
    ///
    /// Only purges a directory hypha recognizes as its own — either the default
    /// cache location or one bearing the [`CACHE_SENTINEL`] marker — and removes
    /// only the managed subtrees, leaving the cache root (and marker) in place.
    pub fn clean_all(&self) -> Result<usize, crate::sink::HyphaError> {
        if !self.root.exists() {
            return Ok(0);
        }

        let is_default = self.root == crate::config::hypha_dir().join("cache");
        let sentinel = self.root.join(CACHE_SENTINEL);
        if !is_default && !sentinel.exists() {
            return Err(HyphaError::with_hint(
                "cache_clean_refused",
                format!(
                    "Refusing to clean {}: not a recognized hypha cache (missing {} marker)",
                    self.root.display(),
                    CACHE_SENTINEL
                ),
                "if this really is your cache directory, remove its contents manually",
            ));
        }

        let count = self.list_all().len();

        // Remove only subdirectories (per-domain caches), preserving the root
        // and the sentinel so a user-configured cache.path is never deleted.
        if let Ok(entries) = std::fs::read_dir(&self.root) {
            for entry in entries.filter_map(|e| e.ok()) {
                let path = entry.path();
                if !path.is_dir() {
                    continue;
                }
                std::fs::remove_dir_all(&path).map_err(|e| {
                    HyphaError::new(
                        "cache_clean_failed",
                        format!("Failed to remove {}: {}", path.display(), e),
                    )
                })?;
            }
        }

        Ok(count)
    }
}

impl CacheDir {
    /// Create a CacheDir with explicit TTL values (for testing)
    #[cfg(test)]
    pub fn with_root(root: PathBuf) -> Self {
        Self {
            root,
            cmn_ttl_ms: 300 * 1000,
            spore_max_download_bytes: 1024 * 1024 * 1024,
            spore_max_extract_bytes: 512 * 1024 * 1024,
            spore_max_extract_files: 100_000,
            spore_max_extract_file_bytes: 256 * 1024 * 1024,
            spore_reject_path_components: vec![".git".to_string(), ".cmn".to_string()],
        }
    }
}
