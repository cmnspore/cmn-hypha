use crate::cache::CacheDir;
use fs2::FileExt;
use substrate::{CmnEntry, CmnUri};

use super::crypto::{get_cmn_entry, verify_cmn_entry_signature};
use super::distribution::{
    build_archive_url_from_endpoint, dist_git_ref, dist_git_url, dist_has_type,
};
use super::spawn::{
    cache_archive_compressed_file, download_and_extract_tarball_cached_with_progress,
};
use super::verify::{
    fetch_verified_spore, primary_capsule, verify_downloaded_content, warn_remove_dir,
};

/// Upper bound for JSON protocol documents fetched by hypha.
pub(super) const JSON_FETCH_MAX_BYTES: usize = 8 * 1024 * 1024;

pub(super) fn json_fetch_opts() -> substrate::client::FetchOptions {
    substrate::client::FetchOptions::with_max_bytes(JSON_FETCH_MAX_BYTES)
}

/// Build FetchOptions with an optional Bearer secret.
pub(super) fn fetch_opts(token_secret: Option<&str>) -> substrate::client::FetchOptions {
    match token_secret {
        Some(token_secret) => substrate::client::FetchOptions::with_bearer_token(token_secret)
            .max_bytes(JSON_FETCH_MAX_BYTES),
        None => json_fetch_opts(),
    }
}

struct SporeCacheLock {
    file: std::fs::File,
}

impl Drop for SporeCacheLock {
    fn drop(&mut self) {
        let _ = self.file.unlock();
    }
}

async fn acquire_spore_cache_lock(
    spore_parent: &std::path::Path,
    hash: &str,
) -> Result<SporeCacheLock, crate::HyphaError> {
    let lock_path = spore_parent.join(format!(".{}.lock", hash));
    let file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&lock_path)
        .map_err(|e| {
            crate::HyphaError::new(
                "cache_lock_failed",
                format!("Failed to open spore cache lock: {}", e),
            )
        })?;

    loop {
        match file.try_lock_exclusive() {
            Ok(()) => return Ok(SporeCacheLock { file }),
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            }
            Err(e) => {
                return Err(crate::HyphaError::new(
                    "cache_lock_failed",
                    format!("Failed to lock spore cache: {}", e),
                ))
            }
        }
    }
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

        match substrate::client::fetch_cmn_entry(&client, domain, json_fetch_opts()).await {
            Ok(entry) => {
                verify_cmn_entry_signature(&entry)?;
                return Ok(entry);
            }
            Err(e) => {
                let msg = e.to_string();
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

/// Thin wrapper around substrate::client::fetch_lineage for internal callers.
pub(super) async fn fetch_bonds(
    synapse_url: &str,
    hash: &str,
    direction: &str,
    max_depth: u32,
    token_secret: Option<&str>,
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
        fetch_opts(token_secret),
    )
    .await
    .map_err(|e| crate::HyphaError::new("synapse_error", e.to_string()))
}

/// Clone git repository to a directory (shallow)
pub async fn clone_git_to_dir(
    url: &str,
    git_ref: Option<&str>,
    dest: &std::path::Path,
    cache: &CacheDir,
) -> Result<(), crate::git::GitError> {
    std::fs::create_dir_all(dest)?;

    let url = url.to_string();
    let git_ref = git_ref.map(|s| s.to_string());
    let dest = dest.to_path_buf();
    let limits = crate::git::GitSizeLimits::new(
        cache.spore_max_extract_bytes,
        cache.spore_max_extract_files,
    );
    tokio::task::spawn_blocking(move || {
        crate::git::clone_repo(&url, &dest, true)?;
        if let Some(r) = git_ref.as_deref() {
            crate::git::checkout_ref(&dest, r)?;
        }
        crate::git::enforce_size_budget(&dest, limits)?;
        Ok::<(), crate::git::GitError>(())
    })
    .await
    .map_err(|e| crate::git::GitError::Command(format!("Git clone task failed: {}", e)))??;

    Ok(())
}

/// Fetch a spore to cache — library-level helper.
///
/// Ensures the spore is downloaded, verified, and cached.
/// If already cached, returns immediately.
pub(crate) async fn fetch_spore_to_cache(
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
    let spore_parent = domain_cache.spore_dir();
    std::fs::create_dir_all(&spore_parent).map_err(|e| {
        crate::HyphaError::new(
            "dir_error",
            format!("Failed to create spore cache directory: {}", e),
        )
    })?;
    let _cache_lock = acquire_spore_cache_lock(&spore_parent, hash).await?;

    if target_path.exists() {
        if target_path.join("content").exists() {
            sink.emit(crate::HyphaEvent::Progress {
                current: 6,
                total: 6,
                message: "Cached".to_string(),
            });
            return Ok(());
        }
        let _ = std::fs::remove_dir_all(&target_path);
    }

    sink.emit(crate::HyphaEvent::Progress {
        current: 1,
        total: 6,
        message: "Fetching cmn.json".to_string(),
    });
    let entry = get_cmn_entry(sink, &domain_cache, cache.cmn_ttl_ms).await?;

    let capsule = primary_capsule(&entry)?;
    let ep = &capsule.endpoints;

    sink.emit(crate::HyphaEvent::Progress {
        current: 2,
        total: 6,
        message: "Fetching spore manifest".to_string(),
    });
    let (manifest, spore) =
        fetch_verified_spore(sink, capsule, hash, &domain_cache, cache.cmn_ttl_ms).await?;

    sink.emit(crate::HyphaEvent::Progress {
        current: 3,
        total: 6,
        message: "Verifying spore".to_string(),
    });

    let dist = spore.distributions();
    if dist.is_empty() {
        return Err(crate::HyphaError::new(
            "manifest_failed",
            "No distribution options in spore manifest",
        ));
    }

    let staging = tempfile::Builder::new()
        .prefix(&format!(".{}.tmp.", hash))
        .tempdir_in(&spore_parent)
        .map_err(|e| {
            crate::HyphaError::new(
                "dir_error",
                format!("Failed to create temp cache dir: {}", e),
            )
        })?;
    let staging_path = staging.path().to_path_buf();

    let manifest_path = staging_path.join("spore.json");
    let manifest_pretty = serde_json::to_string_pretty(&spore).map_err(|e| {
        crate::HyphaError::new(
            "serialize_error",
            format!("Failed to format manifest: {}", e),
        )
    })?;
    std::fs::write(&manifest_path, manifest_pretty).map_err(|e| {
        crate::HyphaError::new("write_error", format!("Failed to save manifest: {}", e))
    })?;

    sink.emit(crate::HyphaEvent::Progress {
        current: 5,
        total: 6,
        message: "Downloading content".to_string(),
    });
    let archive_endpoints = ep
        .iter()
        .filter(|endpoint| endpoint.kind == "archive")
        .collect::<Vec<_>>();
    let mut downloaded = false;
    let mut archive_to_cache: Option<tempfile::NamedTempFile> = None;
    for dist_entry in dist {
        if dist_has_type(dist_entry, "archive") {
            for archive_ep in &archive_endpoints {
                let archive_url = build_archive_url_from_endpoint(archive_ep, hash)?;
                let _ = std::fs::remove_dir_all(staging_path.join("content"));
                match download_and_extract_tarball_cached_with_progress(
                    &archive_url,
                    &staging_path,
                    cache,
                    &uri.domain,
                    hash,
                    archive_ep.format.as_deref(),
                    sink,
                )
                .await
                {
                    Ok(archive_file) => {
                        archive_to_cache = Some(archive_file);
                        downloaded = true;
                        break;
                    }
                    Err(e) if e.is_policy_rejected() => {
                        warn_remove_dir(sink, &staging_path);
                        return Err(crate::HyphaError::new(
                            "spore_security_rejected",
                            e.to_string(),
                        ));
                    }
                    Err(e) if e.is_malicious() => {
                        let safe_url = agent_first_data::redact_url_secrets(&archive_url);
                        sink.emit(crate::HyphaEvent::Warn {
                            message: format!(
                                "Unverified content from {} was rejected: {}",
                                safe_url, e
                            ),
                        });
                    }
                    Err(e) => {
                        let safe_url = agent_first_data::redact_url_secrets(&archive_url);
                        sink.emit(crate::HyphaEvent::Warn {
                            message: format!("Failed to download from {}: {}", safe_url, e),
                        });
                    }
                }
            }
            if downloaded {
                break;
            }
        } else if let Some(git_url) = dist_git_url(dist_entry) {
            let git_ref = dist_git_ref(dist_entry);
            let _ = std::fs::remove_dir_all(staging_path.join("content"));
            match clone_git_repo(git_url, git_ref, &staging_path, cache).await {
                Ok(_) => {
                    downloaded = true;
                    break;
                }
                Err(e) => {
                    let safe_url = agent_first_data::redact_url_secrets(git_url);
                    sink.emit(crate::HyphaEvent::Warn {
                        message: format!("Failed to clone from {}: {}", safe_url, e),
                    });
                }
            }
        }
    }

    if !downloaded {
        warn_remove_dir(sink, &staging_path);
        return Err(crate::HyphaError::new(
            "fetch_failed",
            "Failed to download from any distribution source",
        ));
    }

    sink.emit(crate::HyphaEvent::Progress {
        current: 6,
        total: 6,
        message: "Verifying content hash".to_string(),
    });
    let content_path = staging_path.join("content");
    verify_downloaded_content(
        sink,
        &staging_path,
        &content_path,
        &manifest,
        hash,
        &domain_cache,
    )?;

    let staging_path = staging.keep();
    if target_path.exists() {
        std::fs::remove_dir_all(&target_path).map_err(|e| {
            let _ = std::fs::remove_dir_all(&staging_path);
            crate::HyphaError::new(
                "cache_write_failed",
                format!("Failed to replace old cache dir: {}", e),
            )
        })?;
    }
    std::fs::rename(&staging_path, &target_path).map_err(|e| {
        let _ = std::fs::remove_dir_all(&staging_path);
        crate::HyphaError::new(
            "cache_write_failed",
            format!("Failed to publish verified cache dir: {}", e),
        )
    })?;

    if let Some(archive_file) = archive_to_cache {
        cache_archive_compressed_file(cache, &uri.domain, hash, archive_file.path());
    }

    Ok(())
}

/// Clone a git repository to the cache path (shallow clone for fetch)
async fn clone_git_repo(
    url: &str,
    git_ref: Option<&str>,
    dest: &std::path::Path,
    cache: &CacheDir,
) -> Result<(), crate::git::GitError> {
    let content_dir = dest.join("content");
    std::fs::create_dir_all(&content_dir)?;

    let url = url.to_string();
    let git_ref = git_ref.map(|s| s.to_string());
    let limits = crate::git::GitSizeLimits::new(
        cache.spore_max_extract_bytes,
        cache.spore_max_extract_files,
    );
    tokio::task::spawn_blocking(move || {
        crate::git::clone_repo(&url, &content_dir, true)?;
        if let Some(r) = git_ref.as_deref() {
            crate::git::checkout_ref(&content_dir, r)?;
        }
        crate::git::enforce_size_budget(&content_dir, limits)?;
        Ok::<(), crate::git::GitError>(())
    })
    .await
    .map_err(|e| crate::git::GitError::Command(format!("Git clone task failed: {}", e)))??;

    Ok(())
}
