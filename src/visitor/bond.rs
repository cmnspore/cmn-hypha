use super::*;

/// Remove and recreate a directory so extraction/cloning starts from empty.
fn reset_dir(dir: &std::path::Path) -> Result<(), crate::HyphaError> {
    if dir.exists() {
        std::fs::remove_dir_all(dir).map_err(|e| {
            crate::HyphaError::new("bond_error", format!("Failed to reset content dir: {}", e))
        })?;
    }
    std::fs::create_dir_all(dir).map_err(|e| {
        crate::HyphaError::new(
            "bond_error",
            format!("Failed to recreate content dir: {}", e),
        )
    })
}

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
pub(super) struct BondIndexEntry {
    hash: String,
    dir: String,
    cmn_url: String,
    relation: substrate::BondRelation,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    name: Option<String>,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct BondIndexFile {
    #[serde(default)]
    bonds: Vec<BondIndexEntry>,
}

/// Handle `bond` — fetch all bonds from spore.core.json to .cmn/bonds/
pub async fn bond_fetch(
    dir: &std::path::Path,
    clean: bool,
    status: bool,
    sink: &dyn crate::EventSink,
) -> Result<crate::output::BondResult, crate::HyphaError> {
    bond_in_dir(dir, clean, status, sink).await
}

pub async fn handle_bond_fetch(out: &Output, clean: bool, status: bool) -> ExitCode {
    let cwd = match std::env::current_dir() {
        Ok(d) => d,
        Err(e) => {
            return out.error(
                "dir_error",
                &format!("Failed to get current directory: {}", e),
            );
        }
    };
    let sink = crate::api::OutSink(out);
    match bond_in_dir(&cwd, clean, status, &sink).await {
        Ok(output) => out.ok(output),
        Err(e) => out.error_hypha(&e),
    }
}

/// Bond implementation that works in any directory — library level.
pub(super) async fn bond_in_dir(
    dir: &std::path::Path,
    clean: bool,
    status: bool,
    sink: &dyn crate::EventSink,
) -> Result<crate::output::BondResult, crate::HyphaError> {
    let spore_core_path = dir.join("spore.core.json");
    if !spore_core_path.exists() {
        return Err(crate::HyphaError::with_hint(
            "bond_error",
            "spore.core.json not found",
            "run from a spore directory",
        ));
    }

    let spore_core = std::fs::read_to_string(&spore_core_path).map_err(|e| {
        crate::HyphaError::new(
            "bond_error",
            format!("Failed to read spore.core.json: {}", e),
        )
    })?;

    let core: substrate::SporeCore = serde_json::from_str(&spore_core).map_err(|e| {
        crate::HyphaError::new(
            "bond_error",
            format!("Failed to parse spore.core.json: {}", e),
        )
    })?;

    // Collect bondable spore bonds.
    // (uri, domain, hash, relation, id)
    let mut spore_refs: Vec<(
        String,
        String,
        String,
        substrate::BondRelation,
        Option<String>,
    )> = Vec::new();
    for reference in &core.bonds {
        let uri_str = reference.uri.as_str();
        let relation = reference.relation.clone();
        if relation.is_excluded_from_bond_fetch() {
            continue;
        }
        let uri = match CmnUri::parse(uri_str) {
            Ok(u) => u,
            Err(_) => continue,
        };
        let hash = match uri.hash.clone() {
            Some(h) => h,
            None => continue,
        };
        spore_refs.push((
            uri_str.to_string(),
            uri.domain.clone(),
            hash,
            relation,
            reference.id.clone(),
        ));
    }

    let refs_dir = dir.join(".cmn/bonds");
    let refs_json_path = refs_dir.join("bonds.json");
    let index = load_refs_json(&refs_json_path)?;

    // --status
    if status {
        let mut statuses = Vec::new();
        for (uri_str, _domain, hash, relation, _id) in &spore_refs {
            let bonded = index.iter().any(|entry| entry.hash == *hash);
            statuses.push(crate::output::BondStatusRef {
                cmn_url: uri_str.clone(),
                relation: relation.clone(),
                bonded: json!(bonded),
            });
        }
        for reference in &core.bonds {
            let relation = reference.relation.clone();
            if relation.is_excluded_from_bond_fetch() {
                statuses.push(crate::output::BondStatusRef {
                    cmn_url: reference.uri.clone(),
                    relation,
                    bonded: json!("excluded"),
                });
            }
        }
        return Ok(crate::output::BondResult::Status(
            crate::output::BondStatusOutput { bonds: statuses },
        ));
    }

    // --clean
    if clean {
        let mut reserved_dirs = std::collections::HashSet::new();
        let valid_dirs: std::collections::HashSet<String> = spore_refs
            .iter()
            .map(|(_, _, h, _, id)| {
                resolve_bond_dir_name(&refs_dir, &index, &mut reserved_dirs, id.as_deref(), h)
            })
            .collect();
        let cleaned = clean_bond_dirs(&refs_dir, &index, &valid_dirs, sink, |path| {
            std::fs::remove_dir_all(path)
        })?;
        return Ok(crate::output::BondResult::Clean(
            crate::output::BondCleanOutput { cleaned },
        ));
    }

    // Main bond
    if spore_refs.is_empty() {
        return Ok(crate::output::BondResult::Bond(crate::output::BondOutput {
            bonded: vec![],
            message: Some("No spore bonds to fetch".to_string()),
        }));
    }

    // Pre-check taste status for all refs. If any are not tasted or toxic,
    // return the full list so the caller can act on all of them at once.
    let cache = CacheDir::new()?;
    {
        let mut taste_refs = Vec::new();
        let mut any_blocked = false;
        for (uri_str, _domain, hash, relation, id) in &spore_refs {
            let domain_cache = cache.domain(_domain);
            let taste_status = match domain_cache.load_taste(hash) {
                None => {
                    any_blocked = true;
                    "not_tasted".to_string()
                }
                Some(t) if t.verdict == substrate::TasteVerdict::Toxic => {
                    any_blocked = true;
                    "toxic".to_string()
                }
                Some(t) => t.verdict.to_string(),
            };
            taste_refs.push(crate::output::BondTasteRef {
                cmn_url: uri_str.clone(),
                relation: relation.clone(),
                id: id.clone(),
                taste: taste_status,
            });
        }
        if any_blocked {
            return Ok(crate::output::BondResult::TasteRequired(
                crate::output::BondTasteRequired { refs: taste_refs },
            ));
        }
    }

    std::fs::create_dir_all(&refs_dir).map_err(|e| {
        crate::HyphaError::new("bond_error", format!("Failed to create .cmn/bonds/: {}", e))
    })?;

    let mut bonded = Vec::new();
    let mut index_entries: Vec<BondIndexEntry> = Vec::new();
    let mut reserved_dirs = std::collections::HashSet::new();

    for (uri_str, domain, hash, relation, id) in &spore_refs {
        // Taste already verified in pre-check above; re-check for safety.
        check_taste(sink, &cache, uri_str, domain, hash)?;

        let dir_name =
            resolve_bond_dir_name(&refs_dir, &index, &mut reserved_dirs, id.as_deref(), hash);

        if !is_safe_bond_dir_name(&dir_name) {
            return Err(crate::HyphaError::new(
                "bond_error",
                format!("Unsafe bond directory name: '{}'", dir_name),
            ));
        }
        let ref_dir = refs_dir.join(&dir_name);
        let content_dir = ref_dir.join("content");

        if ref_dir.exists() {
            index_entries.push(BondIndexEntry {
                hash: hash.clone(),
                dir: dir_name,
                cmn_url: uri_str.clone(),
                relation: relation.clone(),
                id: id.clone(),
                name: None,
            });
            bonded.push(crate::output::BondedRef {
                cmn_url: uri_str.clone(),
                relation: relation.clone(),
                status: "already_bonded".to_string(),
            });
            continue;
        }

        let domain_cache = cache.domain(domain);

        let entry = get_cmn_entry(sink, &domain_cache, cache.cmn_ttl_ms).await?;
        let capsule = primary_capsule(&entry)?;
        let ep = &capsule.endpoints;
        let (manifest, spore) =
            fetch_verified_spore(sink, capsule, hash, &domain_cache, cache.cmn_ttl_ms).await?;

        std::fs::create_dir_all(&content_dir).map_err(|e| {
            crate::HyphaError::new(
                "bond_error",
                format!("Failed to create {}: {}", content_dir.display(), e),
            )
        })?;

        let manifest_path = ref_dir.join("spore.json");
        let manifest_pretty = serde_json::to_string_pretty(&spore).map_err(|e| {
            crate::HyphaError::new("bond_error", format!("Failed to format spore.json: {}", e))
        })?;
        std::fs::write(&manifest_path, manifest_pretty).map_err(|e| {
            crate::HyphaError::new("bond_error", format!("Failed to write spore.json: {}", e))
        })?;

        let dist_array = spore.distributions();
        if dist_array.is_empty() {
            return Err(crate::HyphaError::new(
                "manifest_failed",
                format!("No distribution options for {}", hash),
            ));
        }

        let archive_endpoints = ep
            .iter()
            .filter(|endpoint| endpoint.kind == "archive")
            .collect::<Vec<_>>();
        let mut downloaded = false;
        for dist_entry in dist_array {
            if dist_has_type(dist_entry, "archive") {
                for archive_ep in &archive_endpoints {
                    let archive_url = build_archive_url_from_endpoint(archive_ep, hash)?;

                    reset_dir(&content_dir)?;

                    match download_and_extract_to_dir(
                        &archive_url,
                        &content_dir,
                        archive_ep.format.as_deref(),
                    )
                    .await
                    {
                        Ok(_) => {
                            downloaded = true;
                            break;
                        }
                        Err(e) if e.is_policy_rejected() => {
                            return Err(crate::HyphaError::new(
                                "spore_security_rejected",
                                e.to_string(),
                            ));
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
                reset_dir(&content_dir)?;

                match clone_git_to_dir(git_url, git_ref, &content_dir, &cache).await {
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
            return Err(crate::HyphaError::new(
                "fetch_failed",
                format!("Failed to download content for {}", hash),
            ));
        }

        verify_downloaded_content(sink, &ref_dir, &content_dir, &manifest, hash, &domain_cache)?;

        let name = spore.capsule.core.name.as_str();

        index_entries.push(BondIndexEntry {
            hash: hash.clone(),
            dir: dir_name,
            cmn_url: uri_str.clone(),
            relation: relation.clone(),
            id: id.clone(),
            name: Some(name.to_string()),
        });

        bonded.push(crate::output::BondedRef {
            cmn_url: uri_str.clone(),
            relation: relation.clone(),
            status: "bonded".to_string(),
        });
    }

    write_refs_json(&refs_json_path, &index_entries)?;

    Ok(crate::output::BondResult::Bond(crate::output::BondOutput {
        bonded,
        message: None,
    }))
}

/// Load the bond index, treating only an absent file as an empty index.
pub(super) fn load_refs_json(
    path: &std::path::Path,
) -> Result<Vec<BondIndexEntry>, crate::HyphaError> {
    let recovery = format!(
        "Check read permissions; if the index is damaged, delete {} and rerun bond to rebuild it; existing directory names may change",
        path.display()
    );
    let source = match std::fs::read_to_string(path) {
        Ok(source) => source,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => {
            return Err(crate::HyphaError::with_hint(
                "bond_read_failed",
                format!("Failed to read {}: {}", path.display(), e),
                recovery,
            ));
        }
    };
    serde_json::from_str::<BondIndexFile>(&source)
        .map(|index| index.bonds)
        .map_err(|e| {
            crate::HyphaError::with_hint(
                "bond_index_invalid",
                format!("Failed to parse {}: {}", path.display(), e),
                recovery,
            )
        })
}

fn clean_bond_dirs(
    refs_dir: &std::path::Path,
    index: &[BondIndexEntry],
    valid_dirs: &std::collections::HashSet<String>,
    sink: &dyn crate::EventSink,
    mut remove_dir: impl FnMut(&std::path::Path) -> std::io::Result<()>,
) -> Result<Vec<String>, crate::HyphaError> {
    let mut removed = Vec::new();
    let mut kept = Vec::new();
    let mut failed = std::collections::HashSet::new();
    let mut attempted = std::collections::HashSet::new();
    let mut errors = Vec::new();
    for entry in index {
        let name = index_dir_name(entry);
        if valid_dirs.contains(name) || !is_safe_bond_dir_name(name) {
            kept.push(entry.clone());
            if !is_safe_bond_dir_name(name) {
                sink.emit(crate::HyphaEvent::Warn {
                    message: format!("Skipping unsafe bond directory in bonds.json: '{name}'"),
                });
            }
            continue;
        }
        if attempted.insert(name.to_string()) {
            match remove_dir(&refs_dir.join(name)) {
                Ok(()) => removed.push(name.to_string()),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => {
                    failed.insert(name.to_string());
                    errors.push(format!("Failed to remove {name}: {e}"));
                }
            }
        }
        if failed.contains(name) {
            kept.push(entry.clone());
        }
    }
    let entries = match std::fs::read_dir(refs_dir) {
        Ok(entries) => Some(entries),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => {
            errors.push(format!("Failed to read bond directory: {e}"));
            None
        }
    };
    if let Some(entries) = entries {
        for entry in entries {
            let entry = match entry {
                Ok(entry) => entry,
                Err(e) => {
                    errors.push(format!("Failed to read bond directory entry: {e}"));
                    continue;
                }
            };
            let name = entry.file_name().to_string_lossy().to_string();
            if name == "bonds.json"
                || valid_dirs.contains(&name)
                || attempted.contains(&name)
                || !is_safe_bond_dir_name(&name)
            {
                continue;
            }
            match entry.file_type() {
                Ok(kind) if kind.is_dir() => {}
                Ok(_) => continue,
                Err(e) => {
                    errors.push(format!("Failed to inspect {name}: {e}"));
                    continue;
                }
            }
            match remove_dir(&entry.path()) {
                Ok(()) => removed.push(name),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => errors.push(format!("Failed to remove {name}: {e}")),
            }
        }
    }
    write_refs_json(&refs_dir.join("bonds.json"), &kept)?;
    if !errors.is_empty() {
        return Err(crate::HyphaError::new(
            "bond_clean_failed",
            errors.join("; "),
        ));
    }
    Ok(removed)
}

fn index_dir_name(entry: &BondIndexEntry) -> &str {
    if entry.dir.is_empty() {
        &entry.hash
    } else {
        &entry.dir
    }
}

fn resolve_bond_dir_name(
    refs_dir: &std::path::Path,
    index: &[BondIndexEntry],
    reserved_dirs: &mut std::collections::HashSet<String>,
    id: Option<&str>,
    hash: &str,
) -> String {
    // Keep persisted paths stable when local slug rules change.
    if let Some(entry) = index.iter().find(|entry| {
        let dir_name = index_dir_name(entry);
        entry.hash == hash
            && is_safe_bond_dir_name(dir_name)
            && !reserved_dirs.contains(dir_name)
            && !index
                .iter()
                .any(|other| other.hash != hash && index_dir_name(other) == dir_name)
    }) {
        let dir_name = index_dir_name(entry).to_string();
        reserved_dirs.insert(dir_name.clone());
        return dir_name;
    }

    let preferred_dir_name = substrate::local_dir_name(id, None, hash);
    let preferred_belongs_to_hash = index
        .iter()
        .any(|entry| entry.hash == hash && index_dir_name(entry) == preferred_dir_name.as_str());
    let dir_name = if preferred_dir_name != hash
        && (reserved_dirs.contains(&preferred_dir_name)
            || index.iter().any(|entry| {
                index_dir_name(entry) == preferred_dir_name.as_str() && entry.hash != hash
            })
            || (refs_dir.join(&preferred_dir_name).exists() && !preferred_belongs_to_hash))
    {
        hash.to_string()
    } else {
        preferred_dir_name
    };
    reserved_dirs.insert(dir_name.clone());
    dir_name
}

/// Write bonds.json index
pub(super) fn write_refs_json(
    path: &std::path::Path,
    entries: &[BondIndexEntry],
) -> Result<(), crate::HyphaError> {
    let index = BondIndexFile {
        bonds: entries.to_vec(),
    };
    let pretty = serde_json::to_string_pretty(&index).map_err(|e| {
        crate::HyphaError::new(
            "bond_write_failed",
            format!("Failed to format bonds: {}", e),
        )
    })?;
    let parent = path.parent().unwrap_or_else(|| std::path::Path::new("."));
    let temp_dir = parent
        .parent()
        .filter(|dir| dir.file_name() == Some(std::ffi::OsStr::new(".cmn")))
        .map(std::path::Path::to_path_buf)
        .unwrap_or_else(|| parent.join(".cmn"));
    crate::cache::fs::atomic_write_file(path, pretty.as_bytes(), &temp_dir, Some(0o644))
        .map_err(|e| crate::HyphaError::new("bond_write_failed", e.to_string()))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;

    fn bond_index_entry(hash: &str, dir: &str) -> BondIndexEntry {
        BondIndexEntry {
            hash: hash.to_string(),
            dir: dir.to_string(),
            cmn_url: format!("cmn://example.com/{hash}"),
            relation: substrate::BondRelation::DependsOn,
            id: Some("İstanbul".to_string()),
            name: None,
        }
    }

    #[test]
    fn load_refs_json_only_defaults_for_missing_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bonds.json");
        assert!(load_refs_json(&path).unwrap().is_empty());

        std::fs::write(&path, "{").unwrap();
        let error = load_refs_json(&path).unwrap_err();
        assert_eq!(error.code, "bond_index_invalid");
        assert!(error.hint.unwrap().contains("delete"));
        std::fs::remove_file(&path).unwrap();

        // A directory is a deterministic read failure, including as root.
        std::fs::create_dir(&path).unwrap();
        assert_eq!(load_refs_json(&path).unwrap_err().code, "bond_read_failed");
    }

    #[tokio::test]
    async fn all_bond_modes_refuse_damaged_or_unreadable_index() {
        let dir = tempfile::tempdir().unwrap();
        let refs_dir = dir.path().join(".cmn/bonds");
        std::fs::create_dir_all(refs_dir.join("keep")).unwrap();
        let marker = refs_dir.join("keep/marker");
        std::fs::write(&marker, "keep").unwrap();
        let core = json!({
            "name": "test", "domain": "example.com", "synopsis": "test",
            "intent": [], "license": "MIT", "bonds": [],
            "tree": { "algorithm": "blob_tree_blake3_nfc" }
        });
        std::fs::write(dir.path().join("spore.core.json"), core.to_string()).unwrap();
        let index = refs_dir.join("bonds.json");
        std::fs::write(&index, "{").unwrap();
        for (clean, status) in [(false, false), (true, false), (false, true)] {
            let error = bond_in_dir(dir.path(), clean, status, &crate::NoopSink)
                .await
                .err()
                .unwrap();
            assert_eq!(error.code, "bond_index_invalid");
            assert_eq!(std::fs::read_to_string(&index).unwrap(), "{");
            assert_eq!(std::fs::read_to_string(&marker).unwrap(), "keep");
        }
        std::fs::remove_file(&index).unwrap();
        std::fs::create_dir(&index).unwrap();
        for (clean, status) in [(false, false), (true, false), (false, true)] {
            let error = bond_in_dir(dir.path(), clean, status, &crate::NoopSink)
                .await
                .err()
                .unwrap();
            assert_eq!(error.code, "bond_read_failed");
            assert_eq!(std::fs::read_to_string(&marker).unwrap(), "keep");
        }
    }

    #[test]
    fn clean_retains_failed_entries_and_drops_only_successful_or_missing_entries() {
        let dir = tempfile::tempdir().unwrap();
        let refs_dir = dir.path().join(".cmn/bonds");
        for name in ["failed", "removed", "valid"] {
            std::fs::create_dir_all(refs_dir.join(name)).unwrap();
        }
        let index = [
            bond_index_entry("b3.failed", "failed"),
            bond_index_entry("b3.removed", "removed"),
            bond_index_entry("b3.missing", "missing"),
            bond_index_entry("b3.valid", "valid"),
        ];
        let error = clean_bond_dirs(
            &refs_dir,
            &index,
            &std::collections::HashSet::from(["valid".to_string()]),
            &crate::NoopSink,
            |path| {
                if path.file_name() == Some(std::ffi::OsStr::new("failed")) {
                    Err(std::io::Error::new(
                        std::io::ErrorKind::PermissionDenied,
                        "injected",
                    ))
                } else {
                    std::fs::remove_dir_all(path)
                }
            },
        )
        .unwrap_err();
        assert_eq!(error.code, "bond_clean_failed");
        assert!(refs_dir.join("failed").is_dir());
        assert!(!refs_dir.join("removed").exists());
        let saved = load_refs_json(&refs_dir.join("bonds.json")).unwrap();
        assert_eq!(
            saved
                .iter()
                .map(|entry| entry.dir.as_str())
                .collect::<Vec<_>>(),
            ["failed", "valid"]
        );
    }

    #[test]
    fn clean_does_not_report_missing_directory_as_cleaned() {
        let dir = tempfile::tempdir().unwrap();
        let refs_dir = dir.path().join(".cmn/bonds");
        let cleaned = clean_bond_dirs(
            &refs_dir,
            &[bond_index_entry("b3.missing", "missing")],
            &std::collections::HashSet::new(),
            &crate::NoopSink,
            |path| std::fs::remove_dir_all(path),
        )
        .unwrap();
        assert!(cleaned.is_empty());
        assert!(
            load_refs_json(&refs_dir.join("bonds.json"))
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn clean_and_index_writer_propagate_index_save_failure() {
        let dir = tempfile::tempdir().unwrap();
        let refs_dir = dir.path().join(".cmn/bonds");
        std::fs::create_dir_all(refs_dir.join("bonds.json")).unwrap();
        let error = clean_bond_dirs(
            &refs_dir,
            &[],
            &std::collections::HashSet::new(),
            &crate::NoopSink,
            |path| std::fs::remove_dir_all(path),
        )
        .unwrap_err();
        assert_eq!(error.code, "bond_write_failed");
        assert_eq!(
            write_refs_json(&refs_dir.join("bonds.json"), &[])
                .unwrap_err()
                .code,
            "bond_write_failed"
        );
    }

    #[test]
    fn resolve_bond_dir_name_falls_back_to_hash_when_derivation_is_empty() {
        let refs_dir = std::env::temp_dir().join("cmn-bond-dir-name-empty");
        let mut reserved_dirs = std::collections::HashSet::new();

        let dir_name =
            resolve_bond_dir_name(&refs_dir, &[], &mut reserved_dirs, Some(".."), "b3.hash");

        assert_eq!(dir_name, "b3.hash");
    }

    #[test]
    fn resolve_bond_dir_name_falls_back_to_hash_on_collision() {
        let refs_dir = std::env::temp_dir().join("cmn-bond-dir-name-collision");
        let mut reserved_dirs = std::collections::HashSet::from(["a-b".to_string()]);

        let dir_name =
            resolve_bond_dir_name(&refs_dir, &[], &mut reserved_dirs, Some("a/b"), "b3.hash");

        assert_eq!(dir_name, "b3.hash");
    }

    #[test]
    fn resolve_bond_dir_name_rejects_unsafe_persisted_path() {
        let refs_dir = tempfile::tempdir().unwrap();
        let index = [bond_index_entry("b3.hash", "../outside")];
        let mut reserved_dirs = std::collections::HashSet::new();

        let dir_name = resolve_bond_dir_name(
            refs_dir.path(),
            &index,
            &mut reserved_dirs,
            Some("İstanbul"),
            "b3.hash",
        );

        assert_eq!(dir_name, "i-stanbul");
    }

    #[test]
    fn resolve_bond_dir_name_rejects_reserved_persisted_path() {
        let refs_dir = tempfile::tempdir().unwrap();
        let index = [bond_index_entry("b3.hash", "legacy-dir")];
        let mut reserved_dirs = std::collections::HashSet::from(["legacy-dir".to_string()]);

        let dir_name = resolve_bond_dir_name(
            refs_dir.path(),
            &index,
            &mut reserved_dirs,
            Some("İstanbul"),
            "b3.hash",
        );

        assert_eq!(dir_name, "i-stanbul");
    }

    #[test]
    fn resolve_bond_dir_name_rejects_conflicting_persisted_path() {
        let refs_dir = tempfile::tempdir().unwrap();
        let index = [
            bond_index_entry("b3.hash", "legacy-dir"),
            bond_index_entry("b3.other", "legacy-dir"),
        ];
        let mut reserved_dirs = std::collections::HashSet::new();

        let dir_name = resolve_bond_dir_name(
            refs_dir.path(),
            &index,
            &mut reserved_dirs,
            Some("İstanbul"),
            "b3.hash",
        );

        assert_eq!(dir_name, "i-stanbul");
    }

    #[tokio::test]
    async fn bond_clean_preserves_persisted_unicode_path() {
        let work_dir = tempfile::tempdir().unwrap();
        let refs_dir = work_dir.path().join(".cmn/bonds");
        let old_dir_name = "i\u{0307}stanbul";
        let old_dir = refs_dir.join(old_dir_name);
        std::fs::create_dir_all(&old_dir).unwrap();
        std::fs::write(old_dir.join("marker"), "keep").unwrap();
        std::fs::create_dir_all(refs_dir.join("obsolete")).unwrap();
        let hash = "b3.3yMR7vZQ9hL";
        let index_path = refs_dir.join("bonds.json");
        write_refs_json(&index_path, &[bond_index_entry(hash, old_dir_name)]).unwrap();
        let core = json!({
            "name": "test",
            "domain": "example.com",
            "synopsis": "test",
            "intent": [],
            "license": "MIT",
            "tree": { "algorithm": "blob_tree_blake3_nfc" },
            "bonds": [{
                "uri": format!("cmn://example.com/{hash}"),
                "relation": "depends_on",
                "id": "İstanbul"
            }]
        });
        std::fs::write(
            work_dir.path().join("spore.core.json"),
            serde_json::to_vec(&core).unwrap(),
        )
        .unwrap();

        let result = bond_in_dir(work_dir.path(), true, false, &crate::NoopSink)
            .await
            .unwrap();

        assert!(matches!(
            result,
            crate::output::BondResult::Clean(output) if output.cleaned == ["obsolete"]
        ));
        assert_eq!(
            std::fs::read_to_string(old_dir.join("marker")).unwrap(),
            "keep"
        );
        let index = load_refs_json(&index_path).unwrap();
        assert_eq!(index.len(), 1);
        assert_eq!(index[0].dir, old_dir_name);
    }
}
