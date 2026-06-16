use serde::{Deserialize, Serialize};

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
#[serde(default, deny_unknown_fields)]
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
    /// Maximum spore archive HTTP response body size in bytes (default: 1 GB)
    pub spore_max_download_bytes: u64,
    /// Maximum total bytes to extract from a spore archive (default: 512 MB)
    pub spore_max_extract_bytes: u64,
    /// Maximum number of files to extract from a spore archive (default: 100_000)
    pub spore_max_extract_files: u64,
    /// Maximum size of a single file in a spore archive in bytes (default: 256 MB)
    pub spore_max_extract_file_bytes: u64,
    /// Path components rejected when receiving spore content.
    pub spore_reject_path_components: Vec<String>,
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
            key_trust_ttl_s: 604800,
            key_trust_refresh_mode: KeyTrustRefreshMode::Expired,
            key_trust_synapse_witness_mode: SynapseWitnessMode::Allow,
            spore_max_download_bytes: 1024 * 1024 * 1024,
            spore_max_extract_bytes: 512 * 1024 * 1024,
            spore_max_extract_files: 100_000,
            spore_max_extract_file_bytes: 256 * 1024 * 1024,
            spore_reject_path_components: vec![".git".to_string(), ".cmn".to_string()],
            clock_skew_tolerance_s: 300,
            require_domain_first_key: true,
        }
    }
}
