use serde_json::json;
use std::path::Path;
use std::process::ExitCode;

use crate::api::Output;
use crate::auth;
use crate::site::{self, SiteDir};
use substrate::{
    BondRelation, PrettyJson, Spore, SporeBond, SporeCapsule, SporeCore, SPORE_CORE_SCHEMA,
    SPORE_SCHEMA,
};

use super::read_spawned_from_uri;
use super::updated_at::compute_updated_at_ms;

/// Archive compression format
#[derive(Debug, Clone, Copy)]
pub enum ArchiveFormat {
    Zstd,
}

impl ArchiveFormat {
    pub(crate) fn from_str(s: &str) -> Result<Self, crate::sink::HyphaError> {
        match s.to_lowercase().as_str() {
            "zstd" | "zst" => Ok(Self::Zstd),
            _ => Err(crate::sink::HyphaError::new(
                "invalid_args",
                format!(
                    "Unsupported archive format for release generation: {}. Use: zstd",
                    s
                ),
            )),
        }
    }

    pub(crate) fn extension(&self) -> &'static str {
        match self {
            Self::Zstd => "tar.zst",
        }
    }
}

pub struct ReleaseArgs<'a> {
    pub domain: &'a str,
    pub source: Option<String>,
    pub site_path: Option<&'a str>,
    pub dist_git: Option<String>,
    pub dist_ref: Option<String>,
    pub archive: &'a str,
    pub dry_run: bool,
}

pub fn handle_release(out: &Output, args: ReleaseArgs<'_>) -> ExitCode {
    let ReleaseArgs {
        domain,
        source,
        site_path,
        dist_git,
        dist_ref,
        archive,
        dry_run,
    } = args;
    let now_epoch_ms = crate::time::now_epoch_ms();

    if site_path.is_none() {
        if let Err(e) = site::validate_site_domain_path(domain) {
            return out.error_hypha(&e);
        }
    }

    // Parse archive format
    let archive_format = match ArchiveFormat::from_str(archive) {
        Ok(f) => f,
        Err(e) => return out.error_hypha(&e),
    };

    // Validate distribution options
    if dist_git.is_some() && dist_ref.is_none() {
        return out.error("invalid_args", "--dist-git requires --dist-ref");
    }

    if dist_git.is_none() && dist_ref.is_some() {
        return out.error("invalid_args", "--dist-ref requires --dist-git");
    }

    let site = SiteDir::from_args(domain, site_path);
    if !site.exists() {
        return out.error_hint(
            "NO_SITE",
            &format!("Site not found at {}", site.root.display()),
            Some(&format!("run: hypha mycelium root --domain {}", domain)),
        );
    }

    let working_dir = match source
        .map(std::path::PathBuf::from)
        .map(Ok)
        .unwrap_or_else(std::env::current_dir)
    {
        Ok(d) => d,
        Err(e) => {
            return out.error(
                "dir_error",
                &format!("Failed to get working directory: {}", e),
            )
        }
    };

    let spore_core_path = working_dir.join("spore.core.json");

    if !spore_core_path.exists() {
        return out.error_hint(
            "NO_SPORE",
            &format!("No spore.core.json found at {}", working_dir.display()),
            Some("run: hypha hatch"),
        );
    }

    let draft_content = match std::fs::read_to_string(&spore_core_path) {
        Ok(c) => c,
        Err(e) => {
            return out.error(
                "read_error",
                &format!("Failed to read spore.core.json: {}", e),
            )
        }
    };

    let draft_value: serde_json::Value = match serde_json::from_str(&draft_content) {
        Ok(v) => v,
        Err(e) => return out.error("parse_error", &format!("Invalid spore.core.json: {}", e)),
    };
    let schema_type = match substrate::validate_schema(&draft_value) {
        Ok(t) => t,
        Err(e) => {
            return out.error(
                "schema_error",
                &format!("spore.core.json schema validation failed: {}", e),
            )
        }
    };
    if schema_type != substrate::SchemaType::SporeCore {
        return out.error(
            "schema_error",
            &format!("spore.core.json must use {}", SPORE_CORE_SCHEMA),
        );
    }
    // Reject runtime-only fields that must not appear in spore.core.json
    if draft_value.get("updated_at_epoch_ms").is_some() {
        return out.error_hint(
            "INVALID_FIELD",
            "spore.core.json must not contain updated_at_epoch_ms (computed at release time)",
            Some("run: hypha hatch   (hatch removes this field automatically)"),
        );
    }

    let draft: SporeCore = match serde_json::from_value(draft_value) {
        Ok(d) => d,
        Err(e) => return out.error("parse_error", &format!("Invalid spore.core.json: {}", e)),
    };

    // Validate domain in spore.core.json matches --domain
    if draft.domain.is_empty() {
        return out.error_hint(
            "DOMAIN_EMPTY",
            "spore.core.json domain is empty",
            Some(&format!("run: hypha hatch --domain {}", domain)),
        );
    }
    if draft.domain != domain {
        return out.error_hint(
            "DOMAIN_MISMATCH",
            &format!(
                "spore.core.json domain '{}' does not match --domain '{}'",
                draft.domain, domain
            ),
            Some(&format!("run: hypha hatch --domain {}", domain)),
        );
    }

    // Get public key from site identity
    let public_key = match auth::get_identity_with_site(domain, &site) {
        Ok(info) => info.public_key,
        Err(e) => return out.error_from("identity_error", &e),
    };

    // Validate key in spore.core.json matches domain identity
    if draft.key.is_empty() {
        return out.error_hint(
            "KEY_EMPTY",
            "spore.core.json key is empty",
            Some(&format!("run: hypha hatch --domain {}", domain)),
        );
    }
    if draft.key != public_key {
        return out.error_hint(
            "KEY_MISMATCH",
            &format!(
                "Key in spore.core.json does not match domain '{}' (key may have rotated)",
                domain
            ),
            Some(&format!("run: hypha hatch --domain {}", domain)),
        );
    }

    // Build bonds: start from spore.core.json (schema guarantees no spawned_from),
    // then add spawned_from from .cmn/spawned-from/spore.json if present.
    let mut release_bonds: Vec<SporeBond> = draft.bonds.clone();
    let spawned_from_spore_path = working_dir
        .join(".cmn")
        .join("spawned-from")
        .join("spore.json");
    if let Some(parent_uri) = read_spawned_from_uri(&spawned_from_spore_path) {
        release_bonds.push(SporeBond {
            uri: parent_uri,
            relation: BondRelation::SpawnedFrom,
            id: None,
            reason: None,
            with: None,
        });
    }

    // 1. Check for symlinks (not supported in spore content), then walk tree
    if let Err(e) = crate::tree::check_no_symlinks(
        &working_dir,
        &draft.tree.exclude_names,
        &draft.tree.follow_rules,
    ) {
        return out.error("SYMLINK_ERR", &format!("{}", e));
    }
    let entries = match crate::tree::walk_dir(
        &working_dir,
        &draft.tree.exclude_names,
        &draft.tree.follow_rules,
    ) {
        Ok(e) => e,
        Err(e) => return out.error("HASH_ERR", &format!("Failed to walk directory: {}", e)),
    };
    let (tree_hash, size_bytes) = match draft.tree.compute_hash_and_size(&entries) {
        Ok(v) => v,
        Err(e) => return out.error("HASH_ERR", &format!("Failed to compute tree hash: {}", e)),
    };

    let core = SporeCore {
        id: draft.id.clone(),
        version: draft.version.clone(),
        name: draft.name.clone(),
        domain: domain.to_string(),
        key: public_key,
        synopsis: draft.synopsis.clone(),
        intent: draft.intent.clone(),
        license: draft.license.clone(),
        mutations: draft.mutations.clone(),
        size_bytes,
        bonds: release_bonds,
        tree: draft.tree.clone(),
        updated_at_epoch_ms: match compute_updated_at_ms(
            &working_dir,
            &draft.tree.exclude_names,
            &draft.tree.follow_rules,
        ) {
            Ok(ms) if ms > 0 => ms,
            _ => now_epoch_ms,
        },
    };

    // 2. Sign core → core_signature
    let core_signature = match auth::sign_json_with_site(&site, &core) {
        Ok(sig) => sig,
        Err(auth::JsonSignError::Jcs(message)) => return out.error("jcs_error", &message),
        Err(auth::JsonSignError::Sign(err)) => return out.error_from("sign_error", &err),
    };

    // 3. Compute URI hash from tree_hash + core + core_signature
    let uri_hash = match (substrate::Spore {
        schema: substrate::SPORE_SCHEMA.to_string(),
        capsule: substrate::SporeCapsule {
            uri: String::new(),
            core: core.clone(),
            core_signature: core_signature.clone(),
            dist: vec![],
        },
        capsule_signature: String::new(),
    })
    .computed_uri_hash_from_tree_hash(&tree_hash)
    {
        Ok(hash) => hash,
        Err(e) => return out.error("jcs_error", &e.to_string()),
    };
    let filename = uri_hash.clone();

    // 4. Build URI
    let uri = format!("cmn://{}/{}", domain, uri_hash);

    // Dry run: return URI without writing anything
    if dry_run {
        return out.ok_trace(
            json!({
                "uri": uri,
                "hash": uri_hash,
            }),
            json!({
                "status": "dry_run",
                "site": site.public.display().to_string(),
            }),
        );
    }

    // 5. Build dist array based on options
    let mut dist: Vec<substrate::SporeDist> = vec![];

    // Scenario A: External git reference
    if let (Some(git_url), Some(git_ref)) = (&dist_git, &dist_ref) {
        dist.push(substrate::SporeDist {
            kind: substrate::DistKind::Git,
            filename: None,
            url: Some(git_url.clone()),
            git_ref: Some(git_ref.clone()),
            cid: None,
            extra: Default::default(),
        });
    }

    // Scenario B: Archive (always generated) + optional delta
    {
        let mut files = substrate::flatten_entries(&entries);

        let archive_filename = format!("{}.{}", filename, archive_format.extension());
        let archive_dir = site.archive_dir();
        if let Err(e) = std::fs::create_dir_all(&archive_dir) {
            return out.error("dir_error", &format!("Failed to create archive dir: {}", e));
        }
        let archive_path = archive_dir.join(&archive_filename);

        // Create archive directly from in-memory file list (optimized)
        if let Err(e) = create_archive_from_files(&mut files, &archive_path, archive_format) {
            return out.error("archive_error", &format!("Failed to create archive: {}", e));
        }

        // Generate delta archive if previous version exists.
        if let Some(old_hash) = find_previous_hash(&site, domain, &draft.id) {
            if old_hash != uri_hash {
                let old_archive_path = archive_dir.join(format!("{}.tar.zst", old_hash));
                if old_archive_path.exists() {
                    match generate_delta_archive(
                        &mut files,
                        &old_archive_path,
                        &archive_dir,
                        &uri_hash,
                        &old_hash,
                    ) {
                        Ok(_delta_filename) => {}
                        Err(e) => {
                            // Delta is optional — warn but continue
                            out.warn(
                                "DELTA_WARN",
                                &format!("Failed to generate delta archive: {}", e),
                            );
                        }
                    }
                }
            }
        }

        dist.push(substrate::SporeDist {
            kind: substrate::DistKind::Archive,
            filename: None,
            url: None,
            git_ref: None,
            cid: None,
            extra: Default::default(),
        });
    }

    // 6. Build capsule with dist
    let capsule = SporeCapsule {
        uri: uri.clone(),
        core,
        core_signature,
        dist,
    };

    // 7. Sign capsule → capsule_signature
    let capsule_signature = match auth::sign_json_with_site(&site, &capsule) {
        Ok(sig) => sig,
        Err(auth::JsonSignError::Jcs(message)) => return out.error("jcs_error", &message),
        Err(auth::JsonSignError::Sign(err)) => return out.error_from("sign_error", &err),
    };

    // 8. Check if spore already exists with same hash
    let spore_manifest_path = site.spores_dir().join(format!("{}.json", filename));

    if spore_manifest_path.exists() {
        // Spore with same hash already exists - skip
        let existing_json = match std::fs::read_to_string(&spore_manifest_path) {
            Ok(j) => j,
            Err(e) => {
                return out.error(
                    "read_error",
                    &format!("Spore manifest exists but cannot be read: {}", e),
                )
            }
        };
        let existing: serde_json::Value = match serde_json::from_str(&existing_json) {
            Ok(v) => v,
            Err(e) => {
                return out.error(
                    "parse_error",
                    &format!("Spore manifest exists but is invalid JSON: {}", e),
                )
            }
        };

        // Still save to .cmn/spawned-from/ so next release knows the parent
        if let Some(parent) = spawned_from_spore_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = std::fs::write(&spawned_from_spore_path, &existing_json);

        let data = json!({
            "uri": uri,
            "hash": uri_hash,
            "spore": existing,
        });
        let hypha = json!({
            "status": "skipped",
            "site": site.public.display().to_string(),
        });

        return out.ok_trace(&data, hypha);
    }

    // 9. Build complete spore manifest
    let spore_manifest = Spore {
        schema: SPORE_SCHEMA.to_string(),
        capsule,
        capsule_signature,
    };

    // Validate against schema before writing
    let spore_value = match serde_json::to_value(&spore_manifest) {
        Ok(v) => v,
        Err(e) => return out.error("serialize_error", &e.to_string()),
    };
    if let Err(e) = substrate::validate_schema(&spore_value) {
        return out.error(
            "schema_error",
            &format!("Spore schema validation failed: {}", e),
        );
    }

    let spore_json = match spore_manifest.to_pretty_json() {
        Ok(j) => j,
        Err(e) => {
            return out.error(
                "serialize_error",
                &format!("Failed to format spore manifest: {}", e),
            )
        }
    };

    if let Err(e) = std::fs::write(&spore_manifest_path, &spore_json) {
        return out.error(
            "write_error",
            &format!("Failed to write spore manifest: {}", e),
        );
    }

    // Update cmn.json inventory
    if let Err(e) = crate::mycelium::update_inventory(
        &site,
        domain,
        &draft.id,
        &uri_hash,
        &draft.name,
        Some(&draft.synopsis),
        now_epoch_ms,
    ) {
        return out.error(
            "INVENTORY_ERR",
            &format!("Failed to update cmn.json: {}", e),
        );
    }

    // 10. Save released spore to .cmn/spawned-from/spore.json
    //     Next release will read this to set spawned_from.
    //     spore.core.json is NOT modified — no git diff noise.
    {
        if let Some(parent) = spawned_from_spore_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = std::fs::write(&spawned_from_spore_path, &spore_json);
    }

    let data = json!({
        "uri": uri,
        "hash": uri_hash,
        "spore": spore_manifest,
    });
    let hypha = json!({
        "status": "released",
        "site": site.public.display().to_string(),
    });

    out.ok_trace(&data, hypha)
}

/// Create archive directly from in-memory file list (optimized - no temp directory)
fn create_archive_from_files(
    files: &mut [(String, Vec<u8>, bool)],
    output_path: &Path,
    _format: ArchiveFormat,
) -> anyhow::Result<()> {
    create_tar_archive_from_files(files, output_path)
}

/// Build raw (uncompressed) tar bytes from in-memory file list (reproducible: deterministic headers)
pub(crate) fn build_raw_tar_bytes(
    files: &mut [(String, Vec<u8>, bool)],
) -> anyhow::Result<Vec<u8>> {
    // Sort by path for deterministic order
    files.sort_by(|a, b| a.0.cmp(&b.0));

    let mut buf = Vec::new();
    {
        let mut tar = tar::Builder::new(&mut buf);
        for (rel_path, content, is_executable) in files.iter() {
            let mut header = tar::Header::new_gnu();
            header.set_size(content.len() as u64);
            header.set_mode(if *is_executable { 0o755 } else { 0o644 });
            header.set_mtime(0);
            header.set_uid(0);
            header.set_gid(0);
            let _ = header.set_username("");
            let _ = header.set_groupname("");
            header.set_cksum();
            tar.append_data(&mut header, rel_path.as_str(), content.as_slice())?;
        }
        tar.finish()?;
    }
    Ok(buf)
}

/// Create tar archive from in-memory file list (reproducible: deterministic headers)
fn create_tar_archive_from_files(
    files: &mut [(String, Vec<u8>, bool)],
    output_path: &Path,
) -> anyhow::Result<()> {
    let raw_tar = build_raw_tar_bytes(files)?;
    let compressed =
        substrate::archive::encode_zstd(&raw_tar, 19).map_err(|e| anyhow::anyhow!("{}", e))?;
    std::fs::write(output_path, &compressed)?;
    Ok(())
}

/// Look up the previous hash for a spore id from the mycelium inventory.
fn find_previous_hash(site: &SiteDir, _domain: &str, spore_id: &str) -> Option<String> {
    let manifest_path = site.cmn_json_path();
    let cmn_content = std::fs::read_to_string(&manifest_path).ok()?;
    let entry: substrate::CmnEntry = serde_json::from_str(&cmn_content).ok()?;

    let mycelium_hash = entry.primary_capsule().ok()?.mycelium_hash()?.to_string();
    let mycelium_path = site.mycelium_dir().join(format!("{}.json", mycelium_hash));
    let mycelium_content = std::fs::read_to_string(&mycelium_path).ok()?;
    let mycelium: substrate::Mycelium = serde_json::from_str(&mycelium_content).ok()?;

    mycelium
        .capsule
        .core
        .spores
        .iter()
        .find(|s| {
            if s.id.is_empty() {
                // Legacy: match by name for spores without id
                false
            } else {
                s.id == spore_id
            }
        })
        .map(|s| s.hash.clone())
}

/// Generate a delta archive using zstd dictionary compression.
/// Returns the delta filename on success.
fn generate_delta_archive(
    files: &mut [(String, Vec<u8>, bool)],
    old_archive_path: &Path,
    archive_dir: &Path,
    new_hash: &str,
    old_hash: &str,
) -> anyhow::Result<String> {
    // Build new raw tar
    let new_raw_tar = build_raw_tar_bytes(files)?;

    // Decompress old archive to get raw tar (dictionary)
    let old_compressed = std::fs::read(old_archive_path)?;
    let old_raw_tar = substrate::archive::decode_zstd(&old_compressed, 512 * 1024 * 1024)
        .map_err(|e| anyhow::anyhow!("{}", e))?;

    // Create delta using dictionary compression
    let delta_filename = format!("{}.from.{}.tar.zst", new_hash, old_hash);
    let delta_path = archive_dir.join(&delta_filename);

    let compressed = substrate::archive::encode_zstd_with_dict(&new_raw_tar, &old_raw_tar, 19)
        .map_err(|e| anyhow::anyhow!("{}", e))?;
    std::fs::write(&delta_path, &compressed)?;

    Ok(delta_filename)
}
