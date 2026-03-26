use super::*;

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
pub(super) struct BondIndexEntry {
    hash: String,
    dir: String,
    uri: String,
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

/// Handle the `bond fetch` command — fetch all bonds from spore.core.json to .cmn/bonds/
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
            )
        }
    };
    let sink = crate::api::OutSink(out);
    match bond_in_dir(&cwd, clean, status, &sink).await {
        Ok(output) => out.ok(serde_json::to_value(output).unwrap_or_default()),
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
    let index = load_refs_json(&refs_json_path);

    // --status
    if status {
        let mut statuses = Vec::new();
        for (uri_str, _domain, hash, relation, _id) in &spore_refs {
            let bonded = index.iter().any(|entry| entry.hash == *hash);
            statuses.push(crate::output::BondStatusRef {
                uri: uri_str.clone(),
                relation: relation.clone(),
                bonded: json!(bonded),
            });
        }
        for reference in &core.bonds {
            let relation = reference.relation.clone();
            if relation.is_excluded_from_bond_fetch() {
                statuses.push(crate::output::BondStatusRef {
                    uri: reference.uri.clone(),
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
        let mut removed = Vec::new();
        let mut kept = Vec::new();
        for entry in &index {
            let dir_name = index_dir_name(entry);
            if !is_safe_bond_dir_name(dir_name) {
                sink.emit(crate::HyphaEvent::Warn {
                    message: format!(
                        "Skipping unsafe bond directory in bonds.json: '{}'",
                        dir_name
                    ),
                });
                continue;
            }
            if valid_dirs.contains(dir_name) {
                kept.push(entry.clone());
            } else {
                let ref_dir = refs_dir.join(dir_name);
                if ref_dir.is_dir() {
                    let _ = std::fs::remove_dir_all(&ref_dir);
                }
                removed.push(dir_name.to_string());
            }
        }
        if refs_dir.exists() {
            if let Ok(entries) = std::fs::read_dir(&refs_dir) {
                for fs_entry in entries.flatten() {
                    let path = fs_entry.path();
                    if !path.is_dir() {
                        continue;
                    }
                    let dir_name = fs_entry.file_name().to_string_lossy().to_string();
                    if !is_safe_bond_dir_name(&dir_name) {
                        continue;
                    }
                    if !valid_dirs.contains(&dir_name)
                        && std::fs::remove_dir_all(&path).is_ok()
                        && !removed.contains(&dir_name)
                    {
                        removed.push(dir_name);
                    }
                }
            }
        }
        let _ = write_refs_json(&refs_json_path, &kept);
        return Ok(crate::output::BondResult::Clean(
            crate::output::BondCleanOutput { cleaned: removed },
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
    let cache = CacheDir::new();
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
                uri: uri_str.clone(),
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
                uri: uri_str.clone(),
                relation: relation.clone(),
                id: id.clone(),
                name: None,
            });
            bonded.push(crate::output::BondedRef {
                uri: uri_str.clone(),
                relation: relation.clone(),
                status: "already_bonded".to_string(),
            });
            continue;
        }

        let domain_cache = cache.domain(domain);

        let entry = get_cmn_entry(sink, &domain_cache, cache.cmn_ttl_ms).await?;

        let capsule = primary_capsule(&entry)?;
        let public_key = capsule.key.clone();
        let ep = &capsule.endpoints;

        let manifest = fetch_spore_manifest(capsule, hash).await.map_err(|e| {
            crate::HyphaError::new(
                "manifest_failed",
                format!("Failed to fetch spore {}: {}", hash, e),
            )
        })?;
        let spore = decode_spore_manifest(&manifest)?;

        let author_key = embedded_spore_author_key(&manifest);
        let ak = author_key.as_deref().unwrap_or(&public_key);

        verify_manifest_two_key_signatures(&manifest, &public_key, ak).map_err(|e| {
            crate::HyphaError::new(
                "sig_failed",
                format!("Signature verification failed for {}: {}", hash, e),
            )
        })?;

        std::fs::create_dir_all(&content_dir).map_err(|e| {
            crate::HyphaError::new(
                "bond_error",
                format!("Failed to create {}: {}", content_dir.display(), e),
            )
        })?;

        let manifest_path = ref_dir.join("spore.json");
        std::fs::write(
            &manifest_path,
            serde_json::to_string_pretty(&spore).unwrap_or_default(),
        )
        .map_err(|e| {
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
                    let archive_url = build_archive_url_from_endpoint(archive_ep, hash)
                        .map_err(|e| crate::HyphaError::new("url_error", e))?;

                    if content_dir.exists() {
                        std::fs::remove_dir_all(&content_dir).map_err(|e| {
                            crate::HyphaError::new(
                                "bond_error",
                                format!("Failed to reset content dir: {}", e),
                            )
                        })?;
                    }
                    std::fs::create_dir_all(&content_dir).map_err(|e| {
                        crate::HyphaError::new(
                            "bond_error",
                            format!("Failed to recreate content dir: {}", e),
                        )
                    })?;

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
                        Err(e) => {
                            sink.emit(crate::HyphaEvent::Warn {
                                message: format!("Failed to download from {}: {}", archive_url, e),
                            });
                        }
                    }
                }
                if downloaded {
                    break;
                }
            } else if let Some(git_url) = dist_git_url(dist_entry) {
                let git_ref = dist_git_ref(dist_entry);
                if content_dir.exists() {
                    std::fs::remove_dir_all(&content_dir).map_err(|e| {
                        crate::HyphaError::new(
                            "bond_error",
                            format!("Failed to reset content dir: {}", e),
                        )
                    })?;
                }
                std::fs::create_dir_all(&content_dir).map_err(|e| {
                    crate::HyphaError::new(
                        "bond_error",
                        format!("Failed to recreate content dir: {}", e),
                    )
                })?;

                match clone_git_to_dir(git_url, git_ref, &content_dir).await {
                    Ok(_) => {
                        downloaded = true;
                        break;
                    }
                    Err(e) => {
                        sink.emit(crate::HyphaEvent::Warn {
                            message: format!("Failed to clone from {}: {}", git_url, e),
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

        let name = spore.capsule.core.name.as_str();

        index_entries.push(BondIndexEntry {
            hash: hash.clone(),
            dir: dir_name,
            uri: uri_str.clone(),
            relation: relation.clone(),
            id: id.clone(),
            name: Some(name.to_string()),
        });

        bonded.push(crate::output::BondedRef {
            uri: uri_str.clone(),
            relation: relation.clone(),
            status: "bonded".to_string(),
        });
    }

    if let Err(e) = write_refs_json(&refs_json_path, &index_entries) {
        sink.emit(crate::HyphaEvent::Warn {
            message: format!("Failed to write bonds.json: {}", e),
        });
    }

    Ok(crate::output::BondResult::Bond(crate::output::BondOutput {
        bonded,
        message: None,
    }))
}

/// Load bonds.json index, returns empty vec if missing/invalid
pub(super) fn load_refs_json(path: &std::path::Path) -> Vec<BondIndexEntry> {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str::<BondIndexFile>(&s).ok())
        .map(|index| index.bonds)
        .unwrap_or_default()
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
) -> Result<(), String> {
    let index = BondIndexFile {
        bonds: entries.to_vec(),
    };
    std::fs::write(
        path,
        serde_json::to_string_pretty(&index).unwrap_or_default(),
    )
    .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;

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
}
