use serde_json::json;
use std::process::ExitCode;

use crate::api::Output;
use crate::auth;
use crate::cache::CacheDir;
use crate::site::{self, SiteDir};
use crate::visitor;
use substrate::{CmnUri, PrettyJson, Spore, SporeCapsule, SporeCore, SPORE_SCHEMA};

/// Handle the `replicate` command — copy spores to your domain (same hash, re-signed capsule)
#[allow(clippy::too_many_arguments)]
pub async fn handle_replicate(
    out: &Output,
    uris: Vec<String>,
    refs: bool,
    domain: &str,
    site_path: Option<&str>,
) -> ExitCode {
    let now_epoch_ms = crate::time::now_epoch_ms();

    if site_path.is_none() {
        if let Err(e) = site::validate_site_domain_path(domain) {
            return out.error_hypha(&e);
        }
    }

    let site = SiteDir::from_args(domain, site_path);
    if !site.exists() {
        return out.error_hint(
            "NO_SITE",
            &format!("Site not found at {}", site.root.display()),
            Some(&format!("run: hypha mycelium root {}", domain)),
        );
    }

    // Determine URIs to replicate
    let uris_to_replicate = if refs {
        // Read spore.core.json and collect non-self bonds
        let spore_core_path = std::path::Path::new("spore.core.json");
        if !spore_core_path.exists() {
            return out.error_hint(
                "REPLICATE_ERR",
                "spore.core.json not found",
                Some("run from a spore directory, or provide URIs"),
            );
        }
        let content = match std::fs::read_to_string(spore_core_path) {
            Ok(c) => c,
            Err(e) => {
                return out.error(
                    "REPLICATE_ERR",
                    &format!("Failed to read spore.core.json: {}", e),
                )
            }
        };
        let core: SporeCore = match serde_json::from_str(&content) {
            Ok(v) => v,
            Err(e) => {
                return out.error(
                    "REPLICATE_ERR",
                    &format!("Failed to parse spore.core.json: {}", e),
                )
            }
        };
        let collected: Vec<String> = core
            .bonds
            .iter()
            .filter_map(|r| {
                let uri_str = r.uri.as_str();
                let uri = CmnUri::parse(uri_str).ok()?;
                // Skip bonds already on the target domain
                if uri.domain == domain {
                    return None;
                }
                uri.hash.as_ref()?;
                Some(uri_str.to_string())
            })
            .collect();
        if collected.is_empty() {
            return out.ok(json!({
                "replicated": serde_json::Value::Array(vec![]),
                "message": "No non-self bonds to replicate"
            }));
        }
        collected
    } else {
        uris
    };

    let cache = match CacheDir::new() {
        Ok(cache) => cache,
        Err(e) => return out.error_hypha(&e),
    };
    let mut replicated = Vec::new();

    for uri_str in &uris_to_replicate {
        let uri = match CmnUri::parse(uri_str) {
            Ok(u) => u,
            Err(e) => return out.error("invalid_uri", &e),
        };

        let hash = match &uri.hash {
            Some(h) => h.clone(),
            None => return out.error("invalid_uri", "spore URI must include a hash"),
        };

        // Check if spore already exists on target site
        let target_manifest_path = site.spores_dir().join(format!("{}.json", hash));
        if target_manifest_path.exists() {
            replicated.push(json!({
                "uri": uri_str,
                "hash": hash,
                "status": "already_exists",
            }));
            continue;
        }

        // Check taste verdict
        if let Err(exit) = crate::visitor::check_taste_verdict_for_replicate(
            out,
            &cache,
            uri_str,
            &uri.domain,
            &hash,
        ) {
            return exit;
        }

        let domain_cache = cache.domain(&uri.domain);

        // Resolve source: cmn.json → manifest → verify
        let sink = crate::api::OutSink(out);
        let entry = match visitor::get_cmn_entry(&sink, &domain_cache, cache.cmn_ttl_ms).await {
            Ok(p) => p,
            Err(e) => return out.error_hypha(&e),
        };

        let capsule = match entry.primary_capsule() {
            Ok(c) => c,
            Err(e) => return out.error("cmn_invalid", &e.to_string()),
        };

        let (_manifest, source_spore) = match visitor::fetch_verified_spore(
            &sink,
            capsule,
            &hash,
            &domain_cache,
            cache.cmn_ttl_ms,
        )
        .await
        {
            Ok(result) => result,
            Err(e) => return out.error_hypha(&e),
        };

        // Source must offer an archive distribution to re-publish.
        let source_dist_array = source_spore.distributions();
        if source_dist_array.is_empty() {
            return out.error(
                "manifest_failed",
                &format!("No distribution options for {}", hash),
            );
        }
        if !source_dist_array.iter().any(|d| d.is_archive()) {
            return out.error(
                "replicate_err",
                &format!(
                    "Spore {} has no archive distribution; only archive-distributed spores can be replicated",
                    hash
                ),
            );
        }

        // Download AND content-verify the archive into the local cache. This
        // recomputes the content hash against `hash` (and marks the spore toxic
        // on mismatch), so we never re-publish unverified bytes under our key.
        if let Err(e) = visitor::fetch_spore_to_cache(&sink, &cache, uri_str).await {
            return out.error_hypha(&e);
        }

        // The verified compressed archive now lives in the cache; copy it into
        // the target site's archive dir via a temp file + atomic rename.
        let cached_archive = cache.spore_path(&uri.domain, &hash).join("archive.tar.zst");
        if !cached_archive.exists() {
            return out.error(
                "replicate_err",
                &format!("No verified archive available for {} after fetch", hash),
            );
        }

        let archive_dir = site.archive_dir();
        if let Err(e) = std::fs::create_dir_all(&archive_dir) {
            return out.error("dir_error", &format!("Failed to create archive dir: {}", e));
        }
        let target_archive_path = archive_dir.join(format!("{}.tar.zst", hash));
        if let Err(e) = copy_file_atomic(&cached_archive, &target_archive_path) {
            return out.error_hypha(&e);
        }

        let new_dist: Vec<substrate::SporeDist> = vec![substrate::SporeDist {
            kind: substrate::DistKind::Archive,
            filename: None,
            url: None,
            git_ref: None,
            cid: None,
            extra: Default::default(),
        }];

        // Build new capsule: same core + core_signature, new dist, re-signed
        let new_capsule = SporeCapsule {
            uri: format!("cmn://{}/{}", domain, hash),
            core: source_spore.capsule.core.clone(),
            core_signature: source_spore.capsule.core_signature.clone(),
            dist: new_dist,
        };

        // Sign the new capsule
        let capsule_signature = match auth::sign_json_with_site(&site, &new_capsule) {
            Ok(sig) => sig,
            Err(auth::JsonSignError::Jcs(message)) => return out.error("jcs_error", &message),
            Err(auth::JsonSignError::Sign(err)) => return out.error_from("sign_error", &err),
        };

        // Build complete spore manifest
        let new_manifest = Spore {
            schema: SPORE_SCHEMA.to_string(),
            capsule: new_capsule,
            capsule_signature,
        };

        // Write spore manifest to target site
        let spores_dir = site.spores_dir();
        if let Err(e) = std::fs::create_dir_all(&spores_dir) {
            return out.error("dir_error", &format!("Failed to create spores dir: {}", e));
        }

        let manifest_json = match new_manifest.to_pretty_json() {
            Ok(j) => j,
            Err(e) => {
                return out.error(
                    "serialize_error",
                    &format!("Failed to format spore manifest: {}", e),
                )
            }
        };

        if let Err(e) = std::fs::write(&target_manifest_path, &manifest_json) {
            return out.error(
                "write_error",
                &format!("Failed to write spore manifest: {}", e),
            );
        }

        // Update target domain's mycelium inventory
        let spore_id = if source_spore.capsule.core.id.is_empty() {
            "unknown"
        } else {
            source_spore.capsule.core.id.as_str()
        };
        let spore_name = source_spore.capsule.core.name.as_str();
        let spore_synopsis = Some(source_spore.capsule.core.synopsis.as_str());

        if let Err(e) = crate::mycelium::update_inventory(
            &site,
            domain,
            spore_id,
            &hash,
            spore_name,
            spore_synopsis,
            now_epoch_ms,
        ) {
            return out.error(
                "INVENTORY_ERR",
                &format!("Failed to update inventory: {}", e),
            );
        }

        replicated.push(json!({
            "uri": format!("cmn://{}/{}", domain, hash),
            "source_uri": uri_str,
            "hash": hash,
            "status": "replicated",
            "original_domain": source_spore.capsule.core.domain,
        }));
    }

    out.ok(json!({ "replicated": replicated }))
}

/// Copy a file via a temp file + atomic rename so an interrupted copy never
/// leaves a truncated archive published in the site's public dir.
fn copy_file_atomic(
    src: &std::path::Path,
    dest: &std::path::Path,
) -> Result<(), crate::sink::HyphaError> {
    use crate::sink::HyphaError;

    let parent = dest.parent().ok_or_else(|| {
        HyphaError::new("write_error", "Cannot determine archive parent directory")
    })?;
    let tmp = tempfile::NamedTempFile::new_in(parent).map_err(|e| {
        HyphaError::new(
            "write_error",
            format!("Failed to create temp archive: {}", e),
        )
    })?;
    std::fs::copy(src, tmp.path())
        .map_err(|e| HyphaError::new("write_error", format!("Failed to copy archive: {}", e)))?;
    tmp.persist(dest)
        .map_err(|e| HyphaError::new("write_error", format!("Failed to publish archive: {}", e)))?;
    Ok(())
}
