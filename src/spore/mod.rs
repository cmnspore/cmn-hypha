use serde::{Deserialize, Serialize};
use std::path::Path;

mod bond_sync;
mod bonds;
mod hatch;
mod release;
mod replicate;
mod updated_at;

use substrate::{SPORE_CORE_SCHEMA, SporeCore, SporeTree};

pub use bond_sync::{BOND_SYNC_DRIFT_EXIT_CODE, handle_bond_sync};
pub use bonds::{handle_bond_clear, handle_bond_remove, handle_bond_set};
pub use hatch::{HatchArgs, handle_hatch, handle_tree_set, handle_tree_show};
pub use release::{ArchiveFormat, ReleaseArgs, handle_release};
pub use replicate::handle_replicate;

fn default_hatch_tree() -> SporeTree {
    SporeTree {
        algorithm: "blob_tree_blake3_nfc".to_string(),
        exclude_names: vec![".git".to_string(), ".cmn".to_string()],
        follow_rules: vec![".gitignore".to_string()],
    }
}

fn create_default_spore_core(working_dir: &Path) -> SporeCore {
    let name = working_dir
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "unnamed".to_string());

    SporeCore {
        id: String::new(),
        version: String::new(),
        name,
        domain: String::new(),
        key: String::new(),
        synopsis: "A CMN Spore".to_string(),
        intent: vec![],
        mutations: vec![],
        size_bytes: 0,
        license: "MIT".to_string(),
        updated_at_epoch_ms: 0,
        bonds: vec![],
        tree: default_hatch_tree(),
    }
}

fn load_draft() -> Result<(std::path::PathBuf, SporeCore), (String, String)> {
    let working_dir = match std::env::current_dir() {
        Ok(d) => d,
        Err(e) => {
            return Err((
                "dir_error".to_string(),
                format!("Failed to get working directory: {}", e),
            ));
        }
    };

    let spore_core_path = working_dir.join("spore.core.json");

    let draft: SporeCore = if spore_core_path.exists() {
        let content = match std::fs::read_to_string(&spore_core_path) {
            Ok(content) => content,
            Err(e) => {
                return Err((
                    "spore_read_failed".to_string(),
                    format!("Failed to read {}: {}", spore_core_path.display(), e),
                ));
            }
        };
        match serde_json::from_str(&content) {
            Ok(draft) => draft,
            Err(e) => {
                return Err((
                    "spore_parse_failed".to_string(),
                    format!("Failed to parse {}: {}", spore_core_path.display(), e),
                ));
            }
        }
    } else {
        create_default_spore_core(&working_dir)
    };

    Ok((spore_core_path, draft))
}

fn save_draft(path: &Path, draft: &SporeCore) -> Result<(), crate::sink::HyphaError> {
    use crate::sink::HyphaError;
    let value = serde_json::to_value(draft)
        .map_err(|e| HyphaError::new("spore_write_failed", format!("serialize error: {}", e)))?;
    write_spore_core(path, &value)
}

/// Write spore.core.json — single canonical output function.
/// All code that writes spore.core.json MUST use this function.
///
/// Ensures `$schema` is present, validates against the spore-core schema,
/// then delegates key ordering to substrate's `format_spore_core_draft`.
pub fn write_spore_core(
    path: &Path,
    value: &serde_json::Value,
) -> Result<(), crate::sink::HyphaError> {
    #[derive(Serialize, Deserialize)]
    struct RawSporeCoreDocument {
        #[serde(rename = "$schema", default)]
        schema: String,
        #[serde(flatten)]
        rest: serde_json::Map<String, serde_json::Value>,
    }

    // Strip release-computed fields that must not appear in spore.core.json
    let mut clean_value = value.clone();
    if let Some(obj) = clean_value.as_object_mut() {
        obj.remove("updated_at_epoch_ms");
        obj.remove("size_bytes");
    }

    use crate::sink::HyphaError;

    // Ensure canonical schema URL is present and strict.
    let mut raw: RawSporeCoreDocument = serde_json::from_value(clean_value)
        .map_err(|e| HyphaError::new("spore_write_failed", format!("serialize error: {}", e)))?;
    if raw.schema.is_empty() {
        raw.schema = SPORE_CORE_SCHEMA.to_string();
    } else if raw.schema != SPORE_CORE_SCHEMA {
        return Err(HyphaError::new(
            "spore_write_failed",
            format!("spore.core.json $schema must be {}", SPORE_CORE_SCHEMA),
        ));
    }
    let with_schema = serde_json::to_value(&raw)
        .map_err(|e| HyphaError::new("spore_write_failed", format!("serialize error: {}", e)))?;

    let schema_type = substrate::validate_schema(&with_schema).map_err(|e| {
        HyphaError::new(
            "schema_error",
            format!("spore.core.json schema validation failed: {}", e),
        )
    })?;
    if schema_type != substrate::SchemaType::SporeCore {
        return Err(HyphaError::new(
            "schema_error",
            format!(
                "spore.core.json must validate as spore-core schema (got {:?})",
                schema_type
            ),
        ));
    }

    let pretty = substrate::format_spore_core_draft(&with_schema).map_err(|e| {
        HyphaError::new(
            "spore_write_failed",
            format!("Failed to format spore.core.json: {}", e),
        )
    })?;
    let temporary_dir = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."))
        .join(".cmn");
    crate::cache::fs::atomic_write_file(path, pretty.as_bytes(), &temporary_dir, Some(0o644))
        .map_err(|e| {
            HyphaError::new(
                "spore_write_failed",
                format!("Failed to write spore.core.json: {}", e),
            )
        })?;
    Ok(())
}

/// Read the spawned_from URI from a saved spore.json at .cmn/spawned-from/spore.json.
fn read_spawned_from_uri(spore_path: &std::path::Path) -> Option<String> {
    let content = std::fs::read_to_string(spore_path).ok()?;
    let spore: substrate::Spore = serde_json::from_str(&content).ok()?;
    Some(spore.uri().to_string())
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {

    use super::*;
    use serde_json::json;

    #[test]
    fn write_spore_core_injects_schema_when_missing() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("spore.core.json");
        let value = json!({
            "id": "cmn-spec",
            "name": "CMN Spec",
            "domain": "cmn.dev",
            "key": "ed25519.5XmkQ9vZP8nL3xJdFtR7wNcA6sY2bKgU1eH9pXb4",
            "synopsis": "Spec",
            "intent": ["initial"],
            "license": "CC0-1.0",
            "mutations": [],
            "bonds": [],
            "tree": { "algorithm": "blob_tree_blake3_nfc", "exclude_names": [], "follow_rules": [] }
        });

        write_spore_core(&path, &value).unwrap();

        let saved: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(
            saved.get("$schema").and_then(|v| v.as_str()),
            Some(SPORE_CORE_SCHEMA)
        );
    }

    #[test]
    fn write_spore_core_rejects_non_canonical_schema_url() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("spore.core.json");
        let value = json!({
            "$schema": "https://cmn.dev/schemas/v1/spore.json#/$defs/spore_core",
            "id": "cmn-spec",
            "name": "CMN Spec",
            "domain": "cmn.dev",
            "synopsis": "Spec",
            "intent": ["initial"],
            "license": "CC0-1.0",
            "tree": { "algorithm": "blob_tree_blake3_nfc" }
        });

        let err = write_spore_core(&path, &value).unwrap_err();
        assert!(err.message.contains("spore.core.json $schema must be"));
    }

    #[test]
    fn draft_staging_stays_out_of_default_tree_hash() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("spore.core.json");
        let mut draft = create_default_spore_core(dir.path());
        draft.domain = "example.com".to_string();
        let value = serde_json::to_value(&draft).unwrap();
        write_spore_core(&path, &value).unwrap();
        assert!(dir.path().join(".cmn").is_dir());
        assert_eq!(
            std::fs::read_dir(dir.path().join(".cmn")).unwrap().count(),
            0
        );

        let before = crate::tree::compute_tree_hash(dir.path(), &draft.tree).unwrap();
        // Simulate a leftover temporary file after a process interruption.
        std::fs::write(dir.path().join(".cmn/.hypha-write-interrupted"), "partial").unwrap();
        write_spore_core(&path, &value).unwrap();
        assert_eq!(
            crate::tree::compute_tree_hash(dir.path(), &draft.tree).unwrap(),
            before
        );
    }

    #[cfg(unix)]
    #[test]
    fn canonical_draft_writer_preserves_mode_and_refuses_symlink() {
        use std::os::unix::fs::{PermissionsExt, symlink};
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("spore.core.json");
        let mut draft = create_default_spore_core(dir.path());
        draft.domain = "example.com".to_string();
        let value = serde_json::to_value(&draft).unwrap();
        write_spore_core(&path, &value).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o640)).unwrap();
        write_spore_core(&path, &value).unwrap();
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o640
        );
        let original = std::fs::read(&path).unwrap();
        let real_path = dir.path().join("real.json");
        std::fs::rename(&path, &real_path).unwrap();
        symlink(&real_path, &path).unwrap();
        assert_eq!(
            write_spore_core(&path, &value).unwrap_err().code,
            "spore_write_failed"
        );
        assert!(
            std::fs::symlink_metadata(&path)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert_eq!(std::fs::read(&real_path).unwrap(), original);
    }
}
