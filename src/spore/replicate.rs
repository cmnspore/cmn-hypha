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

    let cache = CacheDir::new();
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
        let entry = match visitor::get_cmn_entry(
            &crate::api::OutSink(out),
            &domain_cache,
            cache.cmn_ttl_ms,
        )
        .await
        {
            Ok(p) => p,
            Err(e) => return out.error_hypha(&e),
        };

        let capsule = match entry.primary_capsule() {
            Ok(c) => c,
            Err(e) => return out.error("cmn_invalid", &e.to_string()),
        };
        let public_key = capsule.key.clone();
        let ep = &capsule.endpoints;

        let manifest = match visitor::fetch_spore_manifest(capsule, &hash).await {
            Ok(m) => m,
            Err(e) => {
                return out.error(
                    "manifest_failed",
                    &format!("Failed to fetch spore {}: {}", hash, e),
                )
            }
        };

        let source_spore = match visitor::decode_spore_manifest(&manifest) {
            Ok(spore) => spore,
            Err(e) => return out.error_hypha(&e),
        };

        let author_key = visitor::embedded_spore_author_key(&manifest);
        let ak = author_key.as_deref().unwrap_or(&public_key);

        if let Err(e) = visitor::verify_manifest_two_key_signatures(&manifest, &public_key, ak) {
            return out.error(
                "sig_failed",
                &format!("Signature verification failed for {}: {}", hash, e),
            );
        }

        // Download source archive to target site
        let source_dist_array = source_spore.distributions();
        if source_dist_array.is_empty() {
            return out.error(
                "manifest_failed",
                &format!("No distribution options for {}", hash),
            );
        }

        // Download archive to target site's archive dir
        let archive_dir = site.archive_dir();
        if let Err(e) = std::fs::create_dir_all(&archive_dir) {
            return out.error("dir_error", &format!("Failed to create archive dir: {}", e));
        }

        let mut new_dist: Vec<substrate::SporeDist> = vec![];
        let mut downloaded = false;

        for dist_entry in source_dist_array {
            if dist_entry.is_archive() {
                let archive_filename = format!("{}.tar.zst", hash);
                let target_archive_path = archive_dir.join(&archive_filename);
                let mut archive_downloaded = false;
                for archive_ep in ep.iter().filter(|endpoint| endpoint.kind == "archive") {
                    let archive_url = match archive_ep.resolve_url(&hash) {
                        Ok(url) => url,
                        Err(e) => {
                            out.warn(
                                "URL_ERROR",
                                &format!(
                                    "Invalid archive URL for format {:?}: {}",
                                    archive_ep.format, e
                                ),
                            );
                            continue;
                        }
                    };

                    match download_file_to_path(&archive_url, &target_archive_path).await {
                        Ok(_) => {
                            new_dist.push(substrate::SporeDist {
                                kind: substrate::DistKind::Archive,
                                filename: None,
                                url: None,
                                git_ref: None,
                                cid: None,
                                extra: Default::default(),
                            });
                            archive_downloaded = true;
                            downloaded = true;
                            break;
                        }
                        Err(e) => {
                            out.warn(
                                "DOWNLOAD_FAILED",
                                &format!("Failed to download archive {}: {}", archive_url, e),
                            );
                        }
                    }
                }
                if archive_downloaded {
                    break;
                }
            }
        }

        if !downloaded {
            return out.error(
                "fetch_failed",
                &format!("Failed to download archive for {}", hash),
            );
        }

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

/// Download a file to a specific path
async fn download_file_to_path(
    url: &str,
    dest: &std::path::Path,
) -> Result<(), crate::sink::HyphaError> {
    use crate::sink::HyphaError;
    let max_download_bytes = crate::cache::CacheDir::new().max_download_bytes;

    let client = substrate::client::http_client(300).map_err(|e| {
        HyphaError::new(
            "fetch_failed",
            format!("Failed to create HTTP client: {}", e),
        )
    })?;

    let response = client
        .get(url)
        .send()
        .await
        .map_err(|e| HyphaError::new("fetch_failed", format!("Failed to download: {}", e)))?;

    if !response.status().is_success() {
        return Err(HyphaError::new(
            "fetch_failed",
            format!("HTTP {}", response.status()),
        ));
    }

    if let Some(cl) = response.content_length() {
        if cl > max_download_bytes {
            return Err(HyphaError::new(
                "fetch_failed",
                format!(
                    "Response too large: {} bytes exceeds max_download_bytes ({})",
                    cl, max_download_bytes
                ),
            ));
        }
    }

    let bytes = response
        .bytes()
        .await
        .map_err(|e| HyphaError::new("fetch_failed", format!("Failed to read response: {}", e)))?;
    if bytes.len() as u64 > max_download_bytes {
        return Err(HyphaError::new(
            "fetch_failed",
            format!(
                "Download exceeds max_download_bytes ({})",
                max_download_bytes
            ),
        ));
    }

    let dest = dest.to_path_buf();
    tokio::task::spawn_blocking(move || {
        use std::io::Write;
        let mut out = std::fs::File::create(&dest)
            .map_err(|e| HyphaError::new("write_error", format!("Failed to create file: {}", e)))?;
        out.write_all(&bytes)
            .map_err(|e| HyphaError::new("write_error", format!("Failed to write file: {}", e)))?;
        out.sync_all()
            .map_err(|e| HyphaError::new("write_error", format!("Failed to sync file: {}", e)))?;
        Ok::<(), HyphaError>(())
    })
    .await
    .map_err(|e| HyphaError::new("write_error", format!("Write task failed: {}", e)))??;

    Ok(())
}
