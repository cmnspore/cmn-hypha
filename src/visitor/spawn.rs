use super::*;

/// Handle the `spore spawn` command - create a working copy of a spore
///
/// Spawn flow for archive sources (default):
/// 1. Download and extract archive to user directory
/// 2. If --vcs git: initialize git repo with initial commit
/// 3. Update spore.core.json: add spawn reference, clear domain
///
/// Spawn flow for git sources (--dist git):
/// 1. Clone remote repo to cache bare repo (if not exists)
/// 2. Clone from cache bare repo to user directory
/// 3. If --vcs not specified with git dist: keep .git from clone
/// 4. Update spore.core.json: add spawn reference, clear domain
///
/// Spawn a spore to a local directory — library level.
pub async fn spawn(
    uri_str: &str,
    path: Option<&str>,
    vcs: Option<&str>,
    dist_preference: Option<&str>,
    bond: bool,
    sink: &dyn crate::EventSink,
) -> Result<crate::output::SpawnOutput, crate::HyphaError> {
    let uri = CmnUri::parse(uri_str).map_err(|e| crate::HyphaError::new("invalid_uri", e))?;

    let hash = uri
        .hash
        .as_deref()
        .ok_or_else(|| crate::HyphaError::new("invalid_uri", "spore URI must include a hash"))?;

    let cache = CacheDir::new();
    check_taste(sink, &cache, uri_str, &uri.domain, hash)?;

    if let Some(p) = path {
        let target = std::path::PathBuf::from(p);
        if target.exists() {
            return Err(crate::HyphaError::new(
                "DIR_EXISTS",
                format!("Target path already exists: {}", target.display()),
            ));
        }
    }
    let domain_cache = cache.domain(&uri.domain);

    let entry = get_cmn_entry(sink, &domain_cache, cache.cmn_ttl_ms).await?;

    let capsule = primary_capsule(&entry)?;
    let public_key = capsule.key.clone();
    let ep = &capsule.endpoints;

    let manifest = fetch_spore_manifest(capsule, hash)
        .await
        .map_err(|e| crate::HyphaError::new("manifest_failed", e))?;
    let spore = decode_spore_manifest(&manifest)?;

    // For replicated spores, core.key (author) may differ from the host key.
    // Use the embedded author key when present; fall back to host key for self-hosted spores.
    let author_key = embedded_spore_author_key(&manifest).unwrap_or_else(|| public_key.clone());
    verify_manifest_two_key_signatures(&manifest, &public_key, &author_key)
        .map_err(|e| crate::HyphaError::new("sig_failed", e))?;

    let id_opt = (!spore.capsule.core.id.is_empty()).then_some(spore.capsule.core.id.as_str());
    let name = spore.capsule.core.name.as_str();
    let raw_id = id_opt.filter(|id| !id.is_empty());
    let default_dir_name = substrate::local_dir_name(raw_id, Some(name), hash);

    let dist_array = spore.distributions();
    if dist_array.is_empty() {
        return Err(crate::HyphaError::new(
            "manifest_failed",
            "No distribution options in spore manifest",
        ));
    }

    let archive_dist = dist_array.iter().find(|d| dist_has_type(d, "archive"));
    let git_dist = dist_array.iter().find(|d| dist_has_type(d, "git"));

    let target_path = match path {
        Some(p) => std::path::PathBuf::from(p),
        None => {
            let cwd = std::env::current_dir().map_err(|e| {
                crate::HyphaError::new(
                    "dir_error",
                    format!("Failed to get current directory: {}", e),
                )
            })?;
            let auto_dir_name = if raw_id.is_some()
                && default_dir_name != hash
                && cwd.join(&default_dir_name).exists()
            {
                hash.to_string()
            } else {
                default_dir_name.clone()
            };
            cwd.join(auto_dir_name)
        }
    };

    if target_path.exists() {
        return Err(crate::HyphaError::new(
            "DIR_EXISTS",
            format!(
                "Target path already exists: {}. Remove it first.",
                target_path.display()
            ),
        ));
    }

    let prefer_git = matches!(dist_preference, Some("git"));

    let output = if prefer_git {
        if let Some(git_d) = git_dist {
            if !crate::git::is_available() {
                return Err(crate::HyphaError::new(
                    "git_not_found",
                    "git is not installed. Install git or use --dist archive",
                ));
            }
            spawn_from_git_lib(
                sink,
                uri_str,
                hash,
                name,
                git_d,
                &target_path,
                &domain_cache,
                vcs,
                &manifest,
            )
            .await?
        } else if let Some(archive_d) = archive_dist {
            spawn_from_archive_lib(
                sink,
                uri_str,
                hash,
                name,
                archive_d,
                &target_path,
                ep,
                vcs,
                &manifest,
            )
            .await?
        } else {
            return Err(crate::HyphaError::new(
                "manifest_failed",
                "No distribution found in spore manifest",
            ));
        }
    } else if let Some(archive_d) = archive_dist {
        spawn_from_archive_lib(
            sink,
            uri_str,
            hash,
            name,
            archive_d,
            &target_path,
            ep,
            vcs,
            &manifest,
        )
        .await?
    } else if let Some(git_d) = git_dist {
        if !crate::git::is_available() {
            return Err(crate::HyphaError::new(
                "git_not_found",
                "No archive distribution and git is not installed",
            ));
        }
        spawn_from_git_lib(
            sink,
            uri_str,
            hash,
            name,
            git_d,
            &target_path,
            &domain_cache,
            vcs,
            &manifest,
        )
        .await?
    } else {
        return Err(crate::HyphaError::new(
            "manifest_failed",
            "No distribution found in spore manifest",
        ));
    };

    // Auto-bond after successful spawn
    if bond {
        if let Err(e) = bond_in_dir(&target_path, false, false, sink).await {
            sink.emit(crate::HyphaEvent::Warn {
                message: format!("Bond failed after spawn: {}", e),
            });
        }
    }

    Ok(output)
}

pub async fn handle_spawn(
    out: &Output,
    uri_str: &str,
    path: Option<&str>,
    vcs: Option<&str>,
    dist_preference: Option<&str>,
    bond: bool,
) -> ExitCode {
    let sink = crate::api::OutSink(out);
    match spawn(uri_str, path, vcs, dist_preference, bond, &sink).await {
        Ok(output) => out.ok(serde_json::to_value(output).unwrap_or_default()),
        Err(e) => out.error_hypha(&e),
    }
}

#[allow(clippy::too_many_arguments)]
async fn spawn_from_git_lib(
    sink: &dyn crate::EventSink,
    uri_str: &str,
    hash: &str,
    name: &str,
    git_dist: &substrate::SporeDist,
    target_path: &std::path::Path,
    domain_cache: &DomainCache,
    vcs: Option<&str>,
    manifest: &serde_json::Value,
) -> Result<crate::output::SpawnOutput, crate::HyphaError> {
    let git_url = dist_git_url(git_dist).unwrap_or("");
    let git_ref = dist_git_ref(git_dist);

    if git_url.is_empty() {
        return Err(crate::HyphaError::new(
            "spawn_error",
            "Empty git URL in distribution",
        ));
    }

    let temp_bare_name = format!(".spawn-bare-tmp-{}", std::process::id());
    let temp_bare_path = domain_cache.repos_dir().join(&temp_bare_name);

    if temp_bare_path.exists() {
        let _ = std::fs::remove_dir_all(&temp_bare_path);
    }

    std::fs::create_dir_all(domain_cache.repos_dir()).map_err(|e| {
        crate::HyphaError::new(
            "spawn_error",
            format!("Failed to create repos cache directory: {}", e),
        )
    })?;

    crate::git::clone_bare_repo(git_url, &temp_bare_path).map_err(|e| {
        warn_remove_dir(sink, &temp_bare_path);
        crate::HyphaError::new("spawn_error", format!("Failed to clone bare repo: {}", e))
    })?;

    let root_commit = crate::git::get_root_commit_bare(&temp_bare_path).map_err(|e| {
        warn_remove_dir(sink, &temp_bare_path);
        crate::HyphaError::new("spawn_error", format!("Failed to get root commit: {}", e))
    })?;

    let bare_repo_path = domain_cache.repo_path(&root_commit);
    if !bare_repo_path.exists() {
        std::fs::rename(&temp_bare_path, &bare_repo_path).map_err(|e| {
            warn_remove_dir(sink, &temp_bare_path);
            crate::HyphaError::new(
                "spawn_error",
                format!("Failed to move bare repo to cache: {}", e),
            )
        })?;
    } else {
        let _ = std::fs::remove_dir_all(&temp_bare_path);
    }

    crate::git::clone_from_local(&bare_repo_path, target_path).map_err(|e| {
        crate::HyphaError::new("spawn_error", format!("Failed to clone from cache: {}", e))
    })?;

    if let Some(r) = git_ref {
        crate::git::checkout_ref(target_path, r).map_err(|e| {
            warn_remove_dir(sink, target_path);
            crate::HyphaError::new(
                "spawn_error",
                format!("Failed to checkout ref {}: {}", r, e),
            )
        })?;
    }

    verify_content_hash(target_path, hash, manifest).map_err(|e| {
        warn_remove_dir(sink, target_path);
        crate::HyphaError::new("hash_mismatch", format!("Content hash mismatch: {}", e))
    })?;

    let use_vcs = vcs == Some("git");
    if use_vcs {
        let _ = crate::git::set_remote_url(target_path, "origin", uri_str);
        let _ = crate::git::add_remote(
            target_path,
            "spawn",
            &format!("file://{}", bare_repo_path.display()),
        );
    } else {
        let git_dir = target_path.join(".git");
        if git_dir.exists() {
            let _ = std::fs::remove_dir_all(&git_dir);
        }
    }

    let spore_core_path = target_path.join("spore.core.json");
    if spore_core_path.exists() {
        if let Err(e) = save_spawned_from_manifest(&spore_core_path, manifest) {
            sink.emit(crate::HyphaEvent::Warn {
                message: format!("Failed to save spawned-from: {}", e),
            });
        }
    }

    let abs_path = target_path
        .canonicalize()
        .unwrap_or_else(|_| target_path.to_path_buf());

    Ok(crate::output::SpawnOutput {
        uri: uri_str.to_string(),
        name: name.to_string(),
        path: abs_path.display().to_string(),
        source_type: "git".to_string(),
        vcs: vcs.map(|v| v.to_string()),
    })
}

#[allow(clippy::too_many_arguments)]
async fn spawn_from_archive_lib(
    sink: &dyn crate::EventSink,
    uri_str: &str,
    hash: &str,
    name: &str,
    _archive_dist: &substrate::SporeDist,
    target_path: &std::path::Path,
    endpoints: &[substrate::CmnEndpoint],
    vcs: Option<&str>,
    manifest: &serde_json::Value,
) -> Result<crate::output::SpawnOutput, crate::HyphaError> {
    let temp_dir = tempfile::tempdir().map_err(|e| {
        crate::HyphaError::new("spawn_error", format!("Failed to create temp dir: {}", e))
    })?;

    let archive_path = temp_dir.path().join("archive");
    let cache = CacheDir::new();
    let mut downloaded = false;
    let mut last_error = String::new();
    let mut selected_format: Option<String> = None;
    let mut selected_url = String::new();
    for archive_ep in endpoints
        .iter()
        .filter(|endpoint| endpoint.kind == "archive")
    {
        let resolved_url = build_archive_url_from_endpoint(archive_ep, hash)
            .map_err(|e| crate::HyphaError::new("url_error", e))?;
        match download_file(&resolved_url, &archive_path, cache.max_download_bytes).await {
            Ok(_) => {
                downloaded = true;
                selected_format = archive_ep.format.clone();
                selected_url = resolved_url;
                break;
            }
            Err(e) => {
                last_error = format!("{}: {}", resolved_url, e);
            }
        }
    }
    if !downloaded {
        return Err(crate::HyphaError::new(
            "fetch_failed",
            format!("Failed to download archive: {}", last_error),
        ));
    }

    std::fs::create_dir_all(target_path).map_err(|e| {
        crate::HyphaError::new(
            "spawn_error",
            format!("Failed to create target directory: {}", e),
        )
    })?;

    let limits = ExtractLimits::from_cache(&cache);
    extract_archive(
        &archive_path,
        target_path,
        &selected_url,
        selected_format.as_deref(),
        &limits,
    )
    .map_err(|e| {
        warn_remove_dir(sink, target_path);
        if e.is_malicious() {
            let msg = e.to_string();
            let source_domain = CmnUri::parse(uri_str)
                .ok()
                .map(|u| u.domain)
                .unwrap_or_else(|| "unknown".to_string());
            let domain_cache = cache.domain(&source_domain);
            mark_toxic(&domain_cache, hash, &msg);
            crate::HyphaError::new("TOXIC", msg)
        } else {
            crate::HyphaError::new("spawn_error", format!("Failed to extract archive: {}", e))
        }
    })?;

    verify_content_hash(target_path, hash, manifest).map_err(|e| {
        warn_remove_dir(sink, target_path);
        crate::HyphaError::new("hash_mismatch", format!("Content hash mismatch: {}", e))
    })?;

    if vcs == Some("git") {
        crate::git::init_repo(target_path).map_err(|e| {
            warn_remove_dir(sink, target_path);
            crate::HyphaError::new(
                "spawn_error",
                format!("Failed to initialize git repo: {}", e),
            )
        })?;

        let commit_message = format!("Spawned from {}", uri_str);
        crate::git::add_all_and_commit(target_path, &commit_message).map_err(|e| {
            warn_remove_dir(sink, target_path);
            crate::HyphaError::new(
                "spawn_error",
                format!("Failed to create initial commit: {}", e),
            )
        })?;
        let _ = crate::git::add_remote(target_path, "origin", uri_str);
    }

    let spore_core_path = target_path.join("spore.core.json");
    if spore_core_path.exists() {
        if let Err(e) = save_spawned_from_manifest(&spore_core_path, manifest) {
            sink.emit(crate::HyphaEvent::Warn {
                message: format!("Failed to save spawned-from: {}", e),
            });
        }
    }

    let abs_path = target_path
        .canonicalize()
        .unwrap_or_else(|_| target_path.to_path_buf());

    Ok(crate::output::SpawnOutput {
        uri: uri_str.to_string(),
        name: name.to_string(),
        path: abs_path.display().to_string(),
        source_type: "archive".to_string(),
        vcs: vcs.map(|v| v.to_string()),
    })
}

/// Write extracted archive entries to disk.
fn write_entries_to_disk(
    entries: &[substrate::archive::ArchiveEntry],
    dest: &std::path::Path,
) -> Result<(), ExtractError> {
    for entry in entries {
        let target = dest.join(&entry.path);
        match entry.kind {
            substrate::archive::EntryKind::Directory => {
                std::fs::create_dir_all(&target).map_err(|e| {
                    ExtractError::Failed(format!("Failed to create dir {}: {}", entry.path, e))
                })?;
            }
            substrate::archive::EntryKind::File => {
                if let Some(parent) = target.parent() {
                    std::fs::create_dir_all(parent).map_err(|e| {
                        ExtractError::Failed(format!("Failed to create parent dir: {}", e))
                    })?;
                }
                std::fs::write(&target, &entry.data).map_err(|e| {
                    ExtractError::Failed(format!("Failed to write file {}: {}", entry.path, e))
                })?;
            }
        }
    }
    Ok(())
}

/// Extract an archive to destination directory.
/// Requires endpoint-declared archive format; no legacy suffix fallback.
pub(super) fn extract_archive(
    archive_path: &std::path::Path,
    dest: &std::path::Path,
    url: &str,
    format_hint: Option<&str>,
    limits: &ExtractLimits,
) -> Result<(), ExtractError> {
    std::fs::create_dir_all(dest).map_err(|e| {
        ExtractError::Failed(format!("Failed to create destination directory: {}", e))
    })?;

    if format_hint == Some("tar+zstd") {
        let compressed = std::fs::read(archive_path)
            .map_err(|e| ExtractError::Failed(format!("Failed to read archive: {}", e)))?;
        let sub_limits = substrate::archive::ExtractLimits {
            max_bytes: limits.max_bytes,
            max_files: limits.max_files,
            max_file_bytes: limits.max_file_bytes,
        };
        let entries = substrate::archive::extract_tar_zstd(&compressed, &sub_limits)?;
        write_entries_to_disk(&entries, dest)
    } else {
        Err(ExtractError::Failed(format!(
            "Unsupported archive format for {}: {:?}. Expected format tar+zstd",
            url, format_hint
        )))
    }
}

/// Save source spore manifest to `.cmn/spawned-from/spore.json` after spawn.
/// spore.core.json is left untouched — hatch handles domain/key changes,
/// release checks spawned_from at publish time.
pub(super) fn save_spawned_from_manifest(
    spore_core_path: &Path,
    manifest: &serde_json::Value,
) -> Result<(), String> {
    let cmn_dir = spore_core_path.parent().unwrap_or(spore_core_path);
    let spawned_from_dir = cmn_dir.join(".cmn").join("spawned-from");
    std::fs::create_dir_all(&spawned_from_dir)
        .map_err(|e| format!("Failed to create .cmn/spawned-from: {}", e))?;

    let spore = substrate::decode_spore(manifest)
        .map_err(|e| format!("Invalid source spore manifest: {}", e))?;
    let pretty = spore
        .to_pretty_json()
        .map_err(|e| format!("Failed to format source spore manifest: {}", e))?;
    std::fs::write(spawned_from_dir.join("spore.json"), pretty)
        .map_err(|e| format!("Failed to write .cmn/spawned-from/spore.json: {}", e))?;

    Ok(())
}

/// Download and extract tarball with byte-level progress events. but emits `DownloadProgress` events
/// for byte-level progress tracking (speed / ETA).
pub(super) async fn download_and_extract_tarball_cached_with_progress(
    url: &str,
    dest: &std::path::Path,
    cache: &CacheDir,
    domain: &str,
    hash: &str,
    format_hint: Option<&str>,
    sink: &dyn crate::EventSink,
) -> Result<(), ExtractError> {
    use std::io::Write;

    let client = substrate::client::http_client(300)
        .map_err(|e| format!("Failed to create HTTP client: {}", e))?;

    let response = client
        .get(url)
        .send()
        .await
        .map_err(|e| format!("Failed to download: {}", e))?;

    if !response.status().is_success() {
        return Err(ExtractError::Failed(format!("HTTP {}", response.status())));
    }

    let total_bytes = response.content_length();

    // Reject responses that declare size beyond limit
    let max_download = cache.max_download_bytes;
    if let Some(cl) = total_bytes {
        if cl > max_download {
            return Err(ExtractError::Malicious(format!(
                "Response too large: {} bytes exceeds max_download_bytes ({})",
                cl, max_download
            )));
        }
    }

    // Emit initial progress
    sink.emit(crate::HyphaEvent::DownloadProgress {
        downloaded_bytes: 0,
        total_bytes,
    });

    let bytes = response
        .bytes()
        .await
        .map_err(|e| format!("Download read error: {}", e))?;
    let downloaded = bytes.len() as u64;
    if downloaded > max_download {
        return Err(ExtractError::Malicious(format!(
            "Download exceeds max_download_bytes ({})",
            max_download
        )));
    }

    let tmp_dir =
        tempfile::tempdir().map_err(|e| format!("Failed to create temp directory: {}", e))?;
    let archive_path = tmp_dir.path().join("archive");
    let mut out = std::fs::File::create(&archive_path)
        .map_err(|e| format!("Failed to create temp archive file: {}", e))?;
    out.write_all(&bytes)
        .map_err(|e| format!("Failed to write temp archive: {}", e))?;

    // Final progress
    sink.emit(crate::HyphaEvent::DownloadProgress {
        downloaded_bytes: downloaded,
        total_bytes,
    });
    out.sync_all()
        .map_err(|e| format!("Failed to sync temp archive: {}", e))?;

    let content_dir = dest.join("content");
    std::fs::create_dir_all(&content_dir)
        .map_err(|e| format!("Failed to create content directory: {}", e))?;

    // Detect format from URL and extract
    let limits = ExtractLimits::from_cache(cache);
    extract_archive(&archive_path, &content_dir, url, format_hint, &limits)?;
    if format_hint == Some("tar+zstd") || url.ends_with(".tar.zst") || url.ends_with(".zst") {
        cache_archive_compressed_file(cache, domain, hash, &archive_path);
    }

    Ok(())
}

/// Cache already-compressed archive file for future delta downloads.
pub(super) fn cache_archive_compressed_file(
    cache: &CacheDir,
    domain: &str,
    hash: &str,
    archive_path: &std::path::Path,
) {
    let cache_dir = cache.domain(domain).spore_path(hash);
    if std::fs::create_dir_all(&cache_dir).is_err() {
        return;
    }
    let cache_path = cache_dir.join("archive.tar.zst");
    let _ = std::fs::copy(archive_path, &cache_path);
}

/// Cache decoded raw tar file as compressed archive for future delta downloads.
pub(super) fn cache_archive_raw_file(
    cache: &CacheDir,
    domain: &str,
    hash: &str,
    raw_tar_path: &std::path::Path,
    _max_extract_bytes: u64,
) {
    let cache_dir = cache.domain(domain).spore_path(hash);
    if std::fs::create_dir_all(&cache_dir).is_err() {
        return;
    }
    let cache_path = cache_dir.join("archive.tar.zst");

    let raw_data = match std::fs::read(raw_tar_path) {
        Ok(d) => d,
        Err(_) => return,
    };
    let compressed = match substrate::archive::encode_zstd(&raw_data, 3) {
        Ok(c) => c,
        Err(_) => return,
    };
    if std::fs::write(&cache_path, &compressed).is_err() {
        let _ = std::fs::remove_file(&cache_path);
    }
}

/// Download a delta archive, apply it, and extract directly to dest (no content subdir).
/// Used by grow/pull_from_archive. Returns decoded raw tar file for cache reuse.
pub(super) async fn download_and_apply_delta(
    delta_url: &str,
    old_archive_path: &std::path::Path,
    dest: &std::path::Path,
    limits: &ExtractLimits,
    max_download_bytes: u64,
) -> Result<tempfile::NamedTempFile, ExtractError> {
    let budget = DeltaByteBudget::new(max_download_bytes, limits);
    let delta_file = tempfile::NamedTempFile::new()
        .map_err(|e| format!("Failed to create temp delta file: {}", e))?;
    download_file(delta_url, delta_file.path(), budget.max_download_bytes)
        .await
        .map_err(|e| format!("Failed to download delta: {}", e))?;

    let old_raw_tar = load_old_archive_dictionary(old_archive_path, &budget)?;
    let raw_tar_file = tempfile::NamedTempFile::new()
        .map_err(|e| format!("Failed to create temp decoded delta file: {}", e))?;
    decode_delta_to_raw_tar_file(
        delta_file.path(),
        &old_raw_tar,
        raw_tar_file.path(),
        &budget,
    )?;

    std::fs::create_dir_all(dest).map_err(|e| format!("Failed to create directory: {}", e))?;
    let raw_tar_bytes = std::fs::read(raw_tar_file.path())
        .map_err(|e| format!("Failed to read decoded delta archive: {}", e))?;
    let sub_limits = substrate::archive::ExtractLimits {
        max_bytes: limits.max_bytes,
        max_files: limits.max_files,
        max_file_bytes: limits.max_file_bytes,
    };
    let entries = substrate::archive::extract_tar(&raw_tar_bytes, &sub_limits)?;
    write_entries_to_disk(&entries, dest)?;

    Ok(raw_tar_file)
}
