use serde::{Deserialize, Serialize};
use std::path::Path;

mod bonds;
mod hatch;
mod release;
mod replicate;
mod updated_at;

use substrate::{SporeCore, SporeTree, SPORE_CORE_SCHEMA};

pub use bonds::{handle_bond_clear, handle_bond_remove, handle_bond_set};
pub use hatch::{handle_hatch, handle_tree_set, handle_tree_show, HatchArgs};
pub use release::{handle_release, ArchiveFormat, ReleaseArgs};
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
            ))
        }
    };

    let spore_core_path = working_dir.join("spore.core.json");

    let draft: SporeCore = if spore_core_path.exists() {
        match std::fs::read_to_string(&spore_core_path) {
            Ok(content) => serde_json::from_str(&content)
                .unwrap_or_else(|_| create_default_spore_core(&working_dir)),
            Err(_) => create_default_spore_core(&working_dir),
        }
    } else {
        create_default_spore_core(&working_dir)
    };

    Ok((spore_core_path, draft))
}

fn save_draft(path: &Path, draft: &SporeCore) -> Result<(), String> {
    let value = serde_json::to_value(draft).map_err(|e| format!("serialize error: {}", e))?;
    write_spore_core(path, &value)
}

/// Write spore.core.json — single canonical output function.
/// All code that writes spore.core.json MUST use this function.
///
/// Ensures `$schema` is present, validates against the spore-core schema,
/// then delegates key ordering to substrate's `format_spore_core_draft`.
pub fn write_spore_core(path: &Path, value: &serde_json::Value) -> Result<(), String> {
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

    // Ensure canonical schema URL is present and strict.
    let mut raw: RawSporeCoreDocument =
        serde_json::from_value(clean_value).map_err(|e| format!("serialize error: {}", e))?;
    if raw.schema.is_empty() {
        raw.schema = SPORE_CORE_SCHEMA.to_string();
    } else if raw.schema != SPORE_CORE_SCHEMA {
        return Err(format!(
            "spore.core.json $schema must be {}",
            SPORE_CORE_SCHEMA
        ));
    }
    let with_schema = serde_json::to_value(&raw).map_err(|e| format!("serialize error: {}", e))?;

    let schema_type = substrate::validate_schema(&with_schema)
        .map_err(|e| format!("spore.core.json schema validation failed: {}", e))?;
    if schema_type != substrate::SchemaType::SporeCore {
        return Err(format!(
            "spore.core.json must validate as spore-core schema (got {:?})",
            schema_type
        ));
    }

    let pretty = substrate::format_spore_core_draft(&with_schema)
        .map_err(|e| format!("Failed to format spore.core.json: {}", e))?;
    std::fs::write(path, &pretty).map_err(|e| format!("Failed to write spore.core.json: {}", e))?;
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
        assert!(err.contains("spore.core.json $schema must be"));
    }
}
