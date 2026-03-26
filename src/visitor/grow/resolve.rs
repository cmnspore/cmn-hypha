use super::*;

#[derive(serde::Serialize)]
struct BondUpdate {
    id: String,
    relation: substrate::BondRelation,
    old_uri: String,
    new_uri: String,
    old_hash: String,
    new_hash: String,
}

pub(super) async fn check_for_update(
    domain: &str,
    current_hash: &str,
    synapse_url: &str,
    synapse_token: Option<&str>,
    sink: &dyn crate::EventSink,
) -> Result<Option<(String, String)>, crate::HyphaError> {
    let new_hash = match find_latest_version(synapse_url, current_hash, domain, synapse_token).await
    {
        Ok(Some(node)) => {
            let parsed = CmnUri::parse(&node.uri).map_err(|e| {
                crate::HyphaError::new("lineage_error", format!("Invalid lineage URI: {}", e))
            })?;
            match parsed.hash {
                Some(h) => h,
                None => return Ok(None),
            }
        }
        Ok(None) => return Ok(None),
        Err(e) => return Err(crate::HyphaError::new("synapse_error", e)),
    };

    if new_hash == current_hash {
        return Ok(None);
    }

    let cache = CacheDir::new();
    let domain_cache = cache.domain(domain);
    let new_uri = format!("cmn://{}/{}", domain, new_hash);

    let entry = get_cmn_entry(sink, &domain_cache, cache.cmn_ttl_ms).await?;
    let capsule = primary_capsule(&entry)?;
    let public_key = capsule.key.clone();

    let new_manifest = fetch_spore_manifest(capsule, &new_hash)
        .await
        .map_err(|e| {
            crate::HyphaError::new("manifest_failed", format!("Failed to fetch spore: {}", e))
        })?;

    verify_manifest_two_key_signatures(&new_manifest, &public_key, &public_key).map_err(|e| {
        crate::HyphaError::new("sig_failed", format!("Spore signature invalid: {}", e))
    })?;

    check_taste(sink, &cache, &new_uri, domain, &new_hash)?;

    Ok(Some((new_uri, new_hash)))
}

pub(super) async fn update_bonds(
    dir: &std::path::Path,
    synapse_url: &str,
    synapse_token: Option<&str>,
    sink: &dyn crate::EventSink,
) -> Result<serde_json::Value, crate::HyphaError> {
    let spore_core_path = dir.join("spore.core.json");
    let content = std::fs::read_to_string(&spore_core_path).map_err(|e| {
        crate::HyphaError::new(
            "grow_error",
            format!("Failed to read spore.core.json: {}", e),
        )
    })?;
    let mut core: substrate::SporeCore = serde_json::from_str(&content).map_err(|e| {
        crate::HyphaError::new(
            "grow_error",
            format!("Failed to parse spore.core.json: {}", e),
        )
    })?;

    let mut updated: Vec<BondUpdate> = Vec::new();
    let mut up_to_date = 0u32;

    for bond in &core.bonds {
        let relation = bond.relation.clone();
        if !relation.participates_in_bond_updates() {
            continue;
        }

        let parsed = match CmnUri::parse(&bond.uri) {
            Ok(u) => u,
            Err(_) => continue,
        };
        let hash = match &parsed.hash {
            Some(h) => h.clone(),
            None => continue,
        };
        let id = bond.id.as_deref().unwrap_or(&hash);

        sink.emit(crate::HyphaEvent::Progress {
            current: 0,
            total: 0,
            message: format!("Checking {} ({})...", id, relation),
        });

        match check_for_update(&parsed.domain, &hash, synapse_url, synapse_token, sink).await {
            Ok(Some((new_uri, new_hash))) => {
                updated.push(BondUpdate {
                    id: id.to_string(),
                    relation,
                    old_uri: bond.uri.clone(),
                    new_uri,
                    old_hash: hash,
                    new_hash,
                });
            }
            Ok(None) => {
                up_to_date += 1;
            }
            Err(e) => {
                sink.emit(crate::HyphaEvent::Warn {
                    message: format!("Failed to check {}: {}", id, e),
                });
            }
        }
    }

    if !updated.is_empty() {
        for upd in &updated {
            if let Some(bond) = core.bonds.iter_mut().find(|bond| bond.uri == upd.old_uri) {
                bond.uri = upd.new_uri.clone();
            }
        }
        let core_value = serde_json::to_value(&core).map_err(|e| {
            crate::HyphaError::new("write_error", format!("serialize error: {}", e))
        })?;
        crate::spore::write_spore_core(&spore_core_path, &core_value)
            .map_err(|e| crate::HyphaError::new("write_error", e))?;
    }

    Ok(serde_json::json!({
        "updated": updated,
        "up_to_date": up_to_date,
    }))
}

pub(super) async fn find_latest_version(
    synapse_url: &str,
    current_hash: &str,
    source_domain: &str,
    token: Option<&str>,
) -> Result<Option<BondNode>, String> {
    let mut candidate_hash = current_hash.to_string();
    let mut latest: Option<BondNode> = None;

    for _depth in 0..20 {
        let bonds = fetch_bonds(synapse_url, &candidate_hash, "inbound", 1, token).await?;

        let same_domain: Vec<BondNode> = bonds
            .result
            .bonds
            .into_iter()
            .filter(|n| n.domain == source_domain)
            .collect();

        let next = match same_domain.into_iter().next() {
            Some(n) => n,
            None => break,
        };
        let next_uri = match CmnUri::parse(&next.uri) {
            Ok(u) => u,
            Err(_) => break,
        };
        let next_hash = match &next_uri.hash {
            Some(h) => h.clone(),
            None => break,
        };

        candidate_hash = next_hash;
        latest = Some(next);
    }

    Ok(latest)
}

#[cfg(test)]
pub(super) fn spawned_from_hash(manifest: &serde_json::Value) -> Option<String> {
    substrate::decode_spore(manifest)
        .ok()
        .and_then(|spore| spore.spawned_from_hash())
}
