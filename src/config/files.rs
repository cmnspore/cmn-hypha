use std::path::{Path, PathBuf};

use agent_first_data::document::{Document, DocumentError, DocumentFile, Format, Value};

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
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => write_text_file_atomic(
                &path,
                desired.source(),
                0o600,
                "config_save_failed",
                "config.toml",
            ),
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
    let mut collection_updates = Vec::new();

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
                if desired_value.is_array() || desired_value.is_object() {
                    collection_updates.push((*key, desired_value));
                } else {
                    current.set(key, desired_value).map_err(|error| {
                        config_document_error(
                            "config_save_failed",
                            path,
                            "stage configuration update",
                            &error,
                        )
                    })?;
                }
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

    if collection_updates.is_empty() {
        agent_first_data::document::from_value::<HyphaConfig>(current.value(), "").map_err(
            |error| {
                config_document_error(
                    "config_save_failed",
                    path,
                    "validate updated configuration",
                    &error,
                )
            },
        )?;
        return current.save().map_err(|error| {
            config_document_error("config_save_failed", path, "commit configuration", &error)
        });
    }

    current.ensure_mutable("set").map_err(|error| {
        config_document_error(
            "config_save_failed",
            path,
            "preflight collection update",
            &error,
        )
    })?;
    let mut source = current.source().to_string();
    for (key, value) in collection_updates {
        source = set_toml_collection(&source, key, &value).map_err(|error| {
            crate::sink::HyphaError::new(
                "config_save_failed",
                format!(
                    "Failed to stage collection update at {}: {}",
                    path.display(),
                    error
                ),
            )
        })?;
    }
    let validated = Document::parse(&source, Format::Toml).map_err(|error| {
        config_document_error(
            "config_save_failed",
            path,
            "validate updated configuration",
            &error,
        )
    })?;
    agent_first_data::document::from_value::<HyphaConfig>(validated.value(), "").map_err(
        |error| {
            config_document_error(
                "config_save_failed",
                path,
                "validate updated configuration types",
                &error,
            )
        },
    )?;
    write_text_file_atomic(
        path,
        validated.source(),
        0o600,
        "config_save_failed",
        "config.toml",
    )
}

fn set_toml_collection(source: &str, key: &str, value: &Value) -> Result<String, &'static str> {
    if key != "cache.spore_reject_path_components" {
        return Err("no dedicated editor is registered for this collection field");
    }
    let items = value
        .as_array()
        .ok_or("spore_reject_path_components must be an array")?;
    let mut array = toml_edit::Array::new();
    for item in items {
        let item = item
            .as_str()
            .ok_or("spore_reject_path_components entries must be strings")?;
        array.push(item);
    }

    let mut document = source
        .parse::<toml_edit::DocumentMut>()
        .map_err(|_| "existing TOML could not be parsed by the collection editor")?;
    if document.get("cache").is_none() {
        document.insert("cache", toml_edit::Item::Table(toml_edit::Table::new()));
    }
    let cache = document
        .get_mut("cache")
        .and_then(toml_edit::Item::as_table_like_mut)
        .ok_or("cache must be a TOML table")?;
    let mut replacement = toml_edit::value(array);
    if let Some(decor) = cache
        .get("spore_reject_path_components")
        .and_then(toml_edit::Item::as_value)
        .map(|value| value.decor().clone())
    {
        if let Some(value) = replacement.as_value_mut() {
            *value.decor_mut() = decor;
        }
    }
    cache.insert("spore_reject_path_components", replacement);
    Ok(document.to_string())
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
    use std::io::Write;

    let parent = path.parent().ok_or_else(|| {
        HyphaError::new(
            error_code,
            format!("Failed to determine parent directory for {}", file_label),
        )
    })?;
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("tmp");
    let tmp_path = parent.join(format!(
        ".{}.tmp.{}.{}",
        file_name,
        std::process::id(),
        crate::time::now_epoch_ms()
    ));

    #[cfg(unix)]
    let mut file = {
        use std::os::unix::fs::OpenOptionsExt;

        std::fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .mode(mode)
            .open(&tmp_path)
            .map_err(|e| {
                HyphaError::new(
                    error_code,
                    format!("Failed to create temp {}: {}", file_label, e),
                )
            })?
    };

    #[cfg(not(unix))]
    let mut file = std::fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&tmp_path)
        .map_err(|e| {
            HyphaError::new(
                error_code,
                format!("Failed to create temp {}: {}", file_label, e),
            )
        })?;

    if let Err(e) = file.write_all(content.as_bytes()) {
        let _ = std::fs::remove_file(&tmp_path);
        return Err(HyphaError::new(
            error_code,
            format!("Failed to write temp {}: {}", file_label, e),
        ));
    }
    if let Err(e) = file.sync_all() {
        let _ = std::fs::remove_file(&tmp_path);
        return Err(HyphaError::new(
            error_code,
            format!("Failed to sync temp {}: {}", file_label, e),
        ));
    }
    drop(file);

    std::fs::rename(&tmp_path, path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp_path);
        HyphaError::new(
            error_code,
            format!("Failed to replace {}: {}", file_label, e),
        )
    })?;

    // fsync the directory so the rename itself survives a crash.
    if let Ok(handle) = std::fs::File::open(parent) {
        let _ = handle.sync_all();
    }
    Ok(())
}

pub fn config_path() -> PathBuf {
    hypha_dir().join("config.toml")
}

/// $CMN_HOME/hypha/
pub fn hypha_dir() -> PathBuf {
    crate::site::get_cmn_home().join("hypha")
}
