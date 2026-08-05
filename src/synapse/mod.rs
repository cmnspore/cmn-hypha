use serde_json::json;
use std::process::ExitCode;

use crate::api::Output;
use crate::config::{self, HyphaConfig, SynapseNode};

const JSON_FETCH_MAX_BYTES: usize = 8 * 1024 * 1024;

/// List all configured Synapse nodes
pub fn handle_list(out: &Output) -> ExitCode {
    let config = match HyphaConfig::load() {
        Ok(config) => config,
        Err(e) => return out.error_hypha(&e),
    };
    let default_domain = config.defaults.synapse_domain.as_deref();
    let domains = config::list_synapse_domains();

    let nodes: Vec<serde_json::Value> = domains
        .iter()
        .filter_map(|domain| {
            let node = config::load_synapse_node(domain)?;
            Some(json!({
                "domain": domain,
                "synapse_url": node.synapse_url,
                "has_token": node.token_secret.is_some(),
                "is_default": Some(domain.as_str()) == default_domain,
            }))
        })
        .collect();

    out.ok(json!({
        "node_count": nodes.len(),
        "nodes": nodes,
        "default_domain": default_domain,
    }))
}

/// Show health/info for a Synapse instance (fetches /health per strain-service spec).
pub async fn handle_info(
    out: &Output,
    synapse: Option<&str>,
    synapse_token_secret: Option<&str>,
) -> ExitCode {
    let resolved = match config::resolve_synapse(synapse, synapse_token_secret) {
        Ok(r) => r,
        Err(e) => return out.error_hypha(&e),
    };

    let url = format!("{}/health", resolved.synapse_url.trim_end_matches('/'));

    let client = match substrate::client::http_client(30) {
        Ok(c) => c,
        Err(e) => return out.error("synapse_error", &format!("HTTP client error: {e}")),
    };

    let mut req = client.get(&url);
    if let Some(ref token_secret) = resolved.token_secret {
        req = req.header("Authorization", format!("Bearer {}", token_secret));
    }

    let response = match req.send().await {
        Ok(r) => r,
        Err(e) => return out.error("synapse_error", &format!("Failed to reach synapse: {}", e)),
    };

    if !response.status().is_success() {
        return out.error(
            "synapse_error",
            &format!("Synapse returned HTTP {}", response.status()),
        );
    }

    let health: serde_json::Value =
        match substrate::client::json_from_response(response, &url, Some(JSON_FETCH_MAX_BYTES))
            .await
        {
            Ok(v) => v,
            Err(e) => {
                return out.error(
                    "synapse_error",
                    &format!("Failed to parse synapse health: {}", e),
                )
            }
        };

    // Cache health.json to the node directory
    if let Ok(domain) = config::domain_from_url(&resolved.synapse_url) {
        let info_path = config::synapse_node_dir(&domain).join("health.json");
        if let Ok(json_str) = serde_json::to_string_pretty(&health) {
            let _ = std::fs::write(&info_path, json_str);
        }
    }

    out.ok(json!({
        "synapse_url": resolved.synapse_url,
        "health": health,
    }))
}

/// Add a Synapse node (domain extracted from URL)
pub fn handle_add(out: &Output, url: &str) -> ExitCode {
    if let Err(e) = config::validate_synapse_url(url) {
        return out.error_hypha(&e);
    }

    let domain = match config::domain_from_url(url) {
        Ok(d) => d,
        Err(e) => return out.error_hypha(&e),
    };

    let node = SynapseNode {
        synapse_url: url.to_string(),
        token_secret: None,
    };

    if let Err(e) = config::save_synapse_node(&domain, &node) {
        return out.error_hypha(&e);
    }

    // Auto-set default if this is the first node
    let domains = config::list_synapse_domains();
    let mut config = match HyphaConfig::load() {
        Ok(config) => config,
        Err(e) => return out.error_hypha(&e),
    };
    if domains.len() == 1 && config.defaults.synapse_domain.is_none() {
        config.defaults.synapse_domain = Some(domain.clone());
        if let Err(e) = config.save() {
            return out.error_hypha(&e);
        }
    }

    out.ok(json!({
        "domain": domain,
        "synapse_url": url,
        "is_default": config.defaults.synapse_domain.as_deref() == Some(domain.as_str()),
    }))
}

/// Remove a Synapse node
pub fn handle_remove(out: &Output, domain: &str) -> ExitCode {
    if config::load_synapse_node(domain).is_none() {
        return out.error("synapse_error", &format!("Synapse '{}' not found", domain));
    }

    if let Err(e) = config::remove_synapse_node(domain) {
        return out.error_hypha(&e);
    }

    // Clear default if it was this node
    let mut cfg = match HyphaConfig::load() {
        Ok(cfg) => cfg,
        Err(e) => return out.error_hypha(&e),
    };
    if cfg.defaults.synapse_domain.as_deref() == Some(domain) {
        cfg.defaults.synapse_domain = None;
        if let Err(e) = cfg.save() {
            return out.error_hypha(&e);
        }
    }

    out.ok(json!({
        "removed_domain": domain,
    }))
}

/// Set default Synapse node
pub fn handle_use(out: &Output, domain: &str) -> ExitCode {
    let node = match config::load_synapse_node(domain) {
        Some(n) => n,
        None => {
            return out.error_hint(
                "synapse_error",
                &format!("Synapse '{}' not found", domain),
                Some("run: hypha synapse add <url>"),
            )
        }
    };

    let mut cfg = match HyphaConfig::load() {
        Ok(cfg) => cfg,
        Err(e) => return out.error_hypha(&e),
    };
    cfg.defaults.synapse_domain = Some(domain.to_string());

    if let Err(e) = cfg.save() {
        return out.error_hypha(&e);
    }

    out.ok(json!({
        "default_domain": domain,
        "synapse_url": node.synapse_url,
    }))
}

/// Configure a Synapse node (token, etc.)
pub fn handle_config(out: &Output, domain: &str, token_secret: Option<&str>) -> ExitCode {
    let mut node = match config::load_synapse_node(domain) {
        Some(n) => n,
        None => {
            return out.error_hint(
                "synapse_error",
                &format!("Synapse '{}' not found", domain),
                Some("run: hypha synapse add <url>"),
            )
        }
    };

    if let Some(ts) = token_secret {
        // Empty string clears the token
        node.token_secret = if ts.is_empty() {
            None
        } else {
            Some(ts.to_string())
        };
    }

    if let Err(e) = config::save_synapse_node(domain, &node) {
        return out.error_hypha(&e);
    }

    out.ok(json!({
        "domain": domain,
        "has_token": node.token_secret.is_some(),
    }))
}

/// Discover other Synapse instances via the network
pub async fn handle_discover(
    out: &Output,
    synapse: Option<&str>,
    synapse_token_secret: Option<&str>,
) -> ExitCode {
    let resolved = match config::resolve_synapse(synapse, synapse_token_secret) {
        Ok(r) => r,
        Err(e) => return out.error_hypha(&e),
    };

    let client = match substrate::client::http_client(30) {
        Ok(c) => c,
        Err(e) => return out.error("synapse_error", &format!("HTTP client error: {e}")),
    };

    let opts = match resolved.token_secret.as_deref() {
        Some(t) => {
            substrate::client::FetchOptions::with_bearer_token(t).max_bytes(JSON_FETCH_MAX_BYTES)
        }
        None => substrate::client::FetchOptions::with_max_bytes(JSON_FETCH_MAX_BYTES),
    };

    let response = match substrate::client::search(
        &client,
        &resolved.synapse_url,
        "",
        None,
        None,
        Some("follows:strain-synapse"),
        100,
        opts,
    )
    .await
    {
        Ok(response) => response,
        Err(e) => return out.error("synapse_error", &e.to_string()),
    };
    let results: Vec<crate::output::SearchResult> = response
        .result
        .spores
        .iter()
        .map(|result| crate::output::SearchResult {
            cmn_url: result.uri.clone(),
            domain: result.domain.clone(),
            name: result.name.clone(),
            synopsis: result.synopsis.clone(),
            license: result.license.clone(),
            intent: result.intent.clone(),
            relevance: result.relevance,
        })
        .collect();

    out.ok(json!({
        "synapse_url": resolved.synapse_url,
        "result_count": results.len(),
        "results": results,
    }))
}
