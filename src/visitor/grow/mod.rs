use super::*;

mod resolve;

use resolve::{find_latest_version, update_bonds};

#[cfg(test)]
pub(super) fn spawned_from_hash(manifest: &serde_json::Value) -> Option<String> {
    resolve::spawned_from_hash(manifest)
}

/// Handle the `grow` command — update a spawned spore via Synapse lineage
///
/// 6-step flow with Agent-First Data progress:
/// 1. RESOLVE  — Read ./spore.core.json, extract spawned_from
/// 2. LINEAGE  — Query synapse for newer versions on same domain
/// 3. VERIFY   — Fetch & verify new spore from source site
/// 4. TASTE    — Check taste verdict for new hash
/// 5. APPLY    — Download & apply update
/// 6. COMPLETE — Report result
///
/// Update a spawned spore to a newer version — library level.
#[allow(clippy::too_many_arguments)]
pub async fn grow(
    path: Option<&str>,
    dist_preference: Option<&str>,
    bond: bool,
    synapse_arg: Option<&str>,
    synapse_token_secret: Option<&str>,
    sink: &dyn crate::EventSink,
) -> Result<crate::output::GrowOutput, crate::HyphaError> {
    sink.emit(crate::HyphaEvent::Progress {
        current: 1,
        total: 6,
        message: "Reading spore.core.json".to_string(),
    });

    let target_path = match path {
        Some(p) => std::path::PathBuf::from(p),
        None => std::env::current_dir().map_err(|e| {
            crate::HyphaError::new(
                "dir_error",
                format!("Failed to get current directory: {}", e),
            )
        })?,
    };

    let abs_path = target_path.canonicalize().map_err(|_| {
        crate::HyphaError::new(
            "grow_error",
            format!("Path does not exist: {}", target_path.display()),
        )
    })?;

    let spore_core_path = abs_path.join("spore.core.json");
    if !spore_core_path.exists() {
        return Err(crate::HyphaError::new(
            "grow_error",
            "spore.core.json not found. Not a spore directory.",
        ));
    }

    let spore_core_content = std::fs::read_to_string(&spore_core_path).map_err(|e| {
        crate::HyphaError::new(
            "grow_error",
            format!("Failed to read spore.core.json: {}", e),
        )
    })?;

    let local_core: substrate::SporeCore =
        serde_json::from_str(&spore_core_content).map_err(|e| {
            crate::HyphaError::new(
                "grow_error",
                format!("Failed to parse spore.core.json: {}", e),
            )
        })?;

    // Read spawned_from from .cmn/spawned-from/spore.json
    let spawned_from_spore_path = abs_path
        .join(".cmn")
        .join("spawned-from")
        .join("spore.json");
    let parent_content = std::fs::read_to_string(&spawned_from_spore_path).map_err(|_| {
        crate::HyphaError::with_hint(
            "grow_error",
            "Not a spawned spore — .cmn/spawned-from/spore.json not found",
            "run: hypha spawn <URI>",
        )
    })?;
    let parent: substrate::Spore = serde_json::from_str(&parent_content).map_err(|e| {
        crate::HyphaError::new(
            "grow_error",
            format!("Failed to parse .cmn/spawned-from/spore.json: {}", e),
        )
    })?;
    let spawned_uri = parent.uri().to_string();

    let spawned_parsed = CmnUri::parse(&spawned_uri).map_err(|e| {
        crate::HyphaError::new("grow_error", format!("Invalid spawned_from URI: {}", e))
    })?;

    let current_hash = spawned_parsed.hash.clone().ok_or_else(|| {
        crate::HyphaError::new("grow_error", "spawned_from URI must include a hash")
    })?;

    let source_domain = spawned_parsed.domain.clone();

    // Step 2: LINEAGE — requires Synapse
    sink.emit(crate::HyphaEvent::Progress {
        current: 2,
        total: 6,
        message: "Querying Synapse lineage".to_string(),
    });
    let resolved_synapse = crate::config::resolve_synapse(synapse_arg, synapse_token_secret)
        .map_err(|_| {
            crate::HyphaError::with_hint(
                "synapse_required",
                "grow requires a reachable Synapse node",
                "use 'hypha spawn <uri>' to fetch a specific version directly",
            )
        })?;

    let new_hash: String = match find_latest_version(
        &resolved_synapse.url,
        &current_hash,
        &source_domain,
        resolved_synapse.token_secret.as_deref(),
    )
    .await
    {
        Ok(Some(node)) => {
            let parsed = CmnUri::parse(&node.uri).map_err(|e| {
                crate::HyphaError::new("grow_error", format!("Invalid lineage URI: {}", e))
            })?;
            parsed
                .hash
                .ok_or_else(|| crate::HyphaError::new("grow_error", "Lineage URI has no hash"))?
        }
        Ok(None) => {
            return Ok(crate::output::GrowOutput::UpToDate {
                uri: spawned_uri,
                hash: current_hash,
            });
        }
        Err(e) => return Err(crate::HyphaError::new("synapse_error", e)),
    };

    if new_hash == current_hash {
        return Ok(crate::output::GrowOutput::UpToDate {
            uri: spawned_uri,
            hash: current_hash,
        });
    }

    // Step 3: VERIFY
    sink.emit(crate::HyphaEvent::Progress {
        current: 3,
        total: 6,
        message: "Verifying new spore".to_string(),
    });

    let cache = CacheDir::new();
    let domain_cache = cache.domain(&source_domain);
    let new_uri_str = format!("cmn://{}/{}", source_domain, new_hash);

    let _ = std::fs::remove_file(domain_cache.cmn_path());

    let entry = get_cmn_entry(sink, &domain_cache, cache.cmn_ttl_ms).await?;

    let capsule = primary_capsule(&entry)?;
    let public_key = capsule.key.clone();
    let ep = &capsule.endpoints;

    let new_manifest = fetch_spore_manifest(capsule, &new_hash)
        .await
        .map_err(|e| {
            crate::HyphaError::new(
                "manifest_failed",
                format!("Failed to fetch new spore: {}", e),
            )
        })?;
    let new_spore = decode_spore_manifest(&new_manifest)?;

    verify_manifest_two_key_signatures(&new_manifest, &public_key, &public_key).map_err(|e| {
        crate::HyphaError::new("sig_failed", format!("New spore signature invalid: {}", e))
    })?;

    // Step 4: TASTE
    sink.emit(crate::HyphaEvent::Progress {
        current: 4,
        total: 6,
        message: "Checking taste verdict".to_string(),
    });
    check_taste(sink, &cache, &new_uri_str, &source_domain, &new_hash)?;

    // Resolve distribution strategy before dirty check so we know the apply path.
    let has_local_git = abs_path.join(".git").exists();
    let has_git_dist = new_spore
        .distributions()
        .iter()
        .any(|distribution| distribution.is_git());
    let has_spawn_remote = has_local_git
        && crate::git::get_remote_url(&abs_path, "spawn")
            .ok()
            .flatten()
            .is_some();
    let use_git = match dist_preference {
        Some("git") => true,
        Some("archive") => false,
        _ => has_spawn_remote && has_git_dist,
    };

    // Helper: build merge hint with old + new cache paths.
    let merge_hint = |reason: &str| -> crate::HyphaError {
        let old_path = domain_cache.spore_path(&current_hash);
        let new_path = domain_cache.spore_path(&new_hash);
        crate::HyphaError::with_hint(
            "LOCAL_MODIFIED",
            reason,
            format!(
                "to merge manually, compare old vs new and apply the diff:\n    \
                 hypha taste {}\n  \
                 Old: {}/content/\n  \
                 New: {}/content/",
                new_uri_str,
                old_path.display(),
                new_path.display(),
            ),
        )
    };

    // Check for local modifications before applying update.
    if has_local_git {
        if !has_spawn_remote || !use_git {
            // Has git but no spawn remote (or forced archive): cannot auto-merge.
            // Dirty → error. Clean → also error, because archive full-replace in a
            // git repo would destroy user-added files and produce noisy diffs.
            match crate::git::is_working_dir_clean(&abs_path) {
                Ok(true) => {
                    return Err(crate::HyphaError::with_hint(
                        "NO_SPAWN_REMOTE",
                        "Cannot auto-update: git repo has no spawn remote",
                        format!(
                            "to merge manually, compare old vs new and apply the diff:\n    \
                             hypha taste {}\n  \
                             Old: {}/content/\n  \
                             New: {}/content/",
                            new_uri_str,
                            domain_cache.spore_path(&current_hash).display(),
                            domain_cache.spore_path(&new_hash).display(),
                        ),
                    ));
                }
                Ok(false) => return Err(merge_hint("Working directory has uncommitted changes")),
                Err(e) => {
                    return Err(crate::HyphaError::new(
                        "grow_error",
                        format!("Failed to check git status: {}", e),
                    ));
                }
            }
        }
        // Has spawn remote + git dist: check dirty
        match crate::git::is_working_dir_clean(&abs_path) {
            Ok(true) => {}
            Ok(false) => return Err(merge_hint("Working directory has uncommitted changes")),
            Err(e) => {
                return Err(crate::HyphaError::new(
                    "grow_error",
                    format!("Failed to check git status: {}", e),
                ));
            }
        }
    } else {
        // No .git: check for symlinks, then compare tree hash against spawned_from hash
        crate::tree::check_no_symlinks(
            &abs_path,
            &local_core.tree.exclude_names,
            &local_core.tree.follow_rules,
        )
        .map_err(|e| crate::HyphaError::new("SYMLINK_ERR", format!("{}", e)))?;
        let local_hash =
            crate::tree::compute_tree_hash(&abs_path, &local_core.tree).map_err(|e| {
                crate::HyphaError::new(
                    "grow_error",
                    format!("Failed to compute local tree hash: {}", e),
                )
            })?;
        if local_hash != current_hash {
            return Err(merge_hint("Local files differ from spawned version"));
        }
    }

    // Step 5: APPLY (resolve dist + download)

    let method = if use_git {
        pull_from_git_lib(
            sink,
            &abs_path,
            &new_spore,
            &new_manifest,
            &new_hash,
            &domain_cache,
        )
        .await?
    } else {
        pull_from_archive_lib(
            sink,
            &abs_path,
            &source_domain,
            &new_spore,
            &new_manifest,
            &new_hash,
            ep,
            &current_hash,
        )
        .await?
    };

    // Step 6: COMPLETE
    sink.emit(crate::HyphaEvent::Progress {
        current: 6,
        total: 6,
        message: "Complete".to_string(),
    });

    if bond {
        if let Err(e) = bond_in_dir(&abs_path, false, false, sink).await {
            sink.emit(crate::HyphaEvent::Warn {
                message: format!("Bond failed after grow: {}", e),
            });
        }
    }

    Ok(crate::output::GrowOutput::Updated {
        uri: new_uri_str,
        old_hash: current_hash,
        new_hash,
        method,
        path: abs_path.display().to_string(),
    })
}

pub async fn handle_grow(
    out: &Output,
    path: Option<&str>,
    dist_preference: Option<&str>,
    bond: bool,
    synapse_arg: Option<&str>,
    synapse_token_secret: Option<&str>,
) -> ExitCode {
    let sink = crate::api::OutSink(out);

    // Step 1: grow spawned_from (always)
    let grow_result = match grow(
        path,
        dist_preference,
        bond,
        synapse_arg,
        synapse_token_secret,
        &sink,
    )
    .await
    {
        Ok(output) => serde_json::to_value(output).unwrap_or_default(),
        Err(e) => return out.error_hypha(&e),
    };

    // Step 2: --bond also checks bonds for updates via lineage + fetches them
    if bond {
        let dir = match path
            .map(std::path::PathBuf::from)
            .map(Ok)
            .unwrap_or_else(std::env::current_dir)
        {
            Ok(d) => d,
            Err(e) => return out.error("dir_error", &format!("{}", e)),
        };

        // Check bonds for updates (requires synapse)
        if let Ok(resolved) = crate::config::resolve_synapse(synapse_arg, synapse_token_secret) {
            match update_bonds(&dir, &resolved.url, resolved.token_secret.as_deref(), &sink).await {
                Ok(_) => {}
                Err(e) => {
                    crate::EventSink::emit(
                        &sink,
                        crate::HyphaEvent::Warn {
                            message: format!("Bond update check failed: {}", e),
                        },
                    );
                }
            }
        }

        // Fetch bonds to .cmn/bonds/
        match super::bond::bond_fetch(&dir, false, false, &sink).await {
            Ok(_) => {}
            Err(e) => {
                crate::EventSink::emit(
                    &sink,
                    crate::HyphaEvent::Warn {
                        message: format!("Bond fetch failed: {}", e),
                    },
                );
            }
        }
    }

    out.ok(grow_result)
}

/// Check if a spore has a newer version via Synapse lineage, verify and taste-check it.
///
/// Returns `Ok(Some((new_uri, new_hash)))` if an update is available and verified,
/// `Ok(None)` if already up to date.
///
/// Shared between `grow` (spawned_from update) and `grow --update-bonds`.
async fn pull_from_git_lib(
    sink: &dyn crate::EventSink,
    abs_path: &std::path::Path,
    new_spore: &substrate::Spore,
    new_manifest: &serde_json::Value,
    new_hash: &str,
    domain_cache: &DomainCache,
) -> Result<String, crate::HyphaError> {
    let new_git_dist = new_spore
        .distributions()
        .iter()
        .find(|distribution| distribution.is_git());

    let (new_git_url, new_git_ref) = match new_git_dist {
        Some(d) => (dist_git_url(d), dist_git_ref(d)),
        None => {
            return Err(crate::HyphaError::new(
                "grow_error",
                "New spore version has no git distribution",
            ))
        }
    };

    let new_git_url = new_git_url
        .ok_or_else(|| crate::HyphaError::new("grow_error", "New spore has empty git URL"))?;

    let new_git_ref = new_git_ref.ok_or_else(|| {
        crate::HyphaError::new(
            "grow_error",
            "New spore has no git ref. Cannot verify hash without specific ref.",
        )
    })?;

    if let Ok(Some(old_git_url)) = crate::git::get_remote_url(abs_path, "spawn") {
        let old_url_clean = old_git_url.trim_start_matches("file://");
        let old_is_http = reqwest::Url::parse(old_url_clean)
            .ok()
            .is_some_and(|u| u.scheme() == "http" || u.scheme() == "https");
        if old_is_http && new_git_url != old_url_clean {
            return Err(crate::HyphaError::new("GIT_URL_CHANGED", format!(
                "Git repository URL has changed:\n  Original: {}\n  Current:  {}\n\nUse 'hypha spawn' to spawn the new repository.",
                old_url_clean, new_git_url
            )));
        }
    }

    let root_commit = crate::git::get_root_commit(abs_path).map_err(|e| {
        crate::HyphaError::new("grow_error", format!("Failed to get root commit: {}", e))
    })?;

    let bare_repo_path = domain_cache.repo_path(&root_commit);
    if bare_repo_path.exists() {
        crate::git::fetch_to_bare(&bare_repo_path, new_git_url).map_err(|e| {
            crate::HyphaError::new("grow_error", format!("Failed to fetch to cache: {}", e))
        })?;
    } else {
        std::fs::create_dir_all(domain_cache.repos_dir()).map_err(|e| {
            crate::HyphaError::new("grow_error", format!("Failed to create repos dir: {}", e))
        })?;
        crate::git::clone_bare_repo(new_git_url, &bare_repo_path).map_err(|e| {
            crate::HyphaError::new("grow_error", format!("Failed to clone bare repo: {}", e))
        })?;
    }

    match crate::git::commit_exists(&bare_repo_path, &root_commit) {
        Ok(true) => {}
        Ok(false) => {
            return Err(crate::HyphaError::new("REPO_IDENTITY_ERR", format!(
                "Repository identity mismatch! Root commit {} not found.\nUse 'hypha spawn' to spawn fresh.",
                root_commit
            )));
        }
        Err(e) => {
            return Err(crate::HyphaError::new(
                "grow_error",
                format!("Failed to verify repository identity: {}", e),
            ));
        }
    }

    let old_head = crate::git::get_head_commit(abs_path).map_err(|e| {
        crate::HyphaError::new("grow_error", format!("Failed to get current HEAD: {}", e))
    })?;

    let bare_url = format!("file://{}", bare_repo_path.display());
    if crate::git::get_remote_url(abs_path, "spawn")
        .ok()
        .flatten()
        .is_none()
    {
        let _ = crate::git::add_remote(abs_path, "spawn", &bare_url);
    } else {
        let _ = crate::git::set_remote_url(abs_path, "spawn", &bare_url);
    }

    crate::git::fetch_from_remote(abs_path, "spawn").map_err(|e| {
        crate::HyphaError::new(
            "grow_error",
            format!("Failed to fetch from spawn remote: {}", e),
        )
    })?;

    if let Err(e) = crate::git::checkout_ref(abs_path, new_git_ref) {
        let _ = crate::git::checkout_ref(abs_path, &old_head);
        return Err(crate::HyphaError::new(
            "grow_error",
            format!("Failed to checkout ref {}: {}", new_git_ref, e),
        ));
    }

    if let Err(e) = save_spawned_from_manifest(abs_path, new_manifest) {
        sink.emit(crate::HyphaEvent::Warn {
            message: format!("Failed to update .cmn/spawned-from/spore.json: {}", e),
        });
    }

    if let Err(e) = verify_content_hash(abs_path, new_hash, new_manifest) {
        let _ = crate::git::checkout_ref(abs_path, &old_head);
        return Err(crate::HyphaError::new(
            "hash_mismatch",
            format!("Content hash mismatch after pull, rolled back: {}", e),
        ));
    }

    Ok("git".to_string())
}

#[allow(clippy::too_many_arguments)]
async fn pull_from_archive_lib(
    sink: &dyn crate::EventSink,
    abs_path: &std::path::Path,
    source_domain: &str,
    new_spore: &substrate::Spore,
    new_manifest: &serde_json::Value,
    new_hash: &str,
    endpoints: &[substrate::CmnEndpoint],
    current_hash: &str,
) -> Result<String, crate::HyphaError> {
    let has_archive_dist = new_spore
        .distributions()
        .iter()
        .any(|distribution| distribution.is_archive());
    if !has_archive_dist {
        return Err(crate::HyphaError::new(
            "grow_error",
            "New spore version has no archive distribution",
        ));
    }

    let temp_dir = tempfile::tempdir().map_err(|e| {
        crate::HyphaError::new("grow_error", format!("Failed to create temp dir: {}", e))
    })?;

    let extract_path = temp_dir.path().join("content");
    std::fs::create_dir_all(&extract_path).map_err(|e| {
        crate::HyphaError::new("grow_error", format!("Failed to create extract dir: {}", e))
    })?;

    let mut extracted = false;
    let cache = CacheDir::new();
    let limits = ExtractLimits::from_cache(&cache);
    let domain_cache = cache.domain(source_domain);
    let normalized_old_hash = substrate::parse_hash(current_hash)
        .ok()
        .map(|hash| substrate::format_hash(hash.algorithm, &hash.bytes));
    if let Some(old_hash) = normalized_old_hash {
        let old_archive_cache = cache
            .domain(source_domain)
            .spore_path(&old_hash)
            .join("archive.tar.zst");
        if old_archive_cache.exists() {
            for archive_ep in endpoints
                .iter()
                .filter(|endpoint| endpoint.kind == "archive")
            {
                if archive_ep.format.as_deref() != Some("tar+zstd") {
                    continue;
                }
                let delta_url =
                    match build_archive_delta_url_from_endpoint(archive_ep, new_hash, &old_hash) {
                        Ok(Some(url)) => url,
                        Ok(None) => continue,
                        Err(e) => {
                            sink.emit(crate::HyphaEvent::Warn { message: e });
                            continue;
                        }
                    };

                match download_and_apply_delta(
                    &delta_url,
                    &old_archive_cache,
                    &extract_path,
                    &limits,
                    cache.max_download_bytes,
                )
                .await
                {
                    Ok(raw_tar_file) => {
                        cache_archive_raw_file(
                            &cache,
                            source_domain,
                            new_hash,
                            raw_tar_file.path(),
                            limits.max_bytes,
                        );
                        extracted = true;
                        break;
                    }
                    Err(e) if e.is_malicious() => {
                        let msg = e.to_string();
                        mark_toxic(&domain_cache, new_hash, &msg);
                        return Err(crate::HyphaError::new("TOXIC", msg));
                    }
                    Err(e) => {
                        sink.emit(crate::HyphaEvent::Warn {
                            message: format!(
                                "Delta download failed for format {:?}: {}",
                                archive_ep.format, e
                            ),
                        });
                    }
                }
            }
        }
    }

    if !extracted {
        let mut last_error = String::new();
        for archive_ep in endpoints
            .iter()
            .filter(|endpoint| endpoint.kind == "archive")
        {
            let resolved_url = build_archive_url_from_endpoint(archive_ep, new_hash)
                .map_err(|e| crate::HyphaError::new("url_error", e))?;
            let archive_path = temp_dir.path().join("archive");

            if let Err(e) =
                download_file(&resolved_url, &archive_path, cache.max_download_bytes).await
            {
                last_error = format!("{}: {}", resolved_url, e);
                continue;
            }

            match extract_archive(
                &archive_path,
                &extract_path,
                &resolved_url,
                archive_ep.format.as_deref(),
                &limits,
            ) {
                Ok(_) => {
                    extracted = true;
                    break;
                }
                Err(e) if e.is_malicious() => {
                    let msg = e.to_string();
                    mark_toxic(&domain_cache, new_hash, &msg);
                    return Err(crate::HyphaError::new("TOXIC", msg));
                }
                Err(e) => {
                    last_error =
                        format!("{} (format {:?}): {}", resolved_url, archive_ep.format, e);
                }
            }
        }
        if !extracted {
            return Err(crate::HyphaError::new(
                "fetch_failed",
                format!("Failed to download/extract archive: {}", last_error),
            ));
        }
    }

    verify_content_hash(&extract_path, new_hash, new_manifest).map_err(|e| {
        crate::HyphaError::new("hash_mismatch", format!("Content hash mismatch: {}", e))
    })?;

    remove_tracked_files(abs_path).map_err(|e| {
        crate::HyphaError::new("grow_error", format!("Failed to remove old files: {}", e))
    })?;

    copy_directory_contents(&extract_path, abs_path).map_err(|e| {
        crate::HyphaError::new("grow_error", format!("Failed to copy new files: {}", e))
    })?;

    if let Err(e) = save_spawned_from_manifest(abs_path, new_manifest) {
        sink.emit(crate::HyphaEvent::Warn {
            message: format!("Failed to update .cmn/spawned-from/spore.json: {}", e),
        });
    }

    Ok("archive".to_string())
}

/// Remove all tracked files from the working directory (except .git)
fn remove_tracked_files(repo_path: &std::path::Path) -> Result<(), String> {
    for entry in walkdir::WalkDir::new(repo_path)
        .min_depth(1)
        .into_iter()
        .filter_entry(|e| e.file_name() != ".git" && e.file_name() != ".cmn")
    {
        let entry = entry.map_err(|e| format!("Failed to walk directory: {}", e))?;
        let path = entry.path();

        if path.is_file() {
            std::fs::remove_file(path)
                .map_err(|e| format!("Failed to remove {}: {}", path.display(), e))?;
        }
    }

    // Remove empty directories (except .git)
    for entry in walkdir::WalkDir::new(repo_path)
        .min_depth(1)
        .contents_first(true)
        .into_iter()
        .filter_entry(|e| e.file_name() != ".git" && e.file_name() != ".cmn")
    {
        let entry = entry.map_err(|e| format!("Failed to walk directory: {}", e))?;
        let path = entry.path();

        if path.is_dir() {
            // Only remove if empty
            if std::fs::read_dir(path)
                .map(|mut d| d.next().is_none())
                .unwrap_or(false)
            {
                let _ = std::fs::remove_dir(path);
            }
        }
    }

    Ok(())
}

/// Save the source manifest used for grow to `.cmn/spawned-from/spore.json`.
/// This keeps future grow/absorb anchored to the latest parent hash.
fn save_spawned_from_manifest(
    project_dir: &Path,
    manifest: &serde_json::Value,
) -> Result<(), String> {
    let spore = substrate::decode_spore(manifest)
        .map_err(|e| format!("Invalid source spore manifest: {}", e))?;
    let pretty = spore
        .to_pretty_json()
        .map_err(|e| format!("Failed to format source spore manifest: {}", e))?;

    let spawned_from_path = project_dir
        .join(".cmn")
        .join("spawned-from")
        .join("spore.json");
    if let Some(parent) = spawned_from_path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to create spawned-from directory: {}", e))?;
    }
    std::fs::write(&spawned_from_path, pretty)
        .map_err(|e| format!("Failed to write {}: {}", spawned_from_path.display(), e))?;
    Ok(())
}

/// Copy directory contents from src to dest.
/// Rejects symlinks — only regular files and directories are copied.
fn copy_directory_contents(
    src: &std::path::Path,
    dest: &std::path::Path,
) -> Result<(), ExtractError> {
    // Canonicalize dest for bounds checking (create it first if needed)
    std::fs::create_dir_all(dest).map_err(|e| format!("Failed to create dest directory: {}", e))?;
    let canonical_dest = dest
        .canonicalize()
        .map_err(|e| format!("Failed to canonicalize dest: {}", e))?;

    for entry in walkdir::WalkDir::new(src).min_depth(1).follow_links(false) {
        let entry = entry.map_err(|e| format!("Failed to walk directory: {}", e))?;
        let src_path = entry.path();
        let ft = entry.file_type();

        let relative = src_path
            .strip_prefix(src)
            .map_err(|e| format!("Failed to get relative path: {}", e))?;
        let dest_path = dest.join(relative);

        // Bounds check: ensure dest_path doesn't escape dest via ".." components
        // Use lexical normalization since dest_path may not exist yet
        let normalized = normalize_path(&dest_path);
        if !normalized.starts_with(&canonical_dest) {
            return Err(ExtractError::Malicious(format!(
                "path traversal detected during copy: {}",
                relative.display()
            )));
        }

        // Reject symlinks
        if ft.is_symlink() {
            return Err(ExtractError::Malicious(format!(
                "symlink found during copy: {}",
                src_path.display()
            )));
        }

        if ft.is_dir() {
            std::fs::create_dir_all(&dest_path).map_err(|e| {
                format!("Failed to create directory {}: {}", dest_path.display(), e)
            })?;
        } else if ft.is_file() {
            if let Some(parent) = dest_path.parent() {
                std::fs::create_dir_all(parent).map_err(|e| {
                    format!(
                        "Failed to create parent directory {}: {}",
                        parent.display(),
                        e
                    )
                })?;
            }
            std::fs::copy(src_path, &dest_path)
                .map_err(|e| format!("Failed to copy {}: {}", src_path.display(), e))?;
        }
    }

    Ok(())
}

/// Normalize a path by resolving `.` and `..` components lexically (without I/O).
fn normalize_path(path: &std::path::Path) -> std::path::PathBuf {
    use std::path::Component;
    let mut result = std::path::PathBuf::new();
    for component in path.components() {
        match component {
            Component::ParentDir => {
                result.pop();
            }
            Component::CurDir => {}
            _ => result.push(component),
        }
    }
    result
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn remove_tracked_files_preserves_cmn_metadata() {
        let temp = tempfile::tempdir().unwrap();
        let project = temp.path();

        let app_file = project.join("src/main.rs");
        std::fs::create_dir_all(app_file.parent().unwrap()).unwrap();
        std::fs::write(&app_file, "fn main() {}\n").unwrap();

        let spawned_from = project.join(".cmn/spawned-from/spore.json");
        std::fs::create_dir_all(spawned_from.parent().unwrap()).unwrap();
        std::fs::write(&spawned_from, "{}").unwrap();

        remove_tracked_files(project).unwrap();

        assert!(!app_file.exists(), "project files should be removed");
        assert!(
            spawned_from.exists(),
            ".cmn metadata must be preserved for future grow/absorb"
        );
    }

    #[test]
    fn save_spawned_from_manifest_writes_latest_manifest() {
        let temp = tempfile::tempdir().unwrap();
        let project = temp.path();
        let manifest = json!({
            "$schema": "https://cmn.dev/schemas/v1/spore.json",
            "capsule": {
                "uri": "cmn://example.com/b3.3yMR7vZQ9hL2xKJdFtN8wPcB6sY1mXgU4eH5pTa2",
                "core": {
                    "name": "demo",
                    "domain": "example.com",
                    "key": "ed25519.5XmkQ9vZP8nL3xJdFtR7wNcA6sY2bKgU1eH9pXb4",
                    "synopsis": "demo",
                    "intent": [],
                    "license": "MIT",
                    "mutations": [],
                    "size_bytes": 1,
                    "updated_at_epoch_ms": 1_u64,
                    "bonds": [],
                    "tree": {
                        "algorithm": "blob_tree_blake3_nfc",
                        "exclude_names": [],
                        "follow_rules": []
                    }
                },
                "core_signature": "ed25519.3yMR7vZQ9hL2xKJdFtN8wPcB6sY1mXgU4eH5pTa23yMR7vZQ9hL2xKJdFtN8wPcB6sY1mXgU4eH5pTa2",
                "dist": [
                    {"type": "archive"}
                ]
            },
            "capsule_signature": "ed25519.3yMR7vZQ9hL2xKJdFtN8wPcB6sY1mXgU4eH5pTa23yMR7vZQ9hL2xKJdFtN8wPcB6sY1mXgU4eH5pTa2"
        });

        save_spawned_from_manifest(project, &manifest).unwrap();

        let saved_path = project.join(".cmn/spawned-from/spore.json");
        assert!(saved_path.exists());

        let saved: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(saved_path).unwrap()).unwrap();
        assert_eq!(
            saved.pointer("/capsule/uri").and_then(|v| v.as_str()),
            Some("cmn://example.com/b3.3yMR7vZQ9hL2xKJdFtN8wPcB6sY1mXgU4eH5pTa2")
        );
    }
}
