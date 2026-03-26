use serde::{Deserialize, Serialize};
use std::path::PathBuf;

// ═══════════════════════════════════════════
// Hypha config — $CMN_HOME/hypha/config.toml
// ═══════════════════════════════════════════

#[derive(Debug, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct HyphaConfig {
    pub defaults: Defaults,
    pub cache: CacheConfig,
}

#[derive(Debug, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct Defaults {
    /// Default synapse for queries (sense, lineage, search)
    pub synapse: Option<String>,
    /// Default domain for publishing (release)
    pub domain: Option<String>,
    /// Taste-specific overrides for auto-submission.
    /// Separate because taste may use a different domain (hub subdomain)
    /// and synapse (cmnhub.com) than general queries/publishing.
    pub taste: TasteDefaults,
}

#[derive(Debug, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct TasteDefaults {
    /// Synapse to submit taste reports to
    pub synapse: Option<String>,
    /// Domain to sign taste reports with
    pub domain: Option<String>,
}

#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum KeyTrustRefreshMode {
    /// Refresh key trust only when trust cache is expired/missing.
    #[default]
    Expired,
    /// Always refresh key trust from network sources.
    Always,
    /// Never refresh from network; rely on local trust cache only.
    Offline,
}

#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SynapseWitnessMode {
    /// Allow Synapse key witness when domain confirmation is unavailable.
    #[default]
    Allow,
    /// Require direct domain confirmation (or cached trust); do not use Synapse witness.
    RequireDomain,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(default)]
pub struct CacheConfig {
    /// Custom cache directory path (default: $CMN_HOME/hypha/cache/)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    /// cmn.json cache TTL in seconds (default: 300 = 5 minutes)
    pub cmn_ttl_s: u64,
    /// Key trust cache TTL in seconds (default: 604800 = 7 days)
    pub key_trust_ttl_s: u64,
    /// Key trust refresh strategy (default: expired)
    pub key_trust_refresh_mode: KeyTrustRefreshMode,
    /// Key trust fallback policy when domain is unreachable (default: allow)
    pub key_trust_synapse_witness_mode: SynapseWitnessMode,
    /// Maximum HTTP response body size in bytes (default: 1 GB)
    pub max_download_bytes: u64,
    /// Maximum total bytes to extract from an archive (default: 512 MB)
    pub max_extract_bytes: u64,
    /// Maximum number of files to extract from an archive (default: 100_000)
    pub max_extract_files: u64,
    /// Maximum size of a single file in an archive in bytes (default: 256 MB)
    pub max_extract_file_bytes: u64,
    /// Clock skew tolerance in seconds for key trust TTL checks (default: 300 = 5 minutes).
    /// Adds a grace period to prevent false "key_untrusted" errors caused by clock drift
    /// between the local machine and the publishing domain.
    pub clock_skew_tolerance_s: u64,
    /// Whether the initial key for a domain must come from the domain itself (TOFU).
    /// true = more secure, first contact requires domain to be online.
    /// false = allows synapse to provide initial key (less secure).
    pub require_domain_first_key: bool,
}

impl Default for CacheConfig {
    fn default() -> Self {
        Self {
            path: None,
            cmn_ttl_s: 300,
            key_trust_ttl_s: 604800, // 7 days
            key_trust_refresh_mode: KeyTrustRefreshMode::Expired,
            key_trust_synapse_witness_mode: SynapseWitnessMode::Allow,
            max_download_bytes: 1024 * 1024 * 1024, // 1 GB
            max_extract_bytes: 512 * 1024 * 1024,   // 512 MB
            max_extract_files: 100_000,
            max_extract_file_bytes: 256 * 1024 * 1024, // 256 MB
            clock_skew_tolerance_s: 300,               // 5 minutes
            require_domain_first_key: true,
        }
    }
}

impl HyphaConfig {
    pub fn load() -> Self {
        let path = config_path();
        match std::fs::read_to_string(&path) {
            Ok(content) => toml::from_str(&content).unwrap_or_default(),
            Err(_) => Self::default(),
        }
    }

    pub fn save(&self) -> Result<(), String> {
        let path = config_path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("Failed to create config directory: {}", e))?;
        }
        let content = toml::to_string_pretty(self)
            .map_err(|e| format!("Failed to serialize config: {}", e))?;
        std::fs::write(&path, content).map_err(|e| format!("Failed to write config.toml: {}", e))
    }
}

pub fn config_path() -> PathBuf {
    hypha_dir().join("config.toml")
}

/// $CMN_HOME/hypha/
pub fn hypha_dir() -> PathBuf {
    crate::site::get_cmn_home().join("hypha")
}

// ═══════════════════════════════════════════
// CLI handlers: hypha config list/set
// ═══════════════════════════════════════════

use crate::api::Output;
use std::process::ExitCode;

/// Handle `hypha config list`
pub fn handle_list(out: &Output) -> ExitCode {
    let cfg = HyphaConfig::load();
    let path = config_path();

    let data = serde_json::json!({
        "path": path.display().to_string(),
        "exists": path.exists(),
        "config": serde_json::to_value(&cfg).unwrap_or_default(),
    });

    out.ok(data)
}

/// Handle `hypha config set <key> <value>`
pub fn handle_set(out: &Output, key: &str, value: &str) -> ExitCode {
    let mut cfg = HyphaConfig::load();

    match key {
        "cache.path" => cfg.cache.path = Some(value.to_string()),
        "cache.cmn_ttl_s" => match value.parse::<u64>() {
            Ok(v) => cfg.cache.cmn_ttl_s = v,
            Err(_) => return out.error("invalid_value", &format!("Expected integer for {}", key)),
        },
        "cache.key_trust_ttl_s" => match value.parse::<u64>() {
            Ok(v) => cfg.cache.key_trust_ttl_s = v,
            Err(_) => return out.error("invalid_value", &format!("Expected integer for {}", key)),
        },
        "cache.key_trust_refresh_mode" => match value {
            "expired" => cfg.cache.key_trust_refresh_mode = KeyTrustRefreshMode::Expired,
            "always" => cfg.cache.key_trust_refresh_mode = KeyTrustRefreshMode::Always,
            "offline" => cfg.cache.key_trust_refresh_mode = KeyTrustRefreshMode::Offline,
            _ => {
                return out.error(
                    "invalid_value",
                    &format!(
                        "Expected one of: expired, always, offline for {}",
                        key
                    ),
                )
            }
        },
        "cache.key_trust_synapse_witness_mode" => match value {
            "allow" => cfg.cache.key_trust_synapse_witness_mode = SynapseWitnessMode::Allow,
            "require_domain" => {
                cfg.cache.key_trust_synapse_witness_mode = SynapseWitnessMode::RequireDomain
            }
            _ => {
                return out.error(
                    "invalid_value",
                    &format!("Expected one of: allow, require_domain for {}", key),
                )
            }
        },
        "cache.max_download_bytes" => match value.parse::<u64>() {
            Ok(v) => cfg.cache.max_download_bytes = v,
            Err(_) => return out.error("invalid_value", &format!("Expected integer for {}", key)),
        },
        "cache.max_extract_bytes" => match value.parse::<u64>() {
            Ok(v) => cfg.cache.max_extract_bytes = v,
            Err(_) => return out.error("invalid_value", &format!("Expected integer for {}", key)),
        },
        "cache.max_extract_files" => match value.parse::<u64>() {
            Ok(v) => cfg.cache.max_extract_files = v,
            Err(_) => return out.error("invalid_value", &format!("Expected integer for {}", key)),
        },
        "cache.max_extract_file_bytes" => match value.parse::<u64>() {
            Ok(v) => cfg.cache.max_extract_file_bytes = v,
            Err(_) => return out.error("invalid_value", &format!("Expected integer for {}", key)),
        },
        "cache.clock_skew_tolerance_s" => match value.parse::<u64>() {
            Ok(v) => cfg.cache.clock_skew_tolerance_s = v,
            Err(_) => return out.error("invalid_value", &format!("Expected integer for {}", key)),
        },
        "cache.require_domain_first_key" => match value {
            "true" => cfg.cache.require_domain_first_key = true,
            "false" => cfg.cache.require_domain_first_key = false,
            _ => {
                return out.error(
                    "invalid_value",
                    &format!("Expected one of: true, false for {}", key),
                )
            }
        },
        "defaults.synapse" => cfg.defaults.synapse = Some(value.to_string()),
        "defaults.domain" => cfg.defaults.domain = Some(value.to_string()),
        "defaults.taste.synapse" => cfg.defaults.taste.synapse = Some(value.to_string()),
        "defaults.taste.domain" => cfg.defaults.taste.domain = Some(value.to_string()),
        _ => return out.error("unknown_key", &format!(
            "Unknown config key '{}'. Valid keys: cache.path, cache.cmn_ttl_s, cache.key_trust_ttl_s, cache.key_trust_refresh_mode, cache.key_trust_synapse_witness_mode, cache.max_download_bytes, cache.max_extract_bytes, cache.max_extract_files, cache.max_extract_file_bytes, cache.clock_skew_tolerance_s, cache.require_domain_first_key, defaults.synapse, defaults.domain, defaults.taste.synapse, defaults.taste.domain",
            key
        )),
    }

    match cfg.save() {
        Ok(()) => out.ok(serde_json::json!({
            "key": key,
            "value": value,
        })),
        Err(e) => out.error("save_error", &e),
    }
}

// ═══════════════════════════════════════════
// Per-node synapse config — $CMN_HOME/hypha/synapse/<domain>/config.toml
// ═══════════════════════════════════════════

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

fn validate_synapse_domain(domain: &str) -> Result<(), String> {
    if domain.is_empty() {
        return Err("Synapse domain must not be empty".to_string());
    }
    if domain.chars().any(|c| c.is_control()) {
        return Err(format!(
            "Invalid synapse domain '{}': contains control characters",
            domain
        ));
    }

    let mut components = std::path::Path::new(domain).components();
    let single_normal_component =
        matches!(components.next(), Some(std::path::Component::Normal(_)))
            && components.next().is_none();
    if !single_normal_component {
        return Err(format!(
            "Invalid synapse domain '{}': must be a single path segment",
            domain
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
pub fn save_synapse_node(domain: &str, node: &SynapseNode) -> Result<(), String> {
    validate_synapse_domain(domain)?;
    let dir = synapse_node_dir(domain);
    std::fs::create_dir_all(&dir)
        .map_err(|e| format!("Failed to create synapse node directory: {}", e))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700))
            .map_err(|e| format!("Failed to protect synapse node directory: {}", e))?;
    }

    let path = dir.join("config.toml");
    let content = toml::to_string_pretty(node)
        .map_err(|e| format!("Failed to serialize node config: {}", e))?;
    std::fs::write(&path, &content).map_err(|e| format!("Failed to write node config: {}", e))?;

    // Protect config.toml (0600) — may contain token_secret
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(&path)
            .map_err(|e| format!("Failed to read node config metadata: {}", e))?
            .permissions();
        perms.set_mode(0o600);
        std::fs::set_permissions(&path, perms)
            .map_err(|e| format!("Failed to set node config permissions: {}", e))?;
    }

    Ok(())
}

/// Remove a synapse node directory
pub fn remove_synapse_node(domain: &str) -> Result<(), String> {
    validate_synapse_domain(domain)?;
    let dir = synapse_node_dir(domain);
    if dir.exists() {
        std::fs::remove_dir_all(&dir)
            .map_err(|e| format!("Failed to remove synapse node directory: {}", e))?;
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
pub fn domain_from_url(url: &str) -> Result<String, String> {
    let parsed = reqwest::Url::parse(url).map_err(|e| format!("Invalid URL '{}': {}", url, e))?;
    parsed
        .host_str()
        .map(|h| h.to_string())
        .ok_or_else(|| format!("URL '{}' has no host", url))
}

/// Resolve a synapse CLI argument (domain or URL) to a URL + optional token.
///
/// - If value starts with `http`, treat as URL (check nodes for matching domain)
/// - If value is a domain, look up node directory
/// - If `None`, use `defaults.synapse` from config.toml
/// - `token_override` from `--synapse-token-secret` takes priority over config
pub fn resolve_synapse(
    value: Option<&str>,
    token_override: Option<&str>,
) -> Result<ResolvedSynapse, String> {
    let mut resolved = match value {
        Some(v) if reqwest::Url::parse(v).is_ok() => {
            // Raw URL — check if a node exists for this domain
            let parsed = reqwest::Url::parse(v)
                .map_err(|e| format!("Invalid synapse URL '{}': {}", v, e))?;
            if parsed.scheme() != "http" && parsed.scheme() != "https" {
                return Err(format!(
                    "Invalid synapse URL '{}': scheme must be http or https",
                    v
                ));
            }
            let domain = domain_from_url(v)?;
            let node = load_synapse_node(&domain);
            ResolvedSynapse {
                url: v.to_string(),
                token_secret: node.and_then(|n| n.token_secret),
            }
        }
        Some(domain) => {
            validate_synapse_domain(domain)?;
            // Look up by domain
            match load_synapse_node(domain) {
                Some(node) => ResolvedSynapse {
                    url: node.url,
                    token_secret: node.token_secret,
                },
                None => {
                    return Err(format!(
                        "Synapse '{}' not found (run: hypha synapse add <url>)",
                        domain
                    ))
                }
            }
        }
        None => {
            // Use default from config.toml
            let config = HyphaConfig::load();
            match &config.defaults.synapse {
                Some(default_domain) => match load_synapse_node(default_domain) {
                    Some(node) => ResolvedSynapse {
                        url: node.url,
                        token_secret: node.token_secret,
                    },
                    None => return Err(format!(
                        "Default synapse '{}' not found (run: hypha synapse add <url>)",
                        default_domain
                    )),
                },
                None => return Err(
                    "No synapse specified and no default configured (use -s <url> or run: hypha synapse add <url> && hypha synapse use <domain>)".to_string(),
                ),
            }
        }
    };

    // env var SYNAPSE_TOKEN_SECRET overrides config
    if let Ok(ts) = std::env::var("SYNAPSE_TOKEN_SECRET") {
        resolved.token_secret = if ts.is_empty() { None } else { Some(ts) };
    }

    // CLI --synapse-token-secret overrides env var
    if let Some(ts) = token_override {
        resolved.token_secret = if ts.is_empty() {
            None
        } else {
            Some(ts.to_string())
        };
    }

    Ok(resolved)
}

#[cfg(test)]
pub static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {

    use super::*;

    #[test]
    fn test_default_values() {
        let cfg = HyphaConfig::default();
        assert_eq!(cfg.cache.cmn_ttl_s, 300);
        assert!(cfg.defaults.synapse.is_none());
    }

    #[test]
    fn test_parse_full_toml() {
        let toml_str = r#"
[defaults]
synapse = "synapse.cmn.dev"

[cache]
cmn_ttl_s = 60
"#;
        let cfg: HyphaConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(cfg.cache.cmn_ttl_s, 60);
        assert_eq!(cfg.defaults.synapse.as_deref(), Some("synapse.cmn.dev"));
    }

    #[test]
    fn test_parse_partial_toml_cmn_only() {
        let toml_str = r#"
[cache]
cmn_ttl_s = 10
"#;
        let cfg: HyphaConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(cfg.cache.cmn_ttl_s, 10);
    }

    #[test]
    fn test_parse_empty_toml() {
        let cfg: HyphaConfig = toml::from_str("").unwrap();
        assert_eq!(cfg.cache.cmn_ttl_s, 300);
    }

    #[test]
    fn test_parse_empty_cache_section() {
        let toml_str = "[cache]\n";
        let cfg: HyphaConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(cfg.cache.cmn_ttl_s, 300);
    }

    #[test]
    fn test_invalid_toml_falls_back_to_default() {
        let bad_toml = "this is not valid toml {{{{";
        let cfg: HyphaConfig = toml::from_str(bad_toml).unwrap_or_default();
        assert_eq!(cfg.cache.cmn_ttl_s, 300);
    }

    #[test]
    fn test_require_domain_first_key_default_true() {
        let cfg = HyphaConfig::default();
        assert!(cfg.cache.require_domain_first_key);
    }

    #[test]
    fn test_require_domain_first_key_toml_parse() {
        let toml_str = r#"
[cache]
require_domain_first_key = false
"#;
        let cfg: HyphaConfig = toml::from_str(toml_str).unwrap();
        assert!(!cfg.cache.require_domain_first_key);
    }

    #[test]
    fn test_require_domain_first_key_absent_defaults_true() {
        let toml_str = r#"
[cache]
cmn_ttl_s = 60
"#;
        let cfg: HyphaConfig = toml::from_str(toml_str).unwrap();
        assert!(cfg.cache.require_domain_first_key);
    }

    #[test]
    fn test_zero_ttl_allowed() {
        let toml_str = r#"
[cache]
cmn_ttl_s = 0
"#;
        let cfg: HyphaConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(cfg.cache.cmn_ttl_s, 0);
    }

    #[test]
    fn test_config_save_load() {
        let _lock = super::ENV_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        std::env::set_var("CMN_HOME", dir.path().to_str().unwrap());

        let mut cfg = HyphaConfig::default();
        cfg.defaults.synapse = Some("synapse.cmn.dev".to_string());
        cfg.cache.cmn_ttl_s = 999;
        cfg.save().unwrap();

        let loaded = HyphaConfig::load();
        assert_eq!(loaded.defaults.synapse.as_deref(), Some("synapse.cmn.dev"));
        assert_eq!(loaded.cache.cmn_ttl_s, 999);

        std::env::remove_var("CMN_HOME");
    }

    // ═══════════════════════════════════════════
    // Synapse node tests
    // ═══════════════════════════════════════════

    #[test]
    fn test_synapse_node_roundtrip() {
        let toml_str = r#"
url = "https://synapse.cmn.dev"
token_secret = "sk-abc123"
"#;
        let node: SynapseNode = toml::from_str(toml_str).unwrap();
        assert_eq!(node.url, "https://synapse.cmn.dev");
        assert_eq!(node.token_secret.as_deref(), Some("sk-abc123"));

        let serialized = toml::to_string_pretty(&node).unwrap();
        let parsed: SynapseNode = toml::from_str(&serialized).unwrap();
        assert_eq!(parsed.url, "https://synapse.cmn.dev");
        assert_eq!(parsed.token_secret.as_deref(), Some("sk-abc123"));
    }

    #[test]
    fn test_synapse_node_no_token() {
        let toml_str = "url = \"https://synapse.cmn.dev\"\n";
        let node: SynapseNode = toml::from_str(toml_str).unwrap();
        assert!(node.token_secret.is_none());

        // Verify token_secret is not serialized when None
        let serialized = toml::to_string_pretty(&node).unwrap();
        assert!(!serialized.contains("token_secret"));
    }

    #[test]
    fn test_save_load_synapse_node() {
        let _lock = super::ENV_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        std::env::set_var("CMN_HOME", dir.path().to_str().unwrap());

        let node = SynapseNode {
            url: "https://synapse.cmn.dev".to_string(),
            token_secret: Some("tok".to_string()),
        };
        save_synapse_node("synapse.cmn.dev", &node).unwrap();

        let node_dir = dir
            .path()
            .join("hypha")
            .join("synapse")
            .join("synapse.cmn.dev");
        assert!(node_dir.join("config.toml").exists());

        // Verify permissions
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(node_dir.join("config.toml"))
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(
                mode & 0o777,
                0o600,
                "config.toml should be 0600, got {:o}",
                mode & 0o777
            );
        }

        let loaded = load_synapse_node("synapse.cmn.dev").unwrap();
        assert_eq!(loaded.url, "https://synapse.cmn.dev");
        assert_eq!(loaded.token_secret.as_deref(), Some("tok"));

        std::env::remove_var("CMN_HOME");
    }

    #[test]
    fn test_list_synapse_domains() {
        let _lock = super::ENV_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        std::env::set_var("CMN_HOME", dir.path().to_str().unwrap());

        save_synapse_node(
            "beta.example.com",
            &SynapseNode {
                url: "https://beta.example.com".to_string(),
                token_secret: None,
            },
        )
        .unwrap();
        save_synapse_node(
            "alpha.example.com",
            &SynapseNode {
                url: "https://alpha.example.com".to_string(),
                token_secret: None,
            },
        )
        .unwrap();

        let domains = list_synapse_domains();
        assert_eq!(domains, vec!["alpha.example.com", "beta.example.com"]);

        std::env::remove_var("CMN_HOME");
    }

    #[test]
    fn test_remove_synapse_node() {
        let _lock = super::ENV_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        std::env::set_var("CMN_HOME", dir.path().to_str().unwrap());

        save_synapse_node(
            "test.example.com",
            &SynapseNode {
                url: "https://test.example.com".to_string(),
                token_secret: None,
            },
        )
        .unwrap();

        assert!(load_synapse_node("test.example.com").is_some());

        remove_synapse_node("test.example.com").unwrap();
        assert!(load_synapse_node("test.example.com").is_none());
        assert!(list_synapse_domains().is_empty());

        std::env::remove_var("CMN_HOME");
    }

    #[test]
    fn test_domain_from_url() {
        assert_eq!(
            domain_from_url("https://synapse.cmn.dev").unwrap(),
            "synapse.cmn.dev"
        );
        assert_eq!(
            domain_from_url("http://localhost:8080").unwrap(),
            "localhost"
        );
        assert_eq!(
            domain_from_url("https://example.com/path").unwrap(),
            "example.com"
        );
        assert!(domain_from_url("not-a-url").is_err());
    }

    #[test]
    fn test_resolve_synapse_env_var_override() {
        let _lock = super::ENV_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        std::env::set_var("CMN_HOME", dir.path().to_str().unwrap());

        // Set up a node with a token
        save_synapse_node(
            "test.example.com",
            &SynapseNode {
                url: "https://test.example.com".to_string(),
                token_secret: Some("config-token".to_string()),
            },
        )
        .unwrap();

        // Env var overrides config
        std::env::set_var("SYNAPSE_TOKEN_SECRET", "env-token");
        let resolved = resolve_synapse(Some("test.example.com"), None).unwrap();
        assert_eq!(resolved.token_secret.as_deref(), Some("env-token"));

        // CLI flag overrides env var
        let resolved = resolve_synapse(Some("test.example.com"), Some("cli-token")).unwrap();
        assert_eq!(resolved.token_secret.as_deref(), Some("cli-token"));

        // Empty CLI flag clears even when env var is set
        let resolved = resolve_synapse(Some("test.example.com"), Some("")).unwrap();
        assert!(resolved.token_secret.is_none());

        std::env::remove_var("SYNAPSE_TOKEN_SECRET");

        // Without env var, config token is used
        let resolved = resolve_synapse(Some("test.example.com"), None).unwrap();
        assert_eq!(resolved.token_secret.as_deref(), Some("config-token"));

        std::env::remove_var("CMN_HOME");
    }

    #[test]
    fn test_load_missing_node_returns_none() {
        let _lock = super::ENV_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        std::env::set_var("CMN_HOME", dir.path().to_str().unwrap());

        assert!(load_synapse_node("nonexistent.example.com").is_none());

        std::env::remove_var("CMN_HOME");
    }
}
