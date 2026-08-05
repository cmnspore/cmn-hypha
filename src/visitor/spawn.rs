use super::*;

enum ArchiveEndpointAttemptError {
    Retry(String),
    Fatal(crate::HyphaError),
}

async fn try_archive_endpoints<F, Fut>(
    sink: &dyn crate::EventSink,
    hash: &str,
    endpoints: &[substrate::CmnEndpoint],
    mut attempt: F,
) -> Result<(), crate::HyphaError>
where
    F: FnMut(substrate::CmnEndpoint, String) -> Fut,
    Fut: std::future::Future<Output = Result<(), ArchiveEndpointAttemptError>>,
{
    let mut last_error = "no archive endpoints configured".to_string();

    for archive_ep in endpoints
        .iter()
        .filter(|endpoint| endpoint.kind == "archive")
    {
        let resolved_url = build_archive_url_from_endpoint(archive_ep, hash)?;
        match attempt(archive_ep.clone(), resolved_url.clone()).await {
            Ok(()) => return Ok(()),
            Err(ArchiveEndpointAttemptError::Retry(message)) => {
                last_error = message.clone();
                sink.emit(crate::HyphaEvent::Warn { message });
            }
            Err(ArchiveEndpointAttemptError::Fatal(err)) => return Err(err),
        }
    }

    Err(crate::HyphaError::new(
        "fetch_failed",
        format!("Failed to download archive: {}", last_error),
    ))
}

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

    let cache = CacheDir::new()?;
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
    let ep = &capsule.endpoints;

    let (manifest, spore) =
        fetch_verified_spore(sink, capsule, hash, &domain_cache, cache.cmn_ttl_ms).await?;

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
                &cache,
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
                &domain_cache,
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
            &domain_cache,
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
            &cache,
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
        Ok(output) => out.ok(output),
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
    cache: &CacheDir,
    vcs: Option<&str>,
    manifest: &serde_json::Value,
) -> Result<crate::output::SpawnOutput, crate::HyphaError> {
    let git_url = dist_git_url(git_dist).unwrap_or("");
    let git_ref = dist_git_ref(git_dist);
    let git_limits = crate::git::GitSizeLimits::new(
        cache.spore_max_extract_bytes,
        cache.spore_max_extract_files,
    );

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
    crate::git::enforce_size_budget(&temp_bare_path, git_limits).map_err(|e| {
        warn_remove_dir(sink, &temp_bare_path);
        crate::HyphaError::new(
            "spawn_error",
            format!("Git repo exceeds size budget: {}", e),
        )
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

    crate::git::enforce_size_budget(&bare_repo_path, git_limits).map_err(|e| {
        crate::HyphaError::new(
            "spawn_error",
            format!("Git repo exceeds size budget: {}", e),
        )
    })?;

    crate::git::clone_from_local_no_checkout(&bare_repo_path, target_path).map_err(|e| {
        crate::HyphaError::new("spawn_error", format!("Failed to clone from cache: {}", e))
    })?;
    crate::git::configure_blobless_promisor_remote(
        target_path,
        crate::git::CMN_PROMISOR_REMOTE,
        git_url,
    )
    .map_err(|e| {
        warn_remove_dir(sink, target_path);
        crate::HyphaError::new(
            "spawn_error",
            format!("Failed to configure git promisor remote: {}", e),
        )
    })?;

    let checkout_ref = git_ref.unwrap_or("HEAD");
    crate::git::checkout_ref(target_path, checkout_ref).map_err(|e| {
        warn_remove_dir(sink, target_path);
        crate::HyphaError::new(
            "spawn_error",
            format!("Failed to checkout ref {}: {}", checkout_ref, e),
        )
    })?;
    crate::git::enforce_size_budget(target_path, git_limits).map_err(|e| {
        warn_remove_dir(sink, target_path);
        crate::HyphaError::new(
            "spawn_error",
            format!("Git checkout exceeds size budget: {}", e),
        )
    })?;

    verify_downloaded_content(sink, target_path, target_path, manifest, hash, domain_cache)?;

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
        cmn_url: uri_str.to_string(),
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
    domain_cache: &DomainCache,
    endpoints: &[substrate::CmnEndpoint],
    vcs: Option<&str>,
    manifest: &serde_json::Value,
) -> Result<crate::output::SpawnOutput, crate::HyphaError> {
    let temp_dir = tempfile::tempdir().map_err(|e| {
        crate::HyphaError::new("spawn_error", format!("Failed to create temp dir: {}", e))
    })?;

    let archive_path = temp_dir.path().join("archive");
    let cache = CacheDir::new()?;
    let limits = ExtractLimits::from_cache(&cache);

    try_archive_endpoints(sink, hash, endpoints, |archive_ep, resolved_url| {
        let archive_path = archive_path.clone();
        let limits = &limits;
        let max_download_bytes = cache.spore_max_download_bytes;

        async move {
            if let Err(e) = download_file(&resolved_url, &archive_path, max_download_bytes).await {
                return Err(ArchiveEndpointAttemptError::Retry(format!(
                    "Failed to download from {}: {}",
                    resolved_url, e
                )));
            }

            if target_path.exists() {
                std::fs::remove_dir_all(target_path).map_err(|e| {
                    ArchiveEndpointAttemptError::Fatal(crate::HyphaError::new(
                        "spawn_error",
                        format!(
                            "Failed to clear target directory {} before retry: {}",
                            target_path.display(),
                            e
                        ),
                    ))
                })?;
            }
            std::fs::create_dir_all(target_path).map_err(|e| {
                ArchiveEndpointAttemptError::Fatal(crate::HyphaError::new(
                    "spawn_error",
                    format!("Failed to create target directory: {}", e),
                ))
            })?;

            extract_archive(
                &archive_path,
                target_path,
                &resolved_url,
                archive_ep.format.as_deref(),
                limits,
            )
            .map_err(|e| {
                warn_remove_dir(sink, target_path);
                if e.is_policy_rejected() {
                    ArchiveEndpointAttemptError::Fatal(crate::HyphaError::new(
                        "spore_security_rejected",
                        e.to_string(),
                    ))
                } else if e.is_malicious() {
                    ArchiveEndpointAttemptError::Retry(format!(
                        "Unverified content from {} was rejected: {}",
                        resolved_url, e
                    ))
                } else {
                    ArchiveEndpointAttemptError::Retry(format!(
                        "Failed to extract archive from {}: {}",
                        resolved_url, e
                    ))
                }
            })?;

            verify_downloaded_content(sink, target_path, target_path, manifest, hash, domain_cache)
                .map_err(|e| {
                    ArchiveEndpointAttemptError::Retry(format!(
                        "Failed to verify content from {}: {}",
                        resolved_url, e
                    ))
                })?;

            Ok(())
        }
    })
    .await?;

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
        cmn_url: uri_str.to_string(),
        name: name.to_string(),
        path: abs_path.display().to_string(),
        source_type: "archive".to_string(),
        vcs: vcs.map(|v| v.to_string()),
    })
}

/// Resolve an archive entry path against `dest`, rejecting anything that could
/// escape the destination. This is defense-in-depth at the actual write site —
/// substrate's tar extractor already rejects traversal, but the filesystem
/// boundary should never trust the entry path unconditionally.
fn safe_entry_target(
    dest: &std::path::Path,
    rel: &str,
) -> Result<std::path::PathBuf, ExtractError> {
    use std::path::Component;

    if rel.is_empty() {
        return Err(ExtractError::Failed(
            "archive entry has empty path".to_string(),
        ));
    }
    for component in std::path::Path::new(rel).components() {
        match component {
            Component::Normal(_) | Component::CurDir => {}
            // RootDir, ParentDir (`..`), or a drive Prefix would escape `dest`.
            _ => {
                return Err(ExtractError::Failed(format!(
                    "archive entry escapes destination: {}",
                    rel
                )))
            }
        }
    }
    Ok(dest.join(rel))
}

fn reject_archive_entries(
    entries: &[substrate::archive::ArchiveEntry],
    reject_path_components: &[String],
) -> Result<(), ExtractError> {
    for entry in entries {
        if let Some(component) =
            rejected_path_component(std::path::Path::new(&entry.path), reject_path_components)
        {
            return Err(ExtractError::PolicyRejected(format!(
                "received spore content contains protected path component '{}': {}",
                component, entry.path
            )));
        }
    }
    Ok(())
}

/// Write extracted archive entries to disk.
fn write_entries_to_disk(
    entries: &[substrate::archive::ArchiveEntry],
    dest: &std::path::Path,
    reject_path_components: &[String],
) -> Result<(), ExtractError> {
    reject_archive_entries(entries, reject_path_components)?;

    for entry in entries {
        let target = safe_entry_target(dest, &entry.path)?;
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
                #[cfg(unix)]
                if entry.executable {
                    use std::os::unix::fs::PermissionsExt;
                    std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o755))
                        .map_err(|e| {
                            ExtractError::Failed(format!(
                                "Failed to set executable bit on {}: {}",
                                entry.path, e
                            ))
                        })?;
                }
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
        write_entries_to_disk(&entries, dest, &limits.reject_path_components)
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
) -> Result<(), crate::HyphaError> {
    let cmn_dir = spore_core_path.parent().unwrap_or(spore_core_path);
    let spawned_from_dir = cmn_dir.join(".cmn").join("spawned-from");
    std::fs::create_dir_all(&spawned_from_dir).map_err(|e| {
        crate::HyphaError::new(
            "spawn_error",
            format!("Failed to create .cmn/spawned-from: {}", e),
        )
    })?;

    let spore = substrate::decode_spore(manifest).map_err(|e| {
        crate::HyphaError::new(
            "spawn_error",
            format!("Invalid source spore manifest: {}", e),
        )
    })?;
    let pretty = spore.to_pretty_json().map_err(|e| {
        crate::HyphaError::new(
            "spawn_error",
            format!("Failed to format source spore manifest: {}", e),
        )
    })?;
    std::fs::write(spawned_from_dir.join("spore.json"), pretty).map_err(|e| {
        crate::HyphaError::new(
            "spawn_error",
            format!("Failed to write .cmn/spawned-from/spore.json: {}", e),
        )
    })?;

    Ok(())
}

/// Download and extract tarball with byte-level progress events. but emits `DownloadProgress` events
/// for byte-level progress tracking (speed / ETA).
pub(super) async fn download_and_extract_tarball_cached_with_progress(
    url: &str,
    dest: &std::path::Path,
    cache: &CacheDir,
    _domain: &str,
    _hash: &str,
    format_hint: Option<&str>,
    sink: &dyn crate::EventSink,
) -> Result<tempfile::NamedTempFile, ExtractError> {
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

    let max_download = cache.spore_max_download_bytes;
    if let Some(content_length) = response.content_length() {
        if content_length > max_download {
            return Err(ExtractError::Malicious(format!(
                "Remote payload too large: {} bytes exceeds limit {}",
                content_length, max_download
            )));
        }
    }

    let mut archive_file = tempfile::NamedTempFile::new()
        .map_err(|e| format!("Failed to create temp archive: {}", e))?;
    let archive_path = archive_file.path().to_path_buf();
    let (result, exceeded) = {
        let mut limited = LimitedWriter::new(archive_file.as_file_mut(), max_download);
        let result = substrate::client::download_response_to_writer(
            response,
            url,
            u64::MAX,
            &mut limited,
            |downloaded_bytes, total_bytes| {
                sink.emit(crate::HyphaEvent::DownloadProgress {
                    downloaded_bytes,
                    total_bytes,
                });
            },
        )
        .await;
        (result, limited.exceeded)
    };
    if let Err(e) = result {
        if exceeded {
            return Err(ExtractError::Malicious(format!(
                "Download exceeded size limit of {} bytes",
                max_download
            )));
        }
        return Err(ExtractError::Failed(e.to_string()));
    }
    archive_file
        .as_file()
        .sync_all()
        .map_err(|e| format!("Failed to sync temp archive: {}", e))?;

    let content_dir = dest.join("content");
    std::fs::create_dir_all(&content_dir)
        .map_err(|e| format!("Failed to create content directory: {}", e))?;

    // Detect format from URL and extract
    let limits = ExtractLimits::from_cache(cache);
    extract_archive(&archive_path, &content_dir, url, format_hint, &limits)?;

    Ok(archive_file)
}

/// Cache already-compressed archive file for future delta downloads.
pub(super) fn cache_archive_compressed_file(
    cache: &CacheDir,
    domain: &str,
    hash: &str,
    archive_path: &std::path::Path,
) {
    use std::io::Write;

    let cache_dir = cache.domain(domain).spore_path(hash);
    if !cache_dir.is_dir() {
        return;
    }
    let cache_path = cache_dir.join("archive.tar.zst");
    let mut src = match std::fs::File::open(archive_path) {
        Ok(file) => file,
        Err(_) => return,
    };
    let mut tmp = match tempfile::NamedTempFile::new_in(&cache_dir) {
        Ok(file) => file,
        Err(_) => return,
    };
    if std::io::copy(&mut src, &mut tmp).is_err() {
        return;
    }
    if tmp.as_file_mut().flush().is_err() || tmp.as_file().sync_all().is_err() {
        return;
    }
    let _ = tmp.persist(&cache_path);
}

/// Cache decoded raw tar file as compressed archive for future delta downloads.
pub(super) fn cache_archive_raw_file(
    cache: &CacheDir,
    domain: &str,
    hash: &str,
    raw_tar_path: &std::path::Path,
    _spore_max_extract_bytes: u64,
) {
    use std::io::Write;

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
    let mut tmp = match tempfile::NamedTempFile::new_in(&cache_dir) {
        Ok(file) => file,
        Err(_) => return,
    };
    if tmp.as_file_mut().write_all(&compressed).is_err() {
        return;
    }
    if tmp.as_file_mut().flush().is_err() || tmp.as_file().sync_all().is_err() {
        return;
    }
    let _ = tmp.persist(&cache_path);
}

/// Download a delta archive, apply it, and extract directly to dest (no content subdir).
/// Used by grow/pull_from_archive. Returns decoded raw tar file for cache reuse.
pub(super) async fn download_and_apply_delta(
    delta_url: &str,
    old_archive_path: &std::path::Path,
    dest: &std::path::Path,
    limits: &ExtractLimits,
    spore_max_download_bytes: u64,
) -> Result<tempfile::NamedTempFile, ExtractError> {
    let budget = DeltaByteBudget::new(spore_max_download_bytes, limits);
    let delta_file = tempfile::NamedTempFile::new()
        .map_err(|e| format!("Failed to create temp delta file: {}", e))?;
    download_file(
        delta_url,
        delta_file.path(),
        budget.spore_max_download_bytes,
    )
    .await
    .map_err(|e| match e {
        ExtractError::Malicious(msg) => {
            ExtractError::Malicious(format!("Failed to download delta: {}", msg))
        }
        ExtractError::PolicyRejected(msg) => {
            ExtractError::PolicyRejected(format!("Failed to download delta: {}", msg))
        }
        ExtractError::Failed(msg) => {
            ExtractError::Failed(format!("Failed to download delta: {}", msg))
        }
    })?;

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
    write_entries_to_disk(&entries, dest, &limits.reject_path_components)?;

    Ok(raw_tar_file)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn write_entries_to_disk_preserves_executable_bit() {
        use std::os::unix::fs::PermissionsExt;

        let tmp = tempfile::tempdir().unwrap();
        let entries = vec![
            substrate::archive::ArchiveEntry {
                path: "bin/run.sh".to_string(),
                kind: substrate::archive::EntryKind::File,
                data: b"#!/bin/sh\n".to_vec(),
                executable: true,
            },
            substrate::archive::ArchiveEntry {
                path: "README.md".to_string(),
                kind: substrate::archive::EntryKind::File,
                data: b"hello\n".to_vec(),
                executable: false,
            },
        ];

        write_entries_to_disk(&entries, tmp.path(), &[]).unwrap();

        let run_mode = std::fs::metadata(tmp.path().join("bin/run.sh"))
            .unwrap()
            .permissions()
            .mode();
        let readme_mode = std::fs::metadata(tmp.path().join("README.md"))
            .unwrap()
            .permissions()
            .mode();
        assert_ne!(run_mode & 0o111, 0);
        assert_eq!(readme_mode & 0o111, 0);
    }

    #[cfg(unix)]
    #[test]
    fn extract_archive_tar_zstd_preserves_executable_bit() {
        use std::os::unix::fs::PermissionsExt;

        let tmp = tempfile::tempdir().unwrap();
        let archive_path = tmp.path().join("archive.tar.zst");
        let dest = tmp.path().join("dest");

        let mut tar_bytes = Vec::new();
        {
            let mut tar = tar::Builder::new(&mut tar_bytes);
            let mut header = tar::Header::new_gnu();
            let content = b"#!/bin/sh\necho ok\n";
            header.set_size(content.len() as u64);
            header.set_mode(0o755);
            header.set_cksum();
            tar.append_data(&mut header, "bin/run.sh", &content[..])
                .unwrap();
            tar.finish().unwrap();
        }
        let compressed = substrate::archive::encode_zstd(&tar_bytes, 3).unwrap();
        std::fs::write(&archive_path, compressed).unwrap();

        let limits = ExtractLimits {
            max_bytes: 1 << 20,
            max_files: 10,
            max_file_bytes: 1 << 20,
            reject_path_components: vec![".git".to_string(), ".cmn".to_string()],
        };
        extract_archive(
            &archive_path,
            &dest,
            "https://example.com/archive.tar.zst",
            Some("tar+zstd"),
            &limits,
        )
        .unwrap();

        let mode = std::fs::metadata(dest.join("bin/run.sh"))
            .unwrap()
            .permissions()
            .mode();
        assert_ne!(mode & 0o111, 0);
    }

    #[test]
    fn write_entries_to_disk_rejects_protected_control_components() {
        let tmp = tempfile::tempdir().unwrap();
        let reject = vec![".git".to_string(), ".cmn".to_string()];
        for path in [
            ".git/config",
            "src/.git/config",
            ".cmn/state.json",
            "src/.cmn/state.json",
        ] {
            let entries = vec![substrate::archive::ArchiveEntry {
                path: path.to_string(),
                kind: substrate::archive::EntryKind::File,
                data: b"blocked\n".to_vec(),
                executable: false,
            }];

            let err = write_entries_to_disk(&entries, tmp.path(), &reject).unwrap_err();
            assert!(
                err.is_policy_rejected(),
                "{} should be policy rejected, got {}",
                path,
                err
            );
            assert!(!err.is_malicious(), "{} must not be toxic", path);
        }
    }

    #[test]
    fn write_entries_to_disk_allows_similar_non_control_names() {
        let tmp = tempfile::tempdir().unwrap();
        let reject = vec![".git".to_string(), ".cmn".to_string()];
        let entries = vec![
            substrate::archive::ArchiveEntry {
                path: ".gitignore".to_string(),
                kind: substrate::archive::EntryKind::File,
                data: b"target/\n".to_vec(),
                executable: false,
            },
            substrate::archive::ArchiveEntry {
                path: ".gitattributes".to_string(),
                kind: substrate::archive::EntryKind::File,
                data: b"* text=auto\n".to_vec(),
                executable: false,
            },
            substrate::archive::ArchiveEntry {
                path: "src/legit-cmn-file.txt".to_string(),
                kind: substrate::archive::EntryKind::File,
                data: b"ok\n".to_vec(),
                executable: false,
            },
            substrate::archive::ArchiveEntry {
                path: "docs/git-notes.md".to_string(),
                kind: substrate::archive::EntryKind::File,
                data: b"ok\n".to_vec(),
                executable: false,
            },
        ];

        write_entries_to_disk(&entries, tmp.path(), &reject).unwrap();

        assert!(tmp.path().join(".gitignore").exists());
        assert!(tmp.path().join(".gitattributes").exists());
        assert!(tmp.path().join("src/legit-cmn-file.txt").exists());
        assert!(tmp.path().join("docs/git-notes.md").exists());
    }

    #[tokio::test(flavor = "current_thread")]
    #[allow(clippy::await_holding_lock)]
    async fn archive_endpoint_retry_bad_then_good_does_not_mark_toxic() {
        let _lock = crate::config::ENV_LOCK.lock().unwrap();
        let home = tempfile::tempdir().unwrap();
        std::env::set_var("CMN_HOME", home.path());
        let cache = crate::cache::CacheDir::new().unwrap();
        let domain_cache = cache.domain("example.com");

        let manifest = serde_json::json!({
            "$schema": "https://cmn.dev/schemas/v1/spore.json",
            "capsule": {
                "uri": "cmn://example.com/b3.placeholder",
                "core": {
                    "name": "retry-test",
                    "domain": "example.com",
                    "key": "ed25519.5XmkQ9vZP8nL",
                    "synopsis": "Retry test",
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
                "dist": [{"type": "archive"}]
            },
            "capsule_signature": "ed25519.fakesig"
        });

        let good_content = tempfile::tempdir().unwrap();
        std::fs::write(good_content.path().join("README.md"), "expected").unwrap();
        let spore = substrate::decode_spore(&manifest).unwrap();
        let entries = crate::tree::walk_dir(
            good_content.path(),
            &spore.tree().exclude_names,
            &spore.tree().follow_rules,
        )
        .unwrap();
        let tree_hash = spore.tree().compute_hash(&entries).unwrap();
        let valid_hash = spore.computed_uri_hash_from_tree_hash(&tree_hash).unwrap();

        let endpoints = vec![
            substrate::CmnEndpoint {
                kind: "archive".to_string(),
                url: "https://bad.example.com/archive/{hash}.tar.zst".to_string(),
                hash: String::new(),
                hashes: vec![],
                format: Some("tar+zstd".to_string()),
                delta_url: None,
            },
            substrate::CmnEndpoint {
                kind: "archive".to_string(),
                url: "https://good.example.com/archive/{hash}.tar.zst".to_string(),
                hash: String::new(),
                hashes: vec![],
                format: Some("tar+zstd".to_string()),
                delta_url: None,
            },
        ];
        let target_parent = tempfile::tempdir().unwrap();
        let target = target_parent.path().join("spawned");
        let mut attempts = 0usize;
        let mut seen_urls = Vec::new();

        try_archive_endpoints(&crate::NoopSink, &valid_hash, &endpoints, |_, url| {
            seen_urls.push(url.clone());
            attempts += 1;
            let attempt = attempts;
            let valid_hash = valid_hash.clone();
            let target = target.clone();
            let manifest = &manifest;
            let domain_cache = &domain_cache;

            async move {
                std::fs::create_dir_all(&target).unwrap();
                let content = if attempt == 1 { "tampered" } else { "expected" };
                std::fs::write(target.join("README.md"), content).unwrap();

                verify_downloaded_content(
                    &crate::NoopSink,
                    &target,
                    &target,
                    manifest,
                    &valid_hash,
                    domain_cache,
                )
                .map_err(|e| {
                    ArchiveEndpointAttemptError::Retry(format!(
                        "Failed to verify content from {}: {}",
                        url, e
                    ))
                })?;

                Ok(())
            }
        })
        .await
        .unwrap();

        assert_eq!(attempts, 2);
        assert_eq!(
            seen_urls,
            vec![
                format!("https://bad.example.com/archive/{}.tar.zst", valid_hash),
                format!("https://good.example.com/archive/{}.tar.zst", valid_hash)
            ]
        );
        assert_eq!(
            std::fs::read_to_string(target.join("README.md")).unwrap(),
            "expected"
        );
        assert!(
            domain_cache.load_taste(&valid_hash).is_none(),
            "bad unverified endpoint must not persist a toxic verdict"
        );

        std::env::remove_var("CMN_HOME");
    }
}
