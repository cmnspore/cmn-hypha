use std::path::{Path, PathBuf};

use super::HyphaConfig;

impl HyphaConfig {
    pub fn load() -> Result<Self, crate::sink::HyphaError> {
        use crate::sink::HyphaError;

        let path = config_path();
        match std::fs::read_to_string(&path) {
            Ok(content) => toml::from_str(&content).map_err(|e| {
                HyphaError::with_hint(
                    "config_parse_failed",
                    format!("Failed to parse {}: {}", path.display(), e),
                    "fix the file or remove it to use defaults",
                )
            }),
            Err(_) => Ok(Self::default()),
        }
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
        let content = toml::to_string_pretty(self).map_err(|e| {
            HyphaError::new(
                "config_save_failed",
                format!("Failed to serialize config: {}", e),
            )
        })?;
        write_text_file_atomic(&path, &content, 0o600, "config_save_failed", "config.toml")
    }
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
