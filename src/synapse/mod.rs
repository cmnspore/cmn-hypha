use serde_json::json;
use std::process::ExitCode;

use crate::api::Output;
use crate::config::{self, HyphaConfig, SynapseNode};

/// List all configured Synapse nodes
pub fn handle_list(out: &Output) -> ExitCode {
    let config = HyphaConfig::load();
    let default_domain = config.defaults.synapse.as_deref();
    let domains = config::list_synapse_domains();

    let nodes: Vec<serde_json::Value> = domains
        .iter()
        .filter_map(|domain| {
            let node = config::load_synapse_node(domain)?;
            Some(json!({
                "domain": domain,
                "url": node.url,
                "has_token": node.token_secret.is_some(),
                "default": Some(domain.as_str()) == default_domain,
            }))
        })
        .collect();

    out.ok(json!({
        "count": nodes.len(),
        "nodes": nodes,
        "default": default_domain,
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
        Err(e) => return out.error("synapse_error", &e),
    };

    let url = format!("{}/health", resolved.url.trim_end_matches('/'));

    let client = match substrate::client::http_client(30) {
        Ok(c) => c,
        Err(e) => return out.error("synapse_error", &format!("HTTP client error: {e}")),
    };

    let mut req = client.get(&url);
    if let Some(ref token) = resolved.token_secret {
        req = req.header("Authorization", format!("Bearer {}", token));
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

    let health: serde_json::Value = match response.json().await {
        Ok(v) => v,
        Err(e) => {
            return out.error(
                "synapse_error",
                &format!("Failed to parse synapse health: {}", e),
            )
        }
    };

    // Cache health.json to the node directory
    if let Ok(domain) = config::domain_from_url(&resolved.url) {
        let info_path = config::synapse_node_dir(&domain).join("health.json");
        if let Ok(json_str) = serde_json::to_string_pretty(&health) {
            let _ = std::fs::write(&info_path, json_str);
        }
    }

    out.ok(json!({
        "synapse": resolved.url,
        "health": health,
    }))
}

/// Add a Synapse node (domain extracted from URL)
pub fn handle_add(out: &Output, url: &str) -> ExitCode {
    let domain = match config::domain_from_url(url) {
        Ok(d) => d,
        Err(e) => return out.error("synapse_error", &e),
    };

    let node = SynapseNode {
        url: url.to_string(),
        token_secret: None,
    };

    if let Err(e) = config::save_synapse_node(&domain, &node) {
        return out.error("write_error", &e);
    }

    // Auto-set default if this is the first node
    let domains = config::list_synapse_domains();
    let mut config = HyphaConfig::load();
    if domains.len() == 1 && config.defaults.synapse.is_none() {
        config.defaults.synapse = Some(domain.clone());
        if let Err(e) = config.save() {
            return out.error("write_error", &e);
        }
    }

    out.ok(json!({
        "domain": domain,
        "url": url,
        "default": config.defaults.synapse.as_deref() == Some(domain.as_str()),
    }))
}

/// Remove a Synapse node
pub fn handle_remove(out: &Output, domain: &str) -> ExitCode {
    if config::load_synapse_node(domain).is_none() {
        return out.error("synapse_error", &format!("Synapse '{}' not found", domain));
    }

    if let Err(e) = config::remove_synapse_node(domain) {
        return out.error("write_error", &e);
    }

    // Clear default if it was this node
    let mut cfg = HyphaConfig::load();
    if cfg.defaults.synapse.as_deref() == Some(domain) {
        cfg.defaults.synapse = None;
        if let Err(e) = cfg.save() {
            return out.error("write_error", &e);
        }
    }

    out.ok(json!({
        "removed": domain,
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

    let mut cfg = HyphaConfig::load();
    cfg.defaults.synapse = Some(domain.to_string());

    if let Err(e) = cfg.save() {
        return out.error("write_error", &e);
    }

    out.ok(json!({
        "default": domain,
        "url": node.url,
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
        return out.error("write_error", &e);
    }

    out.ok(json!({
        "domain": domain,
        "token_set": node.token_secret.is_some(),
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
        Err(e) => return out.error("synapse_error", &e),
    };

    let client = match substrate::client::http_client(30) {
        Ok(c) => c,
        Err(e) => return out.error("synapse_error", &format!("HTTP client error: {e}")),
    };

    let opts = match resolved.token_secret.as_deref() {
        Some(t) => substrate::client::FetchOptions::with_bearer_token(t),
        None => Default::default(),
    };

    let results = match substrate::client::search(
        &client,
        &resolved.url,
        "",
        None,
        None,
        Some("follows:strain-synapse"),
        100,
        opts,
    )
    .await
    {
        Ok(r) => serde_json::to_value(r.result.spores).unwrap_or_default(),
        Err(e) => return out.error("synapse_error", &e.to_string()),
    };

    out.ok(json!({
        "synapse": resolved.url,
        "results": results,
    }))
}
