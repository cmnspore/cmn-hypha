use super::*;
use substrate::client::BondNode;

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
    let new_hash =
        match find_latest_version(synapse_url, current_hash, domain, synapse_token, sink).await {
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
            Err(e) => return Err(e),
        };

    if new_hash == current_hash {
        return Ok(None);
    }

    let cache = CacheDir::new()?;
    let domain_cache = cache.domain(domain);
    let new_uri = format!("cmn://{}/{}", domain, new_hash);

    let entry = get_cmn_entry(sink, &domain_cache, cache.cmn_ttl_ms).await?;
    let capsule = primary_capsule(&entry)?;
    let _ = fetch_verified_spore(sink, capsule, &new_hash, &domain_cache, cache.cmn_ttl_ms).await?;

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
            // Match on (uri, relation) so duplicate URIs under different
            // relations rewrite the exact bond that was checked.
            if let Some(bond) = core
                .bonds
                .iter_mut()
                .find(|bond| bond.uri == upd.old_uri && bond.relation == upd.relation)
            {
                bond.uri = upd.new_uri.clone();
            }
        }
        let core_value = serde_json::to_value(&core).map_err(|e| {
            crate::HyphaError::new("write_error", format!("serialize error: {}", e))
        })?;
        crate::spore::write_spore_core(&spore_core_path, &core_value)?;
    }

    Ok(serde_json::json!({
        "updated": updated,
        "up_to_date": up_to_date,
    }))
}

/// Maximum number of lineage hops to follow when resolving the latest version.
/// Bounds runaway walks; reaching it is surfaced as a warning.
const MAX_LINEAGE_DEPTH: usize = 256;

pub(super) async fn find_latest_version(
    synapse_url: &str,
    current_hash: &str,
    source_domain: &str,
    token: Option<&str>,
    sink: &dyn crate::EventSink,
) -> Result<Option<BondNode>, crate::HyphaError> {
    let mut candidate_hash = current_hash.to_string();
    let mut latest: Option<BondNode> = None;
    // Track visited hashes so a cyclic lineage graph can't loop forever.
    let mut visited = std::collections::HashSet::new();
    visited.insert(candidate_hash.clone());

    let mut depth = 0;
    loop {
        if depth >= MAX_LINEAGE_DEPTH {
            sink.emit(crate::HyphaEvent::Warn {
                message: format!(
                    "Lineage walk hit the {}-hop limit; reported version may not be the newest",
                    MAX_LINEAGE_DEPTH
                ),
            });
            break;
        }
        depth += 1;

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

        // Cycle guard: stop if we've already seen this node.
        if !visited.insert(next_hash.clone()) {
            sink.emit(crate::HyphaEvent::Warn {
                message: format!("Lineage cycle detected at {}; stopping walk", next_hash),
            });
            break;
        }

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
