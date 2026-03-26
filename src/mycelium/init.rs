use serde_json::json;
use std::process::ExitCode;

use crate::api::Output;
use crate::auth;
use crate::site::{self, SiteDir};
use substrate::{Mycelium, PrettyJson};

use super::{load_existing_mycelium, sign_and_save_mycelium, with_warning, InitArgs};

pub fn handle_init(out: &Output, args: InitArgs<'_>) -> ExitCode {
    let now_epoch_ms = crate::time::now_epoch_ms();

    let InitArgs {
        domain,
        hub,
        site_path,
        name,
        synopsis,
        bio,
        endpoints_base,
    } = args;

    // --hub mode: generate key first to a temp site, compute subdomain, then set up the real site
    if let Some(hub_domain) = hub {
        return handle_init_hub(out, hub_domain, site_path, name, synopsis, bio);
    }

    let domain = match domain {
        Some(d) => d,
        None => return out.error("missing_domain", "Domain is required (or use --hub)"),
    };

    if site_path.is_none() {
        if let Err(e) = site::validate_site_domain_path(domain) {
            return out.error("invalid_domain", &e);
        }
    }

    let site = SiteDir::from_args(domain, site_path);

    match auth::init_identity_with_site(domain, &site) {
        Ok(info) => {
            let cmn_path = site.cmn_json_path();
            if let Some(parent) = cmn_path.parent() {
                if let Err(e) = std::fs::create_dir_all(parent) {
                    return out.error(
                        "write_error",
                        &format!("Failed to create .well-known dir: {}", e),
                    );
                }
            }

            if let Some(base) = endpoints_base {
                let all_endpoints = SiteDir::endpoints(base);

                // Preserve existing mycelium if it exists on disk
                let mut mycelium = match load_existing_mycelium(&site) {
                    Some((m, _)) => m,
                    None => Mycelium::new(
                        domain,
                        name.unwrap_or(domain),
                        synopsis.unwrap_or(""),
                        now_epoch_ms,
                    ),
                };

                // Update fields if provided
                if let Some(n) = name {
                    mycelium.capsule.core.name = n.to_string();
                }
                if let Some(s) = synopsis {
                    mycelium.capsule.core.synopsis = s.to_string();
                }
                if let Some(b) = bio {
                    mycelium.capsule.core.bio = b.to_string();
                }

                match sign_and_save_mycelium(
                    &site,
                    domain,
                    &mut mycelium,
                    all_endpoints,
                    now_epoch_ms,
                ) {
                    Ok(_) => {}
                    Err(e) => return out.error(e.code(), &e.to_string()),
                }
            } else if !cmn_path.exists() {
                // Fresh site without endpoints_base: write unsigned Mycelium placeholder
                let manifest = Mycelium::new(
                    domain,
                    name.unwrap_or(domain),
                    synopsis.unwrap_or(""),
                    now_epoch_ms,
                );
                let manifest_json = match manifest.to_pretty_json() {
                    Ok(j) => j,
                    Err(e) => {
                        return out.error(
                            "serialize_error",
                            &format!("Failed to format cmn.json: {}", e),
                        )
                    }
                };
                if let Err(e) = std::fs::write(&cmn_path, &manifest_json) {
                    return out.error("write_error", &format!("Failed to write cmn.json: {}", e));
                }
            }
            // else: existing site without endpoints_base — keep existing cmn.json

            let data = json!({
                "domain": domain,
                "public_key": info.public_key,
                "site_path": site.root.display().to_string(),
            });

            let data = if info.newly_created {
                with_warning(
                    data,
                    format!(
                        "New keypair generated. Back up your private key at: {}\n\
                     If this file is lost, your domain identity cannot be recovered.",
                        site.private_key_path().display()
                    ),
                )
            } else {
                data
            };

            out.ok(data)
        }
        Err(e) => out.error_from("init_error", &e),
    }
}

/// Hub mode: generate key → compute subdomain → create site with correct domain + endpoints.
fn handle_init_hub(
    out: &Output,
    hub_domain: &str,
    site_path: Option<&str>,
    _name: Option<&str>,
    _synopsis: Option<&str>,
    _bio: Option<&str>,
) -> ExitCode {
    // Step 1: Generate key to a temporary site to get the public key.
    // We use a temp domain placeholder, then compute the real domain.
    let temp_domain = format!("_pending.{}", hub_domain);
    let temp_site = SiteDir::from_args(&temp_domain, None);

    let info = match auth::init_identity_with_site(&temp_domain, &temp_site) {
        Ok(i) => i,
        Err(e) => return out.error_from("init_error", &e),
    };

    // Step 2: Compute subdomain from public key
    let subdomain = match substrate::crypto::hub::compute_hub_subdomain(&info.public_key) {
        Ok(s) => s,
        Err(e) => {
            // Clean up temp site
            let _ = std::fs::remove_dir_all(&temp_site.root);
            return out.error("key_error", &format!("Failed to compute subdomain: {}", e));
        }
    };

    let domain = format!("{}.{}", subdomain, hub_domain);
    let endpoints_base = format!("https://{}", domain);

    // Step 3: Move key files to the real site directory
    let real_site = SiteDir::from_args(&domain, site_path);
    if real_site.root != temp_site.root {
        if let Some(parent) = real_site.root.parent() {
            if let Err(e) = std::fs::create_dir_all(parent) {
                let _ = std::fs::remove_dir_all(&temp_site.root);
                return out.error("write_error", &format!("Failed to create site dir: {}", e));
            }
        }
        if let Err(e) = std::fs::rename(&temp_site.root, &real_site.root) {
            let _ = std::fs::remove_dir_all(&temp_site.root);
            return out.error("write_error", &format!("Failed to rename site dir: {}", e));
        }
    }

    // Step 4: Build taste-only endpoints and cmn.json
    let taste_endpoints = substrate::CmnEndpoint {
        kind: "taste".to_string(),
        url: format!("{}/cmn/taste/{{hash}}.json", endpoints_base),
        hash: String::new(),
        hashes: vec![],
        format: None,
        delta_url: None,
        protocol_version: None,
    };

    let cmn_path = real_site.cmn_json_path();
    if let Some(parent) = cmn_path.parent() {
        if let Err(e) = std::fs::create_dir_all(parent) {
            return out.error(
                "write_error",
                &format!("Failed to create .well-known dir: {}", e),
            );
        }
    }

    // Build and sign cmn.json entry
    let capsules = vec![substrate::CmnCapsuleEntry {
        uri: substrate::build_domain_uri(&domain),
        key: info.public_key.clone(),
        previous_keys: vec![],
        endpoints: vec![taste_endpoints],
    }];

    let entry_signature = match crate::auth::sign_json_with_site(&real_site, &capsules) {
        Ok(s) => s,
        Err(crate::auth::JsonSignError::Jcs(message)) => {
            return out.error("serialize_error", &message)
        }
        Err(crate::auth::JsonSignError::Sign(err)) => {
            return out.error("sign_error", &format!("Failed to sign cmn.json: {}", err))
        }
    };

    let entry = substrate::CmnEntry {
        schema: substrate::CMN_SCHEMA.to_string(),
        protocol_versions: vec!["v1".to_string()],
        capsules,
        capsule_signature: entry_signature,
    };

    let entry_json = match entry.to_pretty_json_deep() {
        Ok(j) => j,
        Err(e) => {
            return out.error(
                "serialize_error",
                &format!("Failed to format cmn.json: {}", e),
            )
        }
    };

    if let Err(e) = std::fs::write(&cmn_path, &entry_json) {
        return out.error("write_error", &format!("Failed to write cmn.json: {}", e));
    }

    // Also create empty taste dir
    let _ = std::fs::create_dir_all(real_site.taste_dir());

    // Register the hub as a synapse node + set defaults for auto-submit
    let hub_url = format!("https://{}", hub_domain);
    let synapse_node = crate::config::SynapseNode {
        url: hub_url,
        token_secret: None,
    };
    if let Err(e) = crate::config::save_synapse_node(hub_domain, &synapse_node) {
        return out.error(
            "config_error",
            &format!("Failed to save synapse node: {}", e),
        );
    }

    let mut config = crate::config::HyphaConfig::load();
    config.defaults.taste.domain = Some(domain.clone());
    config.defaults.taste.synapse = Some(hub_domain.to_string());
    if let Err(e) = config.save() {
        return out.error("config_error", &format!("Failed to save defaults: {}", e));
    }

    let data = json!({
        "domain": domain,
        "subdomain": subdomain,
        "hub": hub_domain,
        "public_key": info.public_key,
        "site_path": real_site.root.display().to_string(),
        "cmn_json": cmn_path.display().to_string(),
    });

    let data = if info.newly_created {
        with_warning(
            data,
            format!(
                "New keypair generated. Back up your private key at: {}\n\
             If this file is lost, your domain identity cannot be recovered.",
                real_site.private_key_path().display()
            ),
        )
    } else {
        data
    };

    out.ok(data)
}
