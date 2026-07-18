//! Post-publish lifecycle for spores already in the mycelium inventory:
//! `yank` (delist, keep bytes) and `unyank` (re-add from the kept manifest).
//!
//! Adding a spore is deliberately NOT here — that only happens as a side effect
//! of `hypha release` / `hypha replicate`, because an inventory entry's hash
//! must correspond to a real signed spore capsule.

use std::path::PathBuf;
use std::process::ExitCode;

use serde_json::json;
use substrate::{CmnEndpoint, Mycelium, Spore};

use crate::api::Output;
use crate::site::{self, SiteDir};
use crate::HyphaError;

use super::{load_existing_mycelium, resolve_spore_ref, sign_and_save_mycelium};

/// Locate the target site + load its mycelium, deriving the authoritative
/// domain from the site's own `cmn.json` (`core.domain`) rather than trusting a
/// caller-supplied value. This keeps the re-signed URIs consistent with the
/// site and removes the redundant domain positional the other commands require.
///
/// Site resolution: `--site-path` wins; else `--domain` selects
/// `~/.cmn/mycelium/<domain>`; else the sole site under `~/.cmn/mycelium` is
/// used, and ambiguity (or none) is an error.
fn load_site_for_spore_op(
    domain: Option<&str>,
    site_path: Option<&str>,
) -> Result<(SiteDir, Mycelium, Vec<CmnEndpoint>, String), HyphaError> {
    let site = match (site_path, domain) {
        (Some(path), _) => SiteDir::with_path(PathBuf::from(path)),
        (None, Some(domain)) => {
            site::validate_site_domain_path(domain)?;
            SiteDir::new(domain)
        }
        (None, None) => {
            let domains = site::list_domains();
            match domains.as_slice() {
                [only] => SiteDir::new(only),
                [] => {
                    return Err(HyphaError::with_hint(
                        "no_site",
                        "No mycelium sites found",
                        "run `hypha mycelium root ...` first, or pass --site-path",
                    ))
                }
                _ => {
                    return Err(HyphaError::with_hint(
                        "ambiguous_domain",
                        "Multiple mycelium sites found",
                        "pass --domain <DOMAIN> or --site-path <PATH>",
                    ))
                }
            }
        }
    };

    let (mycelium, endpoints) = load_existing_mycelium(&site).ok_or_else(|| {
        HyphaError::with_hint(
            "no_mycelium",
            "No mycelium found",
            "run: hypha mycelium root --endpoints-base URL",
        )
    })?;
    let domain = mycelium.capsule.core.domain.clone();
    Ok((site, mycelium, endpoints, domain))
}

/// Yank a spore: remove its inventory entry and re-sign the mycelium + cmn.json.
/// Every other spore capsule is untouched, so this is far cheaper than
/// `release --clean-published`. Published bytes are kept by default (existing
/// `cmn://<domain>/<hash>` references keep resolving); `--purge` deletes them.
pub fn handle_spore_yank(
    out: &Output,
    domain: Option<&str>,
    spore_id: &str,
    site_path: Option<&str>,
    purge: bool,
) -> ExitCode {
    let now_epoch_ms = crate::time::now_epoch_ms();

    let (site, mut mycelium, endpoints, domain) = match load_site_for_spore_op(domain, site_path) {
        Ok(v) => v,
        Err(e) => return out.error_hypha(&e),
    };

    // Resolve first: 404 with a helpful hint if the id isn't listed, and
    // capture the hash so --purge knows which files to delete.
    let resolved = match resolve_spore_ref(&domain, spore_id, &mycelium, None) {
        Ok(r) => r,
        Err(e) => return out.error_hypha(&e),
    };
    let hash = resolved.hash;

    mycelium.remove_spore(spore_id, now_epoch_ms);

    let mycelium_hash =
        match sign_and_save_mycelium(&site, &domain, &mut mycelium, endpoints, now_epoch_ms) {
            Ok(h) => h,
            Err(e) => return out.error(e.code(), &e.to_string()),
        };

    let mut purged: Vec<String> = Vec::new();
    if purge {
        let targets = [
            site.spores_dir().join(format!("{hash}.json")),
            site.archive_dir().join(format!("{hash}.tar.zst")),
        ];
        for path in targets {
            if path.exists() {
                if let Err(e) = std::fs::remove_file(&path) {
                    return out.error(
                        "write_error",
                        &format!("Failed to purge {}: {e}", path.display()),
                    );
                }
                purged.push(path.display().to_string());
            }
        }
    }

    out.ok(json!({
        "domain": domain,
        "action": if purge { "spore_yank_purge" } else { "spore_yank" },
        "id": spore_id,
        "hash": hash,
        "mycelium_hash": mycelium_hash,
        "purged": purged,
    }))
}

/// Unyank a previously yanked spore: recover its entry from the kept published
/// manifest and re-add it to the inventory. Fails if the manifest was purged
/// (nothing to restore) or the id is already listed.
pub fn handle_spore_unyank(
    out: &Output,
    domain: Option<&str>,
    spore_id: &str,
    site_path: Option<&str>,
) -> ExitCode {
    let now_epoch_ms = crate::time::now_epoch_ms();

    let (site, mut mycelium, endpoints, domain) = match load_site_for_spore_op(domain, site_path) {
        Ok(v) => v,
        Err(e) => return out.error_hypha(&e),
    };

    if mycelium
        .capsule
        .core
        .spores
        .iter()
        .any(|s| s.id == spore_id)
    {
        return out.error_hint(
            "already_listed",
            &format!("Spore '{spore_id}' is already in the mycelium inventory"),
            Some("nothing to unyank"),
        );
    }

    let found = match find_published_spore_by_id(&site, spore_id) {
        Ok(Some(v)) => v,
        Ok(None) => {
            return out.error_hint(
                "spore_manifest_not_found",
                &format!(
                    "No published manifest for id '{spore_id}' under {}",
                    site.spores_dir().display()
                ),
                Some("it may have been yanked with --purge; re-run `hypha release` to republish"),
            )
        }
        Err(e) => return out.error_hypha(&e),
    };

    mycelium.add_spore(
        spore_id,
        &found.hash,
        &found.name,
        Some(found.synopsis.as_str()),
        now_epoch_ms,
    );

    let mycelium_hash =
        match sign_and_save_mycelium(&site, &domain, &mut mycelium, endpoints, now_epoch_ms) {
            Ok(h) => h,
            Err(e) => return out.error(e.code(), &e.to_string()),
        };

    out.ok(json!({
        "domain": domain,
        "action": "spore_unyank",
        "id": spore_id,
        "hash": found.hash,
        "mycelium_hash": mycelium_hash,
    }))
}

struct FoundManifest {
    hash: String,
    name: String,
    synopsis: String,
    updated_at_epoch_ms: u64,
}

/// Scan the site's published spore manifests for the newest one whose core id
/// matches. Manifests accumulate across releases (they are not pruned), so more
/// than one hash can share an id; the newest by `updated_at_epoch_ms` is the one
/// that was current in the inventory before the yank.
fn find_published_spore_by_id(
    site: &SiteDir,
    spore_id: &str,
) -> Result<Option<FoundManifest>, HyphaError> {
    let dir = site.spores_dir();
    let entries = match std::fs::read_dir(&dir) {
        Ok(entries) => entries,
        Err(_) => return Ok(None),
    };

    let mut best: Option<FoundManifest> = None;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        let Ok(content) = std::fs::read_to_string(&path) else {
            continue;
        };
        let Ok(spore) = serde_json::from_str::<Spore>(&content) else {
            continue;
        };
        if spore.capsule.core.id != spore_id {
            continue;
        }
        let Some(hash) = path.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        let candidate = FoundManifest {
            hash: hash.to_string(),
            name: spore.capsule.core.name,
            synopsis: spore.capsule.core.synopsis,
            updated_at_epoch_ms: spore.capsule.core.updated_at_epoch_ms,
        };
        best = match best {
            Some(current) if current.updated_at_epoch_ms >= candidate.updated_at_epoch_ms => {
                Some(current)
            }
            _ => Some(candidate),
        };
    }
    Ok(best)
}
