use serde_json::json;
use std::process::ExitCode;

use crate::api::Output;
use crate::auth;
use crate::site::{self, SiteDir};
use substrate::{build_mycelium_uri, CmnCapsuleEntry, CmnEntry, Mycelium, CMN_SCHEMA};

use super::format::format_mycelium;
use super::MyceliumError;

pub fn handle_status(out: &Output, domain: Option<&str>, site_path: Option<&str>) -> ExitCode {
    if let Some(domain) = domain {
        if site_path.is_none() {
            if let Err(e) = site::validate_site_domain_path(domain) {
                return out.error_hypha(&e);
            }
        }

        let site = SiteDir::from_args(domain, site_path);
        if !site.exists() {
            return out.error(
                "NO_SITE",
                &format!("Site not found at {}", site.root.display()),
            );
        }

        match auth::get_identity_with_site(domain, &site) {
            Ok(info) => {
                let spore_count = std::fs::read_dir(site.spores_dir())
                    .map(|entries| entries.filter_map(|e| e.ok()).count())
                    .unwrap_or(0);

                let data = json!({
                    "domain": domain,
                    "public_key": info.public_key,
                    "site_path": site.root.display().to_string(),
                    "spore_count": spore_count,
                });

                out.ok(data)
            }
            Err(e) => out.error_from("status_error", &e),
        }
    } else if site_path.is_some() {
        out.error(
            "invalid_args",
            "--domain is required when using --site-path",
        )
    } else {
        let domains = site::list_domains();

        if domains.is_empty() {
            let data = json!({
                "domains": [],
                "message": "No sites found"
            });
            out.ok(data)
        } else {
            let mut sites_info = Vec::new();
            for domain in &domains {
                let site = SiteDir::new(domain);
                let spore_count = std::fs::read_dir(site.spores_dir())
                    .map(|entries| entries.filter_map(|e| e.ok()).count())
                    .unwrap_or(0);
                sites_info.push(json!({
                    "domain": domain,
                    "spore_count": spore_count,
                }));
            }

            let data = json!({ "domains": sites_info });

            out.ok(data)
        }
    }
}

pub fn update_inventory(
    site: &SiteDir,
    domain: &str,
    spore_id: &str,
    spore_hash: &str,
    name: &str,
    synopsis: Option<&str>,
    now_epoch_ms: u64,
) -> anyhow::Result<()> {
    let manifest_path = site.cmn_json_path();

    // Get public key for HTTPS-based identity verification
    let identity = auth::get_identity_with_site(domain, site)?;

    // Read existing cmn.json to extract endpoints
    let cmn_content = if manifest_path.exists() {
        std::fs::read_to_string(&manifest_path)?
    } else {
        anyhow::bail!("Endpoints not configured. Run 'hypha mycelium root --endpoints-base URL' or edit cmn.json");
    };

    let existing_entry = serde_json::from_str::<CmnEntry>(&cmn_content)
        .map_err(|_| anyhow::anyhow!("Endpoints not configured. Run 'hypha mycelium root --endpoints-base URL' or edit cmn.json"))?;

    let first_capsule = existing_entry
        .capsules
        .first()
        .ok_or_else(|| anyhow::anyhow!("No capsules in cmn.json"))?;
    let mut endpoints = first_capsule.endpoints.clone();

    // Load existing mycelium or create new one
    let mut mycelium: Mycelium = {
        let first_hash = first_capsule
            .mycelium_hash()
            .ok_or_else(|| anyhow::anyhow!("No mycelium hash in cmn.json endpoints"))?;
        let filename = format!("{}.json", first_hash);
        let mycelium_path = site.mycelium_dir().join(filename);

        if mycelium_path.exists() {
            let mycelium_content = std::fs::read_to_string(&mycelium_path)?;
            serde_json::from_str(&mycelium_content)?
        } else {
            Mycelium::new(domain, domain, "", now_epoch_ms)
        }
    };

    mycelium.add_spore(spore_id, spore_hash, name, synopsis, now_epoch_ms);

    // 1. Sign core → core_signature
    let core_signature = match auth::sign_json_with_site(site, &mycelium.capsule.core) {
        Ok(sig) => sig,
        Err(auth::JsonSignError::Jcs(message)) => return Err(anyhow::anyhow!(message)),
        Err(auth::JsonSignError::Sign(err)) => return Err(err),
    };
    mycelium.capsule.core_signature = core_signature.clone();

    // 2. Calculate hash of core + core_signature
    let content_hash = mycelium
        .computed_uri_hash()
        .map_err(|e| MyceliumError::Jcs(e.to_string()))?;

    // 3. Set URI with hash
    mycelium.capsule.uri = build_mycelium_uri(domain, &content_hash);

    // 4. Sign capsule → capsule_signature
    let capsule_signature = match auth::sign_json_with_site(site, &mycelium.capsule) {
        Ok(sig) => sig,
        Err(auth::JsonSignError::Jcs(message)) => return Err(anyhow::anyhow!(message)),
        Err(auth::JsonSignError::Sign(err)) => return Err(err),
    };
    mycelium.capsule_signature = capsule_signature;

    // Validate mycelium against schema before writing
    let mycelium_value = serde_json::to_value(&mycelium)?;
    substrate::validate_schema(&mycelium_value)
        .map_err(|e| anyhow::anyhow!("Mycelium schema validation failed: {}", e))?;

    // Save full mycelium file to /cmn/mycelium/{hash}.json
    let mycelium_dir = site.mycelium_dir();
    std::fs::create_dir_all(&mycelium_dir)?;
    let filename = content_hash.clone();
    let full_mycelium_path = mycelium_dir.join(format!("{}.json", filename));
    let manifest_json = format_mycelium(&mycelium_value).map_err(|e| anyhow::anyhow!("{}", e))?;
    std::fs::write(&full_mycelium_path, &manifest_json)?;

    // Clean up old mycelium files — keep only previous and current
    let previous_hash = existing_entry
        .capsules
        .first()
        .and_then(|c| c.mycelium_hash().map(|s| s.to_string()))
        .unwrap_or_default();
    if let Ok(entries) = std::fs::read_dir(&mycelium_dir) {
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name_str = name.to_string_lossy();
            if name_str.ends_with(".json") {
                let stem = name_str.trim_end_matches(".json");
                if stem != content_hash && stem != previous_hash {
                    let _ = std::fs::remove_file(entry.path());
                }
            }
        }
    }

    for endpoint in &mut endpoints {
        if endpoint.kind == "mycelium" {
            endpoint.hash = content_hash.clone();
        }
    }

    // Generate and save cmn.json entry (preserving endpoints from existing cmn.json)
    let capsules = vec![CmnCapsuleEntry {
        uri: substrate::build_domain_uri(domain),
        key: identity.public_key,
        previous_keys: vec![],
        endpoints,
    }];

    let entry_signature = match auth::sign_json_with_site(site, &capsules) {
        Ok(sig) => sig,
        Err(auth::JsonSignError::Jcs(message)) => return Err(anyhow::anyhow!(message)),
        Err(auth::JsonSignError::Sign(err)) => return Err(err),
    };

    let entry = CmnEntry {
        schema: CMN_SCHEMA.to_string(),
        protocol_versions: vec!["v1".to_string()],
        capsules,
        capsule_signature: entry_signature,
    };

    // Validate cmn.json against schema before writing
    let entry_value = serde_json::to_value(&entry)?;
    substrate::validate_schema(&entry_value)
        .map_err(|e| anyhow::anyhow!("CMN schema validation failed: {}", e))?;

    let entry_json = entry.to_pretty_json_deep()?;
    if let Some(parent) = manifest_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&manifest_path, entry_json)?;

    Ok(())
}

pub(crate) fn resolve_public_file_path(
    public_dir: &std::path::Path,
    request_url: &str,
) -> Option<std::path::PathBuf> {
    let path_only = request_url.split('?').next().unwrap_or_default();
    let trimmed = path_only.trim_start_matches('/');

    let mut rel = std::path::PathBuf::new();
    for component in std::path::Path::new(trimmed).components() {
        match component {
            std::path::Component::Normal(part) => rel.push(part),
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir
            | std::path::Component::RootDir
            | std::path::Component::Prefix(_) => return None,
        }
    }

    if rel.as_os_str().is_empty() {
        Some(public_dir.join("index.html"))
    } else {
        Some(public_dir.join(rel))
    }
}
