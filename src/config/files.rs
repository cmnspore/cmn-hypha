use std::path::{Path, PathBuf};

use agent_first_data::document::{
    CreateOptions, Document, DocumentError, DocumentFile, Format, Value,
};

use super::HyphaConfig;

const CONFIG_MAX_BYTES: u64 = 1024 * 1024;

const CONFIG_KEYS: &[&str] = &[
    "cache.path",
    "cache.cmn_ttl_s",
    "cache.key_trust_ttl_s",
    "cache.key_trust_refresh_mode",
    "cache.key_trust_synapse_witness_mode",
    "cache.spore_max_download_bytes",
    "cache.spore_max_extract_bytes",
    "cache.spore_max_extract_files",
    "cache.spore_max_extract_file_bytes",
    "cache.spore_reject_path_components",
    "cache.clock_skew_tolerance_s",
    "cache.require_domain_first_key",
    "defaults.synapse_domain",
    "defaults.domain",
    "defaults.taste.synapse_domain",
    "defaults.taste.domain",
];

impl HyphaConfig {
    pub fn load() -> Result<Self, crate::sink::HyphaError> {
        use crate::sink::HyphaError;

        let path = config_path();
        match std::fs::symlink_metadata(&path) {
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Self::default());
            }
            Err(error) => {
                return Err(HyphaError::with_hint(
                    "config_read_failed",
                    format!("Failed to inspect {}: {}", path.display(), error),
                    "fix the file permissions and retry",
                ));
            }
        }

        let doc = DocumentFile::open_capped(&path, Some(Format::Toml), CONFIG_MAX_BYTES)
            .map_err(|error| config_load_error(&path, &error))?;
        agent_first_data::document::from_value(doc.value(), "")
            .map_err(|error| config_load_error(&path, &error))
    }

    pub fn save(&self) -> Result<(), crate::sink::HyphaError> {
        use crate::sink::HyphaError;

        let path = config_path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| {
                HyphaError::new(
                    "config_save_failed",
                    format!("Failed to create config directory: {}", e),
                )
            })?;
        }

        let desired_source = toml::to_string_pretty(self).map_err(|e| {
            HyphaError::new(
                "config_save_failed",
                format!("Failed to serialize config: {}", e),
            )
        })?;
        let desired = Document::parse(&desired_source, Format::Toml).map_err(|error| {
            config_document_error("config_save_failed", &path, "prepare configuration", &error)
        })?;
        let default_source = toml::to_string_pretty(&Self::default()).map_err(|error| {
            HyphaError::new(
                "config_save_failed",
                format!("Failed to serialize default config: {error}"),
            )
        })?;
        let defaults = Document::parse(&default_source, Format::Toml).map_err(|error| {
            config_document_error(
                "config_save_failed",
                &path,
                "prepare default configuration",
                &error,
            )
        })?;

        match std::fs::symlink_metadata(&path) {
            Ok(_) => save_existing_config(&path, &desired, &defaults),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                let options = CreateOptions::new().unix_mode(0o600);
                DocumentFile::create_atomic(&path, desired, options)
                    .map(|_| ())
                    .map_err(|error| {
                        config_document_error(
                            "config_save_failed",
                            &path,
                            "create configuration",
                            &error,
                        )
                    })
            }
            Err(error) => Err(HyphaError::with_hint(
                "config_save_failed",
                format!("Failed to inspect {}: {}", path.display(), error),
                "fix the file permissions and retry",
            )),
        }
    }
}

fn save_existing_config(
    path: &Path,
    desired: &Document,
    defaults: &Document,
) -> Result<(), crate::sink::HyphaError> {
    let mut current = DocumentFile::open_capped(path, Some(Format::Toml), CONFIG_MAX_BYTES)
        .map_err(|error| config_load_error(path, &error))?;
    let mut changed = false;

    for key in CONFIG_KEYS {
        let desired_value = optional_value(desired, key).map_err(|error| {
            config_document_error(
                "config_save_failed",
                path,
                "read prepared configuration",
                &error,
            )
        })?;
        let default_value = optional_value(defaults, key).map_err(|error| {
            config_document_error(
                "config_save_failed",
                path,
                "read default configuration",
                &error,
            )
        })?;
        let current_value = optional_value(&current, key).map_err(|error| {
            config_document_error(
                "config_save_failed",
                path,
                "read existing configuration",
                &error,
            )
        })?;

        match (desired_value, current_value) {
            (Some(desired_value), Some(current_value)) if desired_value == current_value => {}
            (Some(desired_value), None) if Some(&desired_value) == default_value.as_ref() => {}
            (Some(desired_value), _) => {
                current.set(key, desired_value).map_err(|error| {
                    config_document_error(
                        "config_save_failed",
                        path,
                        "stage configuration update",
                        &error,
                    )
                })?;
                changed = true;
            }
            (None, Some(_)) => {
                current.unset(key).map_err(|error| {
                    config_document_error(
                        "config_save_failed",
                        path,
                        "stage configuration removal",
                        &error,
                    )
                })?;
                changed = true;
            }
            (None, None) => {}
        }
    }

    if !changed {
        return Ok(());
    }

    current.decode::<HyphaConfig>().map_err(|error| {
        config_document_error(
            "config_save_failed",
            path,
            "validate updated configuration",
            &error,
        )
    })?;
    current.save().map_err(|error| {
        config_document_error("config_save_failed", path, "commit configuration", &error)
    })
}

fn optional_value(doc: &Document, key: &str) -> Result<Option<Value>, DocumentError> {
    match doc.value_at(key) {
        Ok(value) => Ok(Some(value)),
        Err(error) if error.code() == "document_path_not_found" => Ok(None),
        Err(error) => Err(error),
    }
}

fn config_load_error(path: &Path, error: &DocumentError) -> crate::sink::HyphaError {
    let code = match error.code() {
        "document_parse_failed" | "document_type_mismatch" => "config_parse_failed",
        _ => "config_read_failed",
    };
    config_document_error(code, path, "load configuration", error)
}

fn config_document_error(
    code: &str,
    path: &Path,
    operation: &str,
    error: &DocumentError,
) -> crate::sink::HyphaError {
    crate::sink::HyphaError::with_hint(
        code,
        format!(
            "Failed to {} at {}: {} ({})",
            operation,
            path.display(),
            error.redacted_message(),
            error.code()
        ),
        "fix the configuration file or its permissions and retry",
    )
}

pub(super) fn write_text_file_atomic(
    path: &Path,
    content: &str,
    mode: u32,
    error_code: &str,
    file_label: &str,
) -> Result<(), crate::sink::HyphaError> {
    use crate::sink::HyphaError;

    let parent = path.parent().ok_or_else(|| {
        HyphaError::new(
            error_code,
            format!("Failed to determine parent directory for {}", file_label),
        )
    })?;
    crate::cache::fs::atomic_write_file(path, content.as_bytes(), parent, Some(mode))
        .map_err(|e| HyphaError::new(error_code, format!("Failed to write {}: {}", file_label, e)))
}

pub fn config_path() -> PathBuf {
    hypha_dir().join("config.toml")
}

/// $CMN_HOME/hypha/
pub fn hypha_dir() -> PathBuf {
    crate::site::get_cmn_home().join("hypha")
}
