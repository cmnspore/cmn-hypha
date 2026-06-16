use std::path::Path;

use super::*;

pub(super) async fn pull_from_git_lib(
    sink: &dyn crate::EventSink,
    abs_path: &std::path::Path,
    new_spore: &substrate::Spore,
    new_manifest: &serde_json::Value,
    new_hash: &str,
    domain_cache: &DomainCache,
    cache: &CacheDir,
) -> Result<String, crate::HyphaError> {
    let git_limits = crate::git::GitSizeLimits::new(
        cache.spore_max_extract_bytes,
        cache.spore_max_extract_files,
    );
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
    crate::git::enforce_size_budget(&bare_repo_path, git_limits).map_err(|e| {
        crate::HyphaError::new("grow_error", format!("Git repo exceeds size budget: {}", e))
    })?;

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
    crate::git::configure_blobless_promisor_remote(
        abs_path,
        crate::git::CMN_PROMISOR_REMOTE,
        new_git_url,
    )
    .map_err(|e| {
        crate::HyphaError::new(
            "grow_error",
            format!("Failed to configure git promisor remote: {}", e),
        )
    })?;

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
pub(super) async fn pull_from_archive_lib(
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
    let cache = CacheDir::new()?;
    let limits = ExtractLimits::from_cache(&cache);
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
                            sink.emit(crate::HyphaEvent::Warn {
                                message: e.to_string(),
                            });
                            continue;
                        }
                    };

                match download_and_apply_delta(
                    &delta_url,
                    &old_archive_cache,
                    &extract_path,
                    &limits,
                    cache.spore_max_download_bytes,
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
                    Err(e) if e.is_policy_rejected() => {
                        return Err(crate::HyphaError::new(
                            "spore_security_rejected",
                            e.to_string(),
                        ));
                    }
                    Err(e) if e.is_malicious() => {
                        sink.emit(crate::HyphaEvent::Warn {
                            message: format!(
                                "Delta content was rejected before verification: {}",
                                e
                            ),
                        });
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
            let resolved_url = build_archive_url_from_endpoint(archive_ep, new_hash)?;
            let archive_path = temp_dir.path().join("archive");

            if let Err(e) =
                download_file(&resolved_url, &archive_path, cache.spore_max_download_bytes).await
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
                Err(e) if e.is_policy_rejected() => {
                    return Err(crate::HyphaError::new(
                        "spore_security_rejected",
                        e.to_string(),
                    ));
                }
                Err(e) if e.is_malicious() => {
                    last_error = format!(
                        "{} (format {:?}): unverified content rejected: {}",
                        resolved_url, archive_ep.format, e
                    );
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

    ensure_no_rejected_path_components(&extract_path, &cache.spore_reject_path_components)
        .map_err(|e| crate::HyphaError::new("spore_security_rejected", e.to_string()))?;

    replace_working_tree(abs_path, &extract_path, &cache.spore_reject_path_components)?;

    if let Err(e) = save_spawned_from_manifest(abs_path, new_manifest) {
        sink.emit(crate::HyphaEvent::Warn {
            message: format!("Failed to update .cmn/spawned-from/spore.json: {}", e),
        });
    }

    Ok("archive".to_string())
}

/// Remove all tracked files from the working directory (except .git)
pub(super) fn remove_tracked_files(repo_path: &std::path::Path) -> Result<(), crate::HyphaError> {
    for entry in walkdir::WalkDir::new(repo_path)
        .min_depth(1)
        .into_iter()
        .filter_entry(|e| e.file_name() != ".git" && e.file_name() != ".cmn")
    {
        let entry = entry.map_err(|e| {
            crate::HyphaError::new("grow_error", format!("Failed to walk directory: {}", e))
        })?;
        let path = entry.path();

        if path.is_file() {
            std::fs::remove_file(path).map_err(|e| {
                crate::HyphaError::new(
                    "grow_error",
                    format!("Failed to remove {}: {}", path.display(), e),
                )
            })?;
        }
    }

    for entry in walkdir::WalkDir::new(repo_path)
        .min_depth(1)
        .contents_first(true)
        .into_iter()
        .filter_entry(|e| e.file_name() != ".git" && e.file_name() != ".cmn")
    {
        let entry = entry.map_err(|e| {
            crate::HyphaError::new("grow_error", format!("Failed to walk directory: {}", e))
        })?;
        let path = entry.path();

        if path.is_dir()
            && std::fs::read_dir(path)
                .map(|mut d| d.next().is_none())
                .unwrap_or(false)
        {
            let _ = std::fs::remove_dir(path);
        }
    }

    Ok(())
}

/// Replace the working tree's tracked files with verified new content while
/// preserving `.git`/`.cmn`.
///
/// The expensive, failure-prone work (copying the verified content onto the
/// target filesystem) is done into a sibling staging directory *before* any
/// destruction. Only then are the old tracked files removed and the staged
/// content moved in via fast same-filesystem renames. If the move still fails
/// partway, the complete new content remains in the staging directory and its
/// location is reported, so the working tree is never left silently broken.
fn replace_working_tree(
    abs_path: &std::path::Path,
    new_content: &std::path::Path,
    reject_path_components: &[String],
) -> Result<(), crate::HyphaError> {
    let parent = abs_path.parent().ok_or_else(|| {
        crate::HyphaError::new(
            "grow_error",
            "Cannot determine working tree parent directory",
        )
    })?;

    // Staging dir on the same filesystem as abs_path so the final moves are renames.
    let staging = tempfile::TempDir::new_in(parent).map_err(|e| {
        crate::HyphaError::new("grow_error", format!("Failed to create staging dir: {}", e))
    })?;
    // Detach from RAII so a mid-swap failure doesn't auto-delete staged content.
    let staging_path = staging.keep();

    // 1. Materialize the new content in staging (non-destructive).
    if let Err(e) = copy_directory_contents(new_content, &staging_path, reject_path_components) {
        let _ = std::fs::remove_dir_all(&staging_path);
        let code = if e.is_policy_rejected() {
            "spore_security_rejected"
        } else {
            "grow_error"
        };
        return Err(crate::HyphaError::new(
            code,
            format!("Failed to stage new files: {}", e),
        ));
    }

    // Defense in depth: re-check staged content before mutating the existing tree.
    if let Err(e) = ensure_no_rejected_path_components(&staging_path, reject_path_components) {
        let _ = std::fs::remove_dir_all(&staging_path);
        return Err(crate::HyphaError::new(
            "spore_security_rejected",
            e.to_string(),
        ));
    }

    // 2. Remove old tracked files (keeps .git/.cmn).
    if let Err(e) = remove_tracked_files(abs_path) {
        let _ = std::fs::remove_dir_all(&staging_path);
        return Err(crate::HyphaError::new(
            "grow_error",
            format!("Failed to remove old files: {}", e),
        ));
    }

    // 3. Move staged content into place (fast same-fs renames).
    if let Err(e) = move_dir_contents(&staging_path, abs_path) {
        return Err(crate::HyphaError::new(
            "grow_error",
            format!(
                "Working tree update failed after removing old files; the new content \
                 is staged at {} — move it into place manually: {}",
                staging_path.display(),
                e
            ),
        ));
    }

    let _ = std::fs::remove_dir_all(&staging_path);
    Ok(())
}

/// Move every top-level entry from `from` into `into` via rename.
fn move_dir_contents(from: &std::path::Path, into: &std::path::Path) -> std::io::Result<()> {
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        std::fs::rename(entry.path(), into.join(entry.file_name()))?;
    }
    Ok(())
}

/// Save the source manifest used for grow to `.cmn/spawned-from/spore.json`.
pub(super) fn save_spawned_from_manifest(
    project_dir: &Path,
    manifest: &serde_json::Value,
) -> Result<(), crate::HyphaError> {
    let spore = substrate::decode_spore(manifest).map_err(|e| {
        crate::HyphaError::new(
            "grow_error",
            format!("Invalid source spore manifest: {}", e),
        )
    })?;
    let pretty = spore.to_pretty_json().map_err(|e| {
        crate::HyphaError::new(
            "grow_error",
            format!("Failed to format source spore manifest: {}", e),
        )
    })?;

    let spawned_from_path = project_dir
        .join(".cmn")
        .join("spawned-from")
        .join("spore.json");
    if let Some(parent) = spawned_from_path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| {
            crate::HyphaError::new(
                "grow_error",
                format!("Failed to create spawned-from directory: {}", e),
            )
        })?;
    }
    std::fs::write(&spawned_from_path, pretty).map_err(|e| {
        crate::HyphaError::new(
            "grow_error",
            format!("Failed to write {}: {}", spawned_from_path.display(), e),
        )
    })?;
    Ok(())
}

/// Copy directory contents from src to dest.
/// Rejects symlinks — only regular files and directories are copied.
pub(super) fn copy_directory_contents(
    src: &std::path::Path,
    dest: &std::path::Path,
    reject_path_components: &[String],
) -> Result<(), ExtractError> {
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
        if let Some(component) = rejected_path_component(relative, reject_path_components) {
            return Err(ExtractError::PolicyRejected(format!(
                "received spore content contains protected path component '{}': {}",
                component,
                relative.display()
            )));
        }
        let dest_path = dest.join(relative);

        let normalized = normalize_path(&canonical_dest.join(relative));
        if !normalized.starts_with(&canonical_dest) {
            return Err(ExtractError::Malicious(format!(
                "path traversal detected during copy: {}",
                relative.display()
            )));
        }

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
    use serde_json::json;

    use super::*;

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

    #[test]
    fn replace_working_tree_rejects_control_paths_before_mutation() {
        let temp = tempfile::tempdir().unwrap();
        let project = temp.path().join("project");
        let new_content = temp.path().join("new-content");
        std::fs::create_dir_all(&project).unwrap();
        std::fs::write(project.join("old.txt"), "old\n").unwrap();
        std::fs::create_dir_all(project.join(".cmn/spawned-from")).unwrap();
        std::fs::write(project.join(".cmn/spawned-from/spore.json"), "{}").unwrap();

        std::fs::create_dir_all(new_content.join(".git")).unwrap();
        std::fs::write(new_content.join(".git/config"), "malicious\n").unwrap();
        std::fs::write(new_content.join("new.txt"), "new\n").unwrap();

        let reject = vec![".git".to_string(), ".cmn".to_string()];
        let err = replace_working_tree(&project, &new_content, &reject).unwrap_err();

        assert_eq!(err.code, "spore_security_rejected");
        assert!(
            project.join("old.txt").exists(),
            "existing tree must remain untouched"
        );
        assert!(
            project.join(".cmn/spawned-from/spore.json").exists(),
            ".cmn metadata must remain untouched"
        );
        assert!(
            !project.join("new.txt").exists(),
            "rejected content must not be moved into place"
        );
    }

    #[test]
    fn copy_directory_contents_allows_gitignore_and_similar_names() {
        let temp = tempfile::tempdir().unwrap();
        let src = temp.path().join("src");
        let dest = temp.path().join("dest");
        std::fs::create_dir_all(src.join("docs")).unwrap();
        std::fs::write(src.join(".gitignore"), "target/\n").unwrap();
        std::fs::write(src.join(".gitattributes"), "* text=auto\n").unwrap();
        std::fs::write(src.join("docs/cmn-notes.md"), "ok\n").unwrap();
        std::fs::write(src.join("docs/git-notes.md"), "ok\n").unwrap();

        let reject = vec![".git".to_string(), ".cmn".to_string()];
        copy_directory_contents(&src, &dest, &reject).unwrap();

        assert!(dest.join(".gitignore").exists());
        assert!(dest.join(".gitattributes").exists());
        assert!(dest.join("docs/cmn-notes.md").exists());
        assert!(dest.join("docs/git-notes.md").exists());
    }
}
