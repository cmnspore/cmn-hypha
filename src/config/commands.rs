use std::process::ExitCode;

use crate::api::Output;

use super::{config_path, HyphaConfig, KeyTrustRefreshMode, SynapseWitnessMode};

const VALID_KEYS: &str = "cache.path, cache.cmn_ttl_s, cache.key_trust_ttl_s, cache.key_trust_refresh_mode, cache.key_trust_synapse_witness_mode, cache.spore_max_download_bytes, cache.spore_max_extract_bytes, cache.spore_max_extract_files, cache.spore_max_extract_file_bytes, cache.spore_reject_path_components, cache.clock_skew_tolerance_s, cache.require_domain_first_key, defaults.synapse_domain, defaults.domain, defaults.taste.synapse_domain, defaults.taste.domain";

/// Handle `hypha config list`
pub fn handle_list(out: &Output) -> ExitCode {
    let cfg = match HyphaConfig::load() {
        Ok(cfg) => cfg,
        Err(e) => return out.error_hypha(&e),
    };
    let path = config_path();
    let config = match serde_json::to_value(&cfg) {
        Ok(config) => config,
        Err(err) => {
            return out.error(
                "serialize_error",
                &format!("Failed to serialize Hypha configuration: {err}"),
            )
        }
    };

    let data = serde_json::json!({
        "path": path.display().to_string(),
        "exists": path.exists(),
        "config": config,
    });

    out.ok(data)
}

/// Handle `hypha config set <key> <value>`
pub fn handle_set(out: &Output, key: &str, value: &str) -> ExitCode {
    let mut cfg = match HyphaConfig::load() {
        Ok(cfg) => cfg,
        Err(e) => return out.error_hypha(&e),
    };

    // Assign a u64-typed config field, erroring out on a non-integer value.
    macro_rules! set_u64 {
        ($field:expr) => {
            match value.parse::<u64>() {
                Ok(v) => $field = v,
                Err(_) => {
                    return out.error("invalid_value", &format!("Expected integer for {}", key))
                }
            }
        };
    }

    #[derive(serde::Deserialize)]
    struct StringListValue {
        value: Vec<String>,
    }

    let parse_string_list = |raw: &str| -> Result<Vec<String>, String> {
        toml::from_str::<StringListValue>(&format!("value = {}", raw))
            .map(|parsed| parsed.value)
            .map_err(|e| e.to_string())
    };

    match key {
        "cache.path" => cfg.cache.path = Some(value.to_string()),
        "cache.cmn_ttl_s" => set_u64!(cfg.cache.cmn_ttl_s),
        "cache.key_trust_ttl_s" => set_u64!(cfg.cache.key_trust_ttl_s),
        "cache.key_trust_refresh_mode" => match value {
            "expired" => cfg.cache.key_trust_refresh_mode = KeyTrustRefreshMode::Expired,
            "always" => cfg.cache.key_trust_refresh_mode = KeyTrustRefreshMode::Always,
            "offline" => cfg.cache.key_trust_refresh_mode = KeyTrustRefreshMode::Offline,
            _ => {
                return out.error(
                    "invalid_value",
                    &format!("Expected one of: expired, always, offline for {}", key),
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
        "cache.spore_max_download_bytes" => set_u64!(cfg.cache.spore_max_download_bytes),
        "cache.spore_max_extract_bytes" => set_u64!(cfg.cache.spore_max_extract_bytes),
        "cache.spore_max_extract_files" => set_u64!(cfg.cache.spore_max_extract_files),
        "cache.spore_max_extract_file_bytes" => set_u64!(cfg.cache.spore_max_extract_file_bytes),
        "cache.spore_reject_path_components" => {
            cfg.cache.spore_reject_path_components = match parse_string_list(value) {
                Ok(v) => v,
                Err(e) => {
                    return out.error(
                        "invalid_value",
                        &format!(
                            "Expected TOML string array for {} (example: [\".git\", \".cmn\"]): {}",
                            key, e
                        ),
                    )
                }
            }
        }
        "cache.clock_skew_tolerance_s" => set_u64!(cfg.cache.clock_skew_tolerance_s),
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
        "defaults.synapse_domain" => cfg.defaults.synapse_domain = Some(value.to_string()),
        "defaults.domain" => cfg.defaults.domain = Some(value.to_string()),
        "defaults.taste.synapse_domain" => {
            cfg.defaults.taste.synapse_domain = Some(value.to_string())
        }
        "defaults.taste.domain" => cfg.defaults.taste.domain = Some(value.to_string()),
        _ => {
            return out.error(
                "unknown_key",
                &format!("Unknown config key '{}'. Valid keys: {}", key, VALID_KEYS),
            )
        }
    }

    let config_value = match serde_json::to_value(&cfg) {
        Ok(value) => value,
        Err(error) => {
            return out.error(
                "serialize_error",
                &format!("Failed to serialize updated configuration: {error}"),
            )
        }
    };
    let pointer = format!("/{}", key.replace('.', "/"));
    let stored_value = config_value
        .pointer(&pointer)
        .cloned()
        .unwrap_or(serde_json::Value::Null);

    match cfg.save() {
        Ok(()) => out.ok(serde_json::json!({
            "key": key,
            "value": stored_value,
        })),
        Err(e) => out.error_hypha(&e),
    }
}
