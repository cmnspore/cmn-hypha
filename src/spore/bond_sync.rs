//! `hatch bond sync` — declarative reconcile of one relation's bonds.
//!
//! Replaces the "diff + loop over `hatch bond set`/`remove`" orchestration that
//! consumers were forced to hand-write (see the `update-follows.sh` reconciler
//! in the agentfirstkit repo, which this command absorbs): given a JSON spec of
//! the desired bonds for one relation, `sync` resolves any missing URIs
//! internally, computes the add/update/remove diff against the current
//! spore.core.json, and (unless `--check`) writes the result through the same
//! `save_draft`/`write_spore_core` path as every other hatch mutation.

use std::io::Read;
use std::process::ExitCode;

use serde::Deserialize;
use serde_json::json;

use substrate::{BondRelation, SporeBond};

use crate::api::Output;
use crate::site::SiteDir;
use crate::HyphaError;

use super::{load_draft, save_draft};

/// Exit code for `--check` when drift is found. Distinct from both the normal
/// error exit (`ExitCode::FAILURE` = 1) and success (0), so CI/release
/// pipelines can gate specifically on "bonds are out of sync" rather than on
/// "sync itself failed to run".
///
/// `docs/error-codes.md`'s Exit Codes table already reserves 3-6 for
/// (currently unimplemented) network/verification/git/permission failures —
/// picking 7 here avoids colliding with that documented-but-not-yet-wired-up
/// meaning.
pub const BOND_SYNC_DRIFT_EXIT_CODE: u8 = 7;

/// One entry of the `--spec` JSON array.
#[derive(Debug, Deserialize)]
struct BondSyncSpecEntry {
    id: String,
    #[serde(default)]
    reason: Option<String>,
    #[serde(default)]
    with: Option<serde_json::Value>,
    /// Escape hatch: an explicit URI skips internal resolution entirely.
    #[serde(default)]
    uri: Option<String>,
}

/// Classification of one spec entry against the existing bonds of the target relation.
enum EntryDiff {
    Add(SporeBond),
    Update(SporeBond),
    Unchanged(SporeBond),
}

impl EntryDiff {
    fn bond(&self) -> &SporeBond {
        match self {
            EntryDiff::Add(b) | EntryDiff::Update(b) | EntryDiff::Unchanged(b) => b,
        }
    }
}

/// Result of reconciling `existing` bonds against a `desired` (spec-ordered,
/// fully-resolved) list for one relation.
struct ReconcileResult {
    /// New flat `bonds` array (all relations, target relation reconciled).
    new_bonds: Vec<SporeBond>,
    /// Per spec-entry classification, in spec order.
    per_entry: Vec<EntryDiff>,
    /// Existing target-relation bonds with no matching spec entry.
    to_remove: Vec<SporeBond>,
}

/// Reconcile `existing` bonds so that `target_relation`'s bonds exactly equal
/// `desired` (in spec order), leaving every other relation untouched.
///
/// Matching (which existing bond, if any, a spec entry corresponds to) reuses
/// `hatch bond set`'s own rule: match by `uri` first (across *all* existing
/// bonds, any relation — same as `set`'s upsert-by-uri), else by
/// `(target_relation, id)`. A cross-relation URI match is a deliberate edge
/// case inherited from `set`'s semantics: if a spec entry's URI happens to
/// already exist as a bond of a *different* relation, that bond is "claimed"
/// into the target relation group rather than left in place — sync treats URI
/// as the bond's true identity, same as `set` does.
///
/// Array placement: the reconciled target-relation group is spliced in
/// *contiguously* at the position of the first pre-existing bond of
/// `target_relation` in `existing` (whether or not that particular bond ends
/// up matched, updated, or removed — its position is only used as a slot
/// marker). If `existing` has no bonds of `target_relation` at all, the group
/// is appended at the end. All other (non-matched, non-target) bonds keep
/// their original relative order around the group.
fn reconcile_bonds(
    existing: &[SporeBond],
    target_relation: &BondRelation,
    desired: Vec<SporeBond>,
) -> ReconcileResult {
    let mut consumed = vec![false; existing.len()];
    let mut match_idx = Vec::with_capacity(desired.len());

    for bond in &desired {
        let idx = existing
            .iter()
            .enumerate()
            .find(|(i, b)| !consumed[*i] && b.uri == bond.uri)
            .map(|(i, _)| i)
            .or_else(|| {
                existing
                    .iter()
                    .enumerate()
                    .find(|(i, b)| {
                        !consumed[*i]
                            && &b.relation == target_relation
                            && b.id.as_deref() == bond.id.as_deref()
                    })
                    .map(|(i, _)| i)
            });
        if let Some(i) = idx {
            consumed[i] = true;
        }
        match_idx.push(idx);
    }

    let per_entry: Vec<EntryDiff> = desired
        .iter()
        .zip(match_idx.iter())
        .map(|(bond, idx)| match idx {
            None => EntryDiff::Add(bond.clone()),
            Some(i) if existing[*i] == *bond => EntryDiff::Unchanged(bond.clone()),
            Some(_) => EntryDiff::Update(bond.clone()),
        })
        .collect();

    let to_remove: Vec<SporeBond> = existing
        .iter()
        .enumerate()
        .filter(|(i, b)| !consumed[*i] && &b.relation == target_relation)
        .map(|(_, b)| b.clone())
        .collect();

    // Splice the whole reconciled group in at the position of the first
    // pre-existing target-relation bond (or append if there was none).
    let insertion_index = existing.iter().position(|b| &b.relation == target_relation);
    let mut new_bonds = Vec::with_capacity(existing.len() + desired.len());
    let mut inserted = false;
    for (i, bond) in existing.iter().enumerate() {
        if Some(i) == insertion_index {
            new_bonds.extend(desired.iter().cloned());
            inserted = true;
        }
        if consumed[i] {
            // Represented in the desired group already (matched by uri or id).
            continue;
        }
        if &bond.relation == target_relation {
            // Pre-existing target bond with no spec match — dropped (to_remove).
            continue;
        }
        new_bonds.push(bond.clone());
    }
    if !inserted {
        new_bonds.extend(desired.iter().cloned());
    }

    ReconcileResult {
        new_bonds,
        per_entry,
        to_remove,
    }
}

/// Read `--spec` from a file path, or stdin when the value is `"-"`.
fn read_spec_source(spec: &str) -> Result<String, HyphaError> {
    if spec == "-" {
        let mut buf = String::new();
        std::io::stdin().read_to_string(&mut buf).map_err(|e| {
            HyphaError::new(
                "bond_spec_read_failed",
                format!("Failed to read --spec from stdin: {}", e),
            )
        })?;
        Ok(buf)
    } else {
        std::fs::read_to_string(spec).map_err(|e| {
            HyphaError::new(
                "bond_spec_read_failed",
                format!("Failed to read --spec file '{}': {}", spec, e),
            )
        })
    }
}

/// Resolve an entry's URI when the spec omits one.
///
/// Resolution order (matches the `update-follows.sh` consumer script this
/// command absorbs, `get_uri()`: deployed-first, dry-run fallback):
///   1. Deployed inventory — look up `id` in `--domain`'s local mycelium
///      inventory (what `mycelium status --domain <D> --id <id>` reads).
///   2. Not-yet-deployed fallback — compute the URI the sibling spore would
///      get if released right now, via the exact same computation
///      `release --dry-run` uses (see [`super::release::build_release_plan`]),
///      run against `<parent of cwd>/<id>` as the sibling's source directory.
///      This assumes the monorepo convention of sibling spore sources living
///      next to each other (`spores/<id>/` — the same convention this very
///      repo and hypha's own `spawn` default directory follow).
///
/// NOTE: both paths read *live* state (the deployed inventory, and the
/// sibling's *current* working tree for the dry-run hash) — resolution is
/// only idempotent relative to a fixed deploy point. Run `release` for all
/// siblings before `sync`, not interleaved with it.
fn resolve_bond_uri(
    entry_id: &str,
    domain: Option<&str>,
    site_path: Option<&str>,
) -> Result<String, HyphaError> {
    let domain = domain.ok_or_else(|| {
        HyphaError::with_hint(
            "missing_domain",
            format!(
                "Spec entry '{}' has no \"uri\" and --domain was not given for resolution",
                entry_id
            ),
            "pass --domain (and --site-path for a non-default site) so sync can resolve URIs, or give an explicit \"uri\" for this entry",
        )
    })?;

    let site = SiteDir::from_args(domain, site_path);

    // 1. Deployed inventory (mycelium) — fast path for already-released siblings.
    if site.exists() {
        if let Some(hash) = crate::mycelium::find_local_spore_hash(&site, domain, entry_id) {
            return Ok(substrate::build_spore_uri(domain, &hash));
        }
    }

    // 2. `release --dry-run` fallback for not-yet-deployed siblings.
    let cwd = std::env::current_dir().map_err(|e| {
        HyphaError::new(
            "dir_error",
            format!("Failed to get working directory: {}", e),
        )
    })?;
    let sibling_dir = cwd
        .parent()
        .map(|p| p.join(entry_id))
        .filter(|p| p.join("spore.core.json").exists());

    let Some(sibling_dir) = sibling_dir else {
        return Err(HyphaError::with_hint(
            "bond_uri_unresolved",
            format!(
                "Could not resolve URI for spec entry '{}': not found in {}'s mycelium inventory, and no sibling spore.core.json at ../{}",
                entry_id, domain, entry_id
            ),
            "release the dependency first, or give an explicit \"uri\" for this entry",
        ));
    };

    super::release::build_release_plan(domain, site_path, sibling_dir, crate::time::now_epoch_ms())
        .map(|plan| plan.uri)
        .map_err(|e| {
            HyphaError::with_hint(
                "bond_uri_unresolved",
                format!(
                    "Could not compute dry-run URI for spec entry '{}': {}",
                    entry_id, e.message
                ),
                "release the dependency first, or give an explicit \"uri\" for this entry",
            )
        })
}

pub fn handle_bond_sync(
    out: &Output,
    relation: BondRelation,
    spec: &str,
    domain: Option<String>,
    site_path: Option<String>,
    check: bool,
) -> ExitCode {
    let spec_content = match read_spec_source(spec) {
        Ok(c) => c,
        Err(e) => return out.error_hypha(&e),
    };

    let entries: Vec<BondSyncSpecEntry> = match serde_json::from_str(&spec_content) {
        Ok(v) => v,
        Err(e) => {
            return out.error_hint(
                "bond_spec_invalid",
                &format!("Invalid --spec JSON: {}", e),
                Some("--spec must be a JSON array of {id, reason?, with?, uri?} objects, or \"-\" to read stdin"),
            )
        }
    };

    for entry in &entries {
        if entry.id.trim().is_empty() {
            return out.error(
                "bond_spec_invalid",
                "Each spec entry must have a non-empty \"id\"",
            );
        }
    }

    let mut desired = Vec::with_capacity(entries.len());
    for entry in &entries {
        let uri = match entry.uri.as_deref() {
            Some(u) if !u.is_empty() => u.to_string(),
            _ => match resolve_bond_uri(&entry.id, domain.as_deref(), site_path.as_deref()) {
                Ok(u) => u,
                Err(e) => return out.error_hypha(&e),
            },
        };
        desired.push(SporeBond {
            relation: relation.clone(),
            uri,
            id: Some(entry.id.clone()),
            reason: entry.reason.clone(),
            with: entry.with.clone(),
        });
    }

    let (spore_core_path, mut draft) = match load_draft() {
        Ok(v) => v,
        Err((code, msg)) => return out.error(&code, &msg),
    };

    let ReconcileResult {
        new_bonds,
        per_entry,
        to_remove,
    } = reconcile_bonds(&draft.bonds, &relation, desired);

    let to_add: Vec<&SporeBond> = per_entry
        .iter()
        .filter(|e| matches!(e, EntryDiff::Add(_)))
        .map(EntryDiff::bond)
        .collect();
    let to_update: Vec<&SporeBond> = per_entry
        .iter()
        .filter(|e| matches!(e, EntryDiff::Update(_)))
        .map(EntryDiff::bond)
        .collect();
    let unchanged: Vec<&SporeBond> = per_entry
        .iter()
        .filter(|e| matches!(e, EntryDiff::Unchanged(_)))
        .map(EntryDiff::bond)
        .collect();

    if check {
        let drift = !to_add.is_empty() || !to_update.is_empty() || !to_remove.is_empty();
        let result = json!({
            "relation": relation.as_str(),
            "drift": drift,
            "to_add": to_add,
            "to_update": to_update,
            "to_remove": to_remove,
            "unchanged": unchanged,
        });
        return if drift {
            out.ok_with_code(result, BOND_SYNC_DRIFT_EXIT_CODE)
        } else {
            out.ok(result)
        };
    }

    // Bonds applied in spec order, other relations untouched (see reconcile_bonds).
    let applied: Vec<&SporeBond> = per_entry
        .iter()
        .filter(|e| !matches!(e, EntryDiff::Unchanged(_)))
        .map(EntryDiff::bond)
        .collect();
    let result = json!({
        "relation": relation.as_str(),
        "applied": applied,
        "removed": to_remove,
        "unchanged": unchanged,
    });

    draft.bonds = new_bonds;
    if let Err(e) = save_draft(&spore_core_path, &draft) {
        return out.error_hypha(&e);
    }

    out.ok(result)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    fn bond(relation: BondRelation, uri: &str, id: &str) -> SporeBond {
        SporeBond {
            relation,
            uri: uri.to_string(),
            id: Some(id.to_string()),
            reason: None,
            with: None,
        }
    }

    #[test]
    fn reconcile_empty_spec_clears_relation_and_keeps_others() {
        let existing = vec![
            bond(BondRelation::Follows, "cmn://d/b3.a", "a"),
            bond(BondRelation::DependsOn, "cmn://d/b3.b", "b"),
        ];
        let result = reconcile_bonds(&existing, &BondRelation::Follows, vec![]);
        assert_eq!(result.new_bonds, vec![existing[1].clone()]);
        assert_eq!(result.to_remove, vec![existing[0].clone()]);
        assert!(result.per_entry.is_empty());
    }

    #[test]
    fn reconcile_is_idempotent_on_second_pass() {
        let existing = vec![bond(BondRelation::DependsOn, "cmn://d/b3.other", "other")];
        let desired = vec![bond(BondRelation::Follows, "cmn://d/b3.a", "a")];
        let first = reconcile_bonds(&existing, &BondRelation::Follows, desired.clone());
        assert_eq!(first.to_remove.len(), 0);
        assert_eq!(first.per_entry.len(), 1);
        assert!(matches!(first.per_entry[0], EntryDiff::Add(_)));

        // Second pass against the bonds produced by the first pass: zero changes.
        let second = reconcile_bonds(&first.new_bonds, &BondRelation::Follows, desired);
        assert_eq!(second.to_remove.len(), 0);
        assert!(matches!(second.per_entry[0], EntryDiff::Unchanged(_)));
        assert_eq!(second.new_bonds, first.new_bonds);
    }

    #[test]
    fn reconcile_places_group_at_first_pre_existing_target_bond() {
        let existing = vec![
            bond(BondRelation::DependsOn, "cmn://d/b3.x", "x"),
            bond(BondRelation::Follows, "cmn://d/b3.old", "old"),
            bond(BondRelation::DependsOn, "cmn://d/b3.y", "y"),
        ];
        let desired = vec![bond(BondRelation::Follows, "cmn://d/b3.new", "new")];
        let result = reconcile_bonds(&existing, &BondRelation::Follows, desired);
        let uris: Vec<&str> = result.new_bonds.iter().map(|b| b.uri.as_str()).collect();
        assert_eq!(uris, vec!["cmn://d/b3.x", "cmn://d/b3.new", "cmn://d/b3.y"]);
    }

    #[test]
    fn reconcile_appends_group_when_no_pre_existing_target_bonds() {
        let existing = vec![bond(BondRelation::DependsOn, "cmn://d/b3.x", "x")];
        let desired = vec![bond(BondRelation::Follows, "cmn://d/b3.new", "new")];
        let result = reconcile_bonds(&existing, &BondRelation::Follows, desired);
        let uris: Vec<&str> = result.new_bonds.iter().map(|b| b.uri.as_str()).collect();
        assert_eq!(uris, vec!["cmn://d/b3.x", "cmn://d/b3.new"]);
    }
}
