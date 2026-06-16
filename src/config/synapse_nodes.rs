use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use super::{files::write_text_file_atomic, hypha_dir, HyphaConfig};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SynapseNode {
    pub url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token_secret: Option<String>,
}

/// Resolved synapse: URL + optional auth token
pub struct ResolvedSynapse {
    pub url: String,
    pub token_secret: Option<String>,
}

fn validate_synapse_domain(domain: &str) -> Result<(), crate::sink::HyphaError> {
    use crate::sink::HyphaError;

    if domain.is_empty() {
        return Err(HyphaError::new(
            "invalid_synapse_domain",
            "Synapse domain must not be empty",
        ));
    }
    if domain.chars().any(|c| c.is_control()) {
        return Err(HyphaError::new(
            "invalid_synapse_domain",
            format!(
                "Invalid synapse domain '{}': contains control characters",
                domain
            ),
        ));
    }

    let mut components = std::path::Path::new(domain).components();
    let single_normal_component =
        matches!(components.next(), Some(std::path::Component::Normal(_)))
            && components.next().is_none();
    if !single_normal_component {
        return Err(HyphaError::new(
            "invalid_synapse_domain",
            format!(
                "Invalid synapse domain '{}': must be a single path segment",
                domain
            ),
        ));
    }

    Ok(())
}

/// Directory for a synapse node: $CMN_HOME/hypha/synapse/<domain>/
pub fn synapse_node_dir(domain: &str) -> PathBuf {
    hypha_dir().join("synapse").join(domain)
}

/// Load a synapse node config from its directory
pub fn load_synapse_node(domain: &str) -> Option<SynapseNode> {
    if validate_synapse_domain(domain).is_err() {
        return None;
    }
    let path = synapse_node_dir(domain).join("config.toml");
    let content = std::fs::read_to_string(&path).ok()?;
    toml::from_str(&content).ok()
}

/// Save a synapse node config to its directory (0600 permissions)
pub fn save_synapse_node(domain: &str, node: &SynapseNode) -> Result<(), crate::sink::HyphaError> {
    use crate::sink::HyphaError;

    validate_synapse_domain(domain)?;
    let dir = synapse_node_dir(domain);
    std::fs::create_dir_all(&dir).map_err(|e| {
        HyphaError::new(
            "synapse_node_save_failed",
            format!("Failed to create synapse node directory: {}", e),
        )
    })?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).map_err(|e| {
            HyphaError::new(
                "synapse_node_save_failed",
                format!("Failed to protect synapse node directory: {}", e),
            )
        })?;
    }

    let content = toml::to_string_pretty(node).map_err(|e| {
        HyphaError::new(
            "synapse_node_save_failed",
            format!("Failed to serialize node config: {}", e),
        )
    })?;
    let path = dir.join("config.toml");
    write_text_file_atomic(
        &path,
        &content,
        0o600,
        "synapse_node_save_failed",
        "synapse node config",
    )
}

/// Remove a synapse node directory
pub fn remove_synapse_node(domain: &str) -> Result<(), crate::sink::HyphaError> {
    use crate::sink::HyphaError;

    validate_synapse_domain(domain)?;
    let dir = synapse_node_dir(domain);
    if dir.exists() {
        std::fs::remove_dir_all(&dir).map_err(|e| {
            HyphaError::new(
                "synapse_node_remove_failed",
                format!("Failed to remove synapse node directory: {}", e),
            )
        })?;
    }
    Ok(())
}

/// List all configured synapse node domains by scanning the synapse directory
pub fn list_synapse_domains() -> Vec<String> {
    let synapse_dir = hypha_dir().join("synapse");
    let entries = match std::fs::read_dir(&synapse_dir) {
        Ok(e) => e,
        Err(_) => return Vec::new(),
    };

    let mut domains: Vec<String> = entries
        .filter_map(|e| e.ok())
        .filter(|e| e.path().join("config.toml").exists())
        .filter_map(|e| e.file_name().into_string().ok())
        .collect();
    domains.sort();
    domains
}

/// Extract domain (host) from a URL
pub fn domain_from_url(url: &str) -> Result<String, crate::sink::HyphaError> {
    use crate::sink::HyphaError;

    let parsed = reqwest::Url::parse(url)
        .map_err(|e| HyphaError::new("invalid_url", format!("Invalid URL '{}': {}", url, e)))?;
    let domain = parsed
        .host_str()
        .map(|h| h.to_string())
        .ok_or_else(|| HyphaError::new("invalid_url", format!("URL '{}' has no host", url)))?;
    validate_synapse_domain(&domain)?;
    Ok(domain)
}

fn is_anonymous_transport_host(host: &str) -> bool {
    let host_lc = host.to_ascii_lowercase();
    host_lc.ends_with(".onion") || host_lc.ends_with(".i2p")
}

fn is_ip_literal_host(host: &str) -> bool {
    let host = host
        .strip_prefix('[')
        .and_then(|h| h.strip_suffix(']'))
        .unwrap_or(host);
    host.parse::<std::net::IpAddr>().is_ok()
}

/// Validate a Synapse URL without closing the door on future secure schemes.
///
/// Today `https` is the common safe transport. Plain `http` is rejected on
/// clearnet, but remains allowed for `.onion`/`.i2p`, matching substrate URL
/// validation.
pub fn validate_synapse_url(url: &str) -> Result<(), crate::sink::HyphaError> {
    use crate::sink::HyphaError;

    let parsed = reqwest::Url::parse(url).map_err(|e| {
        HyphaError::new(
            "invalid_synapse_url",
            format!("Invalid synapse URL '{}': {}", url, e),
        )
    })?;
    let host = parsed.host_str().ok_or_else(|| {
        HyphaError::new(
            "invalid_synapse_url",
            format!("Invalid synapse URL '{}': missing host", url),
        )
    })?;

    if is_ip_literal_host(host) {
        return Err(HyphaError::new(
            "invalid_synapse_url",
            format!(
                "IP literal hosts are rejected for synapse URL '{}'; use a domain name",
                url
            ),
        ));
    }

    match parsed.scheme() {
        "https" => {}
        "http" if is_anonymous_transport_host(host) => {}
        "http" => {
            return Err(HyphaError::new(
                "invalid_synapse_url",
                format!(
                    "Insecure cleartext transport rejected for synapse URL '{}'; use https, or http only for .onion/.i2p",
                    url
                ),
            ));
        }
        "ws" | "ftp" => {
            return Err(HyphaError::new(
                "invalid_synapse_url",
                format!(
                    "Insecure cleartext scheme '{}' rejected for synapse URL '{}'",
                    parsed.scheme(),
                    url
                ),
            ));
        }
        _ => {}
    }

    Ok(())
}

/// Resolve a synapse CLI argument (domain or URL) to a URL + optional token.
pub fn resolve_synapse(
    value: Option<&str>,
    token_override: Option<&str>,
) -> Result<ResolvedSynapse, crate::sink::HyphaError> {
    use crate::sink::HyphaError;

    let mut resolved = match value {
        Some(v) if reqwest::Url::parse(v).is_ok() => {
            validate_synapse_url(v)?;
            let domain = domain_from_url(v)?;
            let node = load_synapse_node(&domain);
            ResolvedSynapse {
                url: v.to_string(),
                token_secret: node.and_then(|n| n.token_secret),
            }
        }
        Some(domain) => {
            validate_synapse_domain(domain)?;
            match load_synapse_node(domain) {
                Some(node) => {
                    validate_synapse_url(&node.url)?;
                    ResolvedSynapse {
                        url: node.url,
                        token_secret: node.token_secret,
                    }
                }
                None => {
                    return Err(HyphaError::with_hint(
                        "synapse_not_found",
                        format!("Synapse '{}' not found", domain),
                        "run: hypha synapse add <url>",
                    ));
                }
            }
        }
        None => {
            let config = HyphaConfig::load()?;
            match &config.defaults.synapse {
                Some(default_domain) => match load_synapse_node(default_domain) {
                    Some(node) => {
                        validate_synapse_url(&node.url)?;
                        ResolvedSynapse {
                            url: node.url,
                            token_secret: node.token_secret,
                        }
                    }
                    None => {
                        return Err(HyphaError::with_hint(
                            "synapse_not_found",
                            format!("Default synapse '{}' not found", default_domain),
                            "run: hypha synapse add <url>",
                        ));
                    }
                },
                None => {
                    return Err(HyphaError::with_hint(
                        "synapse_not_configured",
                        "No synapse specified and no default configured",
                        "use -s <url> or run: hypha synapse add <url> && hypha synapse use <domain>",
                    ));
                }
            }
        }
    };

    if let Ok(ts) = std::env::var("SYNAPSE_TOKEN_SECRET") {
        resolved.token_secret = if ts.is_empty() { None } else { Some(ts) };
    }

    if let Some(ts) = token_override {
        resolved.token_secret = if ts.is_empty() {
            None
        } else {
            Some(ts.to_string())
        };
    }

    Ok(resolved)
}
