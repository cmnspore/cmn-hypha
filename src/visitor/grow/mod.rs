use super::*;

mod apply;
mod resolve;

use apply::{pull_from_archive_lib, pull_from_git_lib};
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
        sink,
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
        Err(e) => return Err(e),
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

    let cache = CacheDir::new()?;
    let domain_cache = cache.domain(&source_domain);
    let new_uri_str = format!("cmn://{}/{}", source_domain, new_hash);

    let _ = std::fs::remove_file(domain_cache.cmn_path());

    let entry = get_cmn_entry(sink, &domain_cache, cache.cmn_ttl_ms).await?;

    let capsule = primary_capsule(&entry)?;
    let ep = &capsule.endpoints;
    let (new_manifest, new_spore) =
        fetch_verified_spore(sink, capsule, &new_hash, &domain_cache, cache.cmn_ttl_ms).await?;

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
            &cache,
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
