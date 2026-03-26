use std::process::ExitCode;

use crate::api::Output;
use substrate::{BondRelation, SporeBond};

use super::{load_draft, save_draft};

pub fn handle_bond_set(
    out: &Output,
    uri: &str,
    relation: Option<BondRelation>,
    id: Option<String>,
    reason: Option<String>,
    with_entries: Vec<String>,
) -> ExitCode {
    let (spore_core_path, mut draft) = match load_draft() {
        Ok(v) => v,
        Err((code, msg)) => return out.error(&code, &msg),
    };

    let with_update = if with_entries.is_empty() {
        None
    } else {
        let mut obj = serde_json::Map::new();
        for entry in &with_entries {
            let Some((key, val_str)) = entry.split_once('=') else {
                return out.error_hint(
                    "invalid_args",
                    &format!("Invalid --with format: '{}'", entry),
                    Some("expected format: KEY=VALUE"),
                );
            };
            let value = serde_json::from_str(val_str)
                .unwrap_or_else(|_| serde_json::Value::String(val_str.to_string()));
            obj.insert(key.to_string(), value);
        }
        Some(serde_json::Value::Object(obj))
    };

    let existing_idx =
        draft
            .bonds
            .iter()
            .position(|r| r.uri == uri)
            .or_else(|| match (&relation, &id) {
                (Some(rel), Some(bond_id)) => draft
                    .bonds
                    .iter()
                    .position(|r| r.relation == *rel && r.id.as_deref() == Some(bond_id)),
                _ => None,
            });
    if let Some(idx) = existing_idx {
        let existing = &mut draft.bonds[idx];
        existing.uri = uri.to_string();
        if let Some(rel) = relation {
            existing.relation = rel;
        }
        if id.is_some() {
            existing.id = id;
        }
        if reason.is_some() {
            existing.reason = reason;
        }
        if let Some(new_with) = with_update {
            if let Some(serde_json::Value::Object(ref mut existing_obj)) = existing.with {
                if let serde_json::Value::Object(new_obj) = new_with {
                    for (k, v) in new_obj {
                        existing_obj.insert(k, v);
                    }
                }
            } else {
                existing.with = Some(new_with);
            }
        }
    } else {
        let Some(rel) = relation else {
            return out.error(
                "invalid_args",
                &format!(
                    "Bond with URI '{}' not found. --relation is required when creating a new bond",
                    uri
                ),
            );
        };
        draft.bonds.push(SporeBond {
            uri: uri.to_string(),
            relation: rel,
            id,
            reason,
            with: with_update,
        });
    }

    if let Err(e) = save_draft(&spore_core_path, &draft) {
        return out.error("write_error", &e);
    }

    out.ok(&draft)
}

pub fn handle_bond_remove(
    out: &Output,
    uri: Option<String>,
    relation: Option<BondRelation>,
) -> ExitCode {
    if uri.is_none() && relation.is_none() {
        return out.error(
            "invalid_args",
            "At least one of --uri or --relation is required",
        );
    }

    let (spore_core_path, mut draft) = match load_draft() {
        Ok(v) => v,
        Err((code, msg)) => return out.error(&code, &msg),
    };

    draft.bonds.retain(|r| {
        let uri_match = uri.as_ref().is_some_and(|u| r.uri == *u);
        let rel_match = relation.as_ref().is_some_and(|rel| r.relation == *rel);
        if uri.is_some() && relation.is_some() {
            !(uri_match && rel_match)
        } else if uri.is_some() {
            !uri_match
        } else {
            !rel_match
        }
    });

    if let Err(e) = save_draft(&spore_core_path, &draft) {
        return out.error("write_error", &e);
    }

    out.ok(&draft)
}

pub fn handle_bond_clear(out: &Output) -> ExitCode {
    let (spore_core_path, mut draft) = match load_draft() {
        Ok(v) => v,
        Err((code, msg)) => return out.error(&code, &msg),
    };

    draft.bonds.clear();

    if let Err(e) = save_draft(&spore_core_path, &draft) {
        return out.error("write_error", &e);
    }

    out.ok(&draft)
}
