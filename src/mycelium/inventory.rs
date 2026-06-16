use serde::Serialize;
use serde_json::json;
use std::process::ExitCode;

use crate::api::Output;
use crate::auth;
use crate::site::{self, SiteDir};
use substrate::{
    build_mycelium_uri, build_spore_uri, CmnCapsuleEntry, CmnEntry, CmnUri, Mycelium, CMN_SCHEMA,
};

use super::format::format_mycelium;
use super::MyceliumError;

#[derive(Debug, Clone, Serialize)]
pub(crate) struct ResolvedSporeRef {
    pub id: String,
    pub hash: String,
    pub uri: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub synopsis: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mycelium_hash: Option<String>,
}

pub(crate) fn resolve_spore_ref(
    domain: &str,
    spore_id: &str,
    mycelium: &Mycelium,
    mycelium_hash: Option<&str>,
) -> Result<ResolvedSporeRef, crate::HyphaError> {
    if spore_id.trim().is_empty() {
        return Err(crate::HyphaError::new(
            "invalid_args",
            "--id must not be empty",
        ));
    }

    let spore = mycelium
        .capsule
        .core
        .spores
        .iter()
        .find(|entry| entry.id == spore_id)
        .ok_or_else(|| {
            crate::HyphaError::with_hint(
                "spore_not_found",
                format!("Spore id '{spore_id}' not found in mycelium for {domain}"),
                "publish the spore first or check the id in the domain's mycelium inventory",
            )
        })?;

    if spore.hash.trim().is_empty() {
        return Err(crate::HyphaError::new(
            "manifest_failed",
            format!("Spore id '{spore_id}' has an empty hash in mycelium for {domain}"),
        ));
    }

    let mycelium_hash = mycelium_hash
        .map(str::to_string)
        .or_else(|| {
            CmnUri::parse(&mycelium.capsule.uri)
                .ok()
                .and_then(|uri| uri.hash)
        })
        .filter(|hash| !hash.is_empty());

    Ok(ResolvedSporeRef {
        id: spore.id.clone(),
        hash: spore.hash.clone(),
        uri: build_spore_uri(domain, &spore.hash),
        name: spore.name.clone(),
        synopsis: spore.synopsis.clone(),
        mycelium_hash,
    })
}

pub(crate) fn load_local_mycelium(
    site: &SiteDir,
    domain: Option<&str>,
) -> Result<(Mycelium, String), crate::HyphaError> {
    let cmn_path = site.cmn_json_path();
    let cmn_content = std::fs::read_to_string(&cmn_path).map_err(|e| {
        crate::HyphaError::new(
            "manifest_failed",
            format!("Failed to read {}: {e}", cmn_path.display()),
        )
    })?;
    let entry: CmnEntry = serde_json::from_str(&cmn_content).map_err(|e| {
        crate::HyphaError::new(
            "manifest_failed",
            format!("Failed to parse {}: {e}", cmn_path.display()),
        )
    })?;

    let target_uri = domain.map(substrate::build_domain_uri);
    let capsule = target_uri
        .as_ref()
        .and_then(|target| entry.capsules.iter().find(|capsule| capsule.uri == *target))
        .or_else(|| entry.capsules.first())
        .ok_or_else(|| {
            crate::HyphaError::new(
                "manifest_failed",
                format!("No capsules found in {}", cmn_path.display()),
            )
        })?;
    let mycelium_hash = capsule
        .mycelium_hash()
        .ok_or_else(|| {
            crate::HyphaError::new(
                "manifest_failed",
                format!("No mycelium endpoint hash in {}", cmn_path.display()),
            )
        })?
        .to_string();

    let mycelium_path = site.mycelium_dir().join(format!("{mycelium_hash}.json"));
    let mycelium_content = std::fs::read_to_string(&mycelium_path).map_err(|e| {
        crate::HyphaError::new(
            "manifest_failed",
            format!("Failed to read {}: {e}", mycelium_path.display()),
        )
    })?;
    let mycelium = serde_json::from_str(&mycelium_content).map_err(|e| {
        crate::HyphaError::new(
            "manifest_failed",
            format!("Failed to parse {}: {e}", mycelium_path.display()),
        )
    })?;

    Ok((mycelium, mycelium_hash))
}

pub(crate) fn find_local_spore_hash(
    site: &SiteDir,
    domain: &str,
    spore_id: &str,
) -> Option<String> {
    let (mycelium, mycelium_hash) = load_local_mycelium(site, Some(domain)).ok()?;
    let domain = mycelium.capsule.core.domain.as_str();
    resolve_spore_ref(domain, spore_id, &mycelium, Some(&mycelium_hash))
        .ok()
        .map(|resolved| resolved.hash)
}

fn resolve_local_spore_by_id(
    site: &SiteDir,
    domain: &str,
    spore_id: &str,
) -> Result<(ResolvedSporeRef, serde_json::Value), crate::HyphaError> {
    let (mycelium, mycelium_hash) = load_local_mycelium(site, Some(domain))?;
    let resolved = resolve_spore_ref(domain, spore_id, &mycelium, Some(&mycelium_hash))?;

    let spore_path = site.spores_dir().join(format!("{}.json", resolved.hash));
    let spore_content = std::fs::read_to_string(&spore_path).map_err(|e| {
        crate::HyphaError::new(
            "manifest_failed",
            format!("Failed to read {}: {e}", spore_path.display()),
        )
    })?;
    let spore: serde_json::Value = serde_json::from_str(&spore_content).map_err(|e| {
        crate::HyphaError::new(
            "manifest_failed",
            format!("Failed to parse {}: {e}", spore_path.display()),
        )
    })?;
    substrate::validate_schema(&spore).map_err(|e| {
        crate::HyphaError::new(
            "schema_error",
            format!(
                "Spore schema validation failed for {}: {e}",
                spore_path.display()
            ),
        )
    })?;

    Ok((resolved, spore))
}

pub fn handle_status(
    out: &Output,
    domain: Option<&str>,
    site_path: Option<&str>,
    spore_id: Option<&str>,
) -> ExitCode {
    if spore_id.is_some() && domain.is_none() {
        return out.error("invalid_args", "--id requires a domain");
    }

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

        if let Some(spore_id) = spore_id {
            return match resolve_local_spore_by_id(&site, domain, spore_id) {
                Ok((resolved, spore)) => out.ok_trace(
                    json!({ "spore": spore }),
                    json!({
                        "source": "local_mycelium",
                        "site_path": site.root.display().to_string(),
                        "resolved": resolved,
                    }),
                ),
                Err(e) => out.error_hypha(&e),
            };
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
    } else if spore_id.is_some() {
        out.error("invalid_args", "--id requires a domain")
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
        anyhow::bail!(
            "Endpoints not configured. Run 'hypha mycelium root --endpoints-base URL' or edit cmn.json"
        );
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
    let core_signature = auth::sign_json_with_site(site, &mycelium.capsule.core)?;
    mycelium.capsule.core_signature = core_signature.clone();

    // 2. Calculate hash of core + core_signature
    let content_hash = mycelium
        .computed_uri_hash()
        .map_err(|e| MyceliumError::Jcs(e.to_string()))?;

    // 3. Set URI with hash
    mycelium.capsule.uri = build_mycelium_uri(domain, &content_hash);

    // 4. Sign capsule → capsule_signature
    let capsule_signature = auth::sign_json_with_site(site, &mycelium.capsule)?;
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
        serial: first_capsule.serial.saturating_add(1),
        key: identity.public_key,
        history: first_capsule.history.clone(),
        endpoints,
    }];

    let entry_signature = auth::sign_json_with_site(site, &capsules)?;

    let entry = CmnEntry {
        schema: CMN_SCHEMA.to_string(),
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

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn resolve_spore_ref_matches_id_and_builds_uri() {
        let mut mycelium = Mycelium::new("example.com", "Example", "", 1);
        let mycelium_hash = "b3.3yMR7vZQ9hL2xKJdFtN8wPcB6sY1mXgU4eH5pTa2";
        mycelium.capsule.uri = build_mycelium_uri("example.com", mycelium_hash);
        mycelium.add_spore("my-lib", "b3.spore", "My Lib", Some("A library"), 2);

        let resolved = resolve_spore_ref("example.com", "my-lib", &mycelium, None).unwrap();

        assert_eq!(resolved.id, "my-lib");
        assert_eq!(resolved.hash, "b3.spore");
        assert_eq!(resolved.uri, "cmn://example.com/b3.spore");
        assert_eq!(resolved.mycelium_hash.as_deref(), Some(mycelium_hash));
    }

    #[test]
    fn resolve_spore_ref_requires_id_match() {
        let mut mycelium = Mycelium::new("example.com", "Example", "", 1);
        mycelium.add_spore("", "b3.legacy", "legacy-lib", None, 2);

        let err = resolve_spore_ref("example.com", "legacy-lib", &mycelium, Some("b3.mycelium"))
            .unwrap_err();

        assert_eq!(err.code, "spore_not_found");
    }

    #[test]
    fn resolve_spore_ref_reports_missing_id() {
        let mycelium = Mycelium::new("example.com", "Example", "", 1);

        let err = resolve_spore_ref("example.com", "missing", &mycelium, None).unwrap_err();

        assert_eq!(err.code, "spore_not_found");
    }
}
