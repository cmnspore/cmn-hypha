use super::*;

/// Taste — evaluate spore safety (library level).
///
/// Two modes:
/// - Download mode (verdict=None): fetch spore to cache and return metadata
/// - Record mode (verdict=Some): record a taste verdict for a cached spore
#[allow(clippy::too_many_arguments)]
pub async fn taste(
    uri_str: &str,
    verdict: Option<substrate::TasteVerdict>,
    notes: Option<&str>,
    synapse_url: Option<&str>,
    synapse_token_secret: Option<&str>,
    domain_for_signing: Option<&str>,
    sink: &dyn crate::EventSink,
) -> Result<crate::output::TasteOutput, crate::HyphaError> {
    taste_at(
        uri_str,
        verdict,
        notes,
        synapse_url,
        synapse_token_secret,
        domain_for_signing,
        crate::time::now_epoch_ms(),
        sink,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
async fn taste_at(
    uri_str: &str,
    verdict: Option<substrate::TasteVerdict>,
    notes: Option<&str>,
    synapse_url: Option<&str>,
    synapse_token_secret: Option<&str>,
    domain_for_signing: Option<&str>,
    now_epoch_ms: u64,
    sink: &dyn crate::EventSink,
) -> Result<crate::output::TasteOutput, crate::HyphaError> {
    let uri = CmnUri::parse(uri_str).map_err(|e| crate::HyphaError::new("invalid_uri", e))?;

    match uri.hash.clone() {
        Some(hash) => match verdict {
            Some(v) => taste_record_lib(
                sink,
                uri_str,
                &uri,
                &hash,
                v,
                notes,
                synapse_url,
                synapse_token_secret,
                domain_for_signing,
                now_epoch_ms,
            )
            .await
            .map(crate::output::TasteOutput::Record),
            None => taste_download_lib(
                sink,
                uri_str,
                &uri,
                &hash,
                synapse_url,
                synapse_token_secret,
            )
            .await
            .map(crate::output::TasteOutput::Download),
        },
        None => match verdict {
            Some(v) => taste_domain_record_lib(sink, uri_str, &uri, v, notes, now_epoch_ms)
                .map(crate::output::TasteOutput::Record),
            None => taste_domain_download_lib(sink, uri_str, &uri)
                .await
                .map(crate::output::TasteOutput::Download),
        },
    }
}

async fn taste_download_lib(
    sink: &dyn crate::EventSink,
    uri_str: &str,
    uri: &CmnUri,
    hash: &str,
    synapse_url: Option<&str>,
    synapse_token_secret: Option<&str>,
) -> Result<crate::output::TasteDownloadOutput, crate::HyphaError> {
    let cache = CacheDir::new();
    fetch_spore_to_cache(sink, &cache, uri_str).await?;
    let domain_cache = cache.domain(&uri.domain);
    let spore_path = domain_cache.spore_path(hash);

    let manifest_path = spore_path.join("spore.json");
    let manifest = std::fs::read_to_string(&manifest_path)
        .ok()
        .and_then(|s| serde_json::from_str::<substrate::Spore>(&s).ok());

    let (name, synopsis) = manifest
        .as_ref()
        .map(|spore| {
            (
                spore.capsule.core.name.clone(),
                spore.capsule.core.synopsis.clone(),
            )
        })
        .unwrap_or_default();

    let taste_verdict = domain_cache
        .load_taste(hash)
        .map(|t| crate::output::TasteVerdict {
            verdict: t.verdict,
            notes: t.notes,
            tasted_at_epoch_ms: t.tasted_at_epoch_ms,
        });

    let parent = manifest
        .as_ref()
        .and_then(|spore| spore.spawned_from_uri().map(str::to_string));

    let remote_tastes = if let Some(synapse_arg) = synapse_url {
        match crate::config::resolve_synapse(Some(synapse_arg), synapse_token_secret) {
            Ok(resolved) => {
                match fetch_taste_reports(&resolved.url, hash, resolved.token_secret.as_deref())
                    .await
                {
                    Ok(tastes) => Some(tastes),
                    Err(e) => {
                        sink.emit(crate::HyphaEvent::Warn {
                            message: format!("Failed to fetch taste reports: {}", e),
                        });
                        None
                    }
                }
            }
            Err(e) => {
                sink.emit(crate::HyphaEvent::Warn {
                    message: format!("Failed to resolve synapse: {}", e),
                });
                None
            }
        }
    } else {
        None
    };

    Ok(crate::output::TasteDownloadOutput {
        uri: uri_str.to_string(),
        cache_path: spore_path.display().to_string(),
        name: if name.is_empty() { None } else { Some(name) },
        synopsis: if synopsis.is_empty() {
            None
        } else {
            Some(synopsis)
        },
        parent,
        taste: taste_verdict,
        remote_tastes,
    })
}

#[allow(clippy::too_many_arguments)]
async fn taste_record_lib(
    sink: &dyn crate::EventSink,
    uri_str: &str,
    uri: &CmnUri,
    hash: &str,
    verdict: substrate::TasteVerdict,
    notes: Option<&str>,
    synapse_url: Option<&str>,
    synapse_token_secret: Option<&str>,
    domain_for_signing: Option<&str>,
    now_epoch_ms: u64,
) -> Result<crate::output::TasteRecordOutput, crate::HyphaError> {
    let cache = CacheDir::new();
    let domain_cache = cache.domain(&uri.domain);

    let spore_path = domain_cache.spore_path(hash);
    if !spore_path.exists() {
        return Err(crate::HyphaError::with_hint(
            "NOT_CACHED",
            "Spore not cached",
            format!("run: hypha taste {}", uri_str),
        ));
    }

    let taste_cache = TasteVerdictCache {
        verdict,
        notes: notes.map(|n| n.to_string()),
        tasted_at_epoch_ms: now_epoch_ms,
    };

    domain_cache
        .save_taste(hash, &taste_cache)
        .map_err(|e| crate::HyphaError::new("write_error", e))?;

    let mut output = crate::output::TasteRecordOutput {
        uri: uri_str.to_string(),
        verdict,
        notes: notes.map(|n| n.to_string()),
        tasted_at_epoch_ms: taste_cache.tasted_at_epoch_ms,
        shared: None,
        synapse: None,
        share_error: None,
    };

    // Resolve domain + synapse: CLI args > [taste] config > [defaults] config
    let config = crate::config::HyphaConfig::load();
    let effective_domain = domain_for_signing
        .or(config.defaults.taste.domain.as_deref())
        .or(config.defaults.domain.as_deref());
    let effective_synapse = synapse_url
        .or(config.defaults.taste.synapse.as_deref())
        .or(config.defaults.synapse.as_deref());

    if let (Some(signing_domain), Some(synapse_arg)) = (effective_domain, effective_synapse) {
        match crate::config::resolve_synapse(Some(synapse_arg), synapse_token_secret) {
            Ok(resolved) => {
                match share_taste_report_lib(
                    uri_str,
                    verdict,
                    notes,
                    signing_domain,
                    &resolved.url,
                    resolved.token_secret.as_deref(),
                    now_epoch_ms,
                )
                .await
                {
                    Ok(_) => {
                        output.shared = Some(true);
                        output.synapse = Some(resolved.url);
                    }
                    Err(e) => {
                        sink.emit(crate::HyphaEvent::Warn {
                            message: format!("Failed to share taste report: {}", e),
                        });
                        output.shared = Some(false);
                        output.share_error = Some(e);
                    }
                }
            }
            Err(e) => {
                sink.emit(crate::HyphaEvent::Warn {
                    message: format!("Failed to resolve synapse: {}", e),
                });
                output.shared = Some(false);
                output.share_error = Some(e);
            }
        }
    }

    Ok(output)
}

/// Taste download for domain-only URI: resolve domain, show mycelium info.
async fn taste_domain_download_lib(
    sink: &dyn crate::EventSink,
    uri_str: &str,
    uri: &CmnUri,
) -> Result<crate::output::TasteDownloadOutput, crate::HyphaError> {
    let cache = CacheDir::new();
    let domain_cache = cache.domain(&uri.domain);

    let entry = get_cmn_entry(sink, &domain_cache, cache.cmn_ttl_ms).await?;

    let _capsule = primary_capsule(&entry)
        .map_err(|e| crate::HyphaError::new("manifest_failed", e.message))?;

    let name = entry
        .capsules
        .first()
        .map(|c| c.uri.as_str())
        .unwrap_or(&uri.domain)
        .to_string();

    let taste_verdict = domain_cache
        .load_domain_taste()
        .map(|t| crate::output::TasteVerdict {
            verdict: t.verdict,
            notes: t.notes,
            tasted_at_epoch_ms: t.tasted_at_epoch_ms,
        });

    Ok(crate::output::TasteDownloadOutput {
        uri: uri_str.to_string(),
        cache_path: domain_cache.mycelium_dir().display().to_string(),
        name: Some(name),
        synopsis: None,
        parent: None,
        taste: taste_verdict,
        remote_tastes: None,
    })
}

/// Taste record for domain-only URI: record verdict for the domain.
fn taste_domain_record_lib(
    _sink: &dyn crate::EventSink,
    uri_str: &str,
    uri: &CmnUri,
    verdict: substrate::TasteVerdict,
    notes: Option<&str>,
    now_epoch_ms: u64,
) -> Result<crate::output::TasteRecordOutput, crate::HyphaError> {
    let cache = CacheDir::new();
    let domain_cache = cache.domain(&uri.domain);

    if !domain_cache.mycelium_dir().exists() {
        return Err(crate::HyphaError::with_hint(
            "NOT_CACHED",
            "Domain not cached",
            format!("run: hypha taste {}", uri_str),
        ));
    }

    let taste_cache = TasteVerdictCache {
        verdict,
        notes: notes.map(|n| n.to_string()),
        tasted_at_epoch_ms: now_epoch_ms,
    };

    domain_cache
        .save_domain_taste(&taste_cache)
        .map_err(|e| crate::HyphaError::new("write_error", e))?;

    Ok(crate::output::TasteRecordOutput {
        uri: uri_str.to_string(),
        verdict,
        notes: notes.map(|n| n.to_string()),
        tasted_at_epoch_ms: taste_cache.tasted_at_epoch_ms,
        shared: None,
        synapse: None,
        share_error: None,
    })
}

/// Check taste verdict before operations like spawn/grow/absorb.
///
/// Library-level version: uses `EventSink` for warnings and returns
/// `HyphaError` on failure.
pub fn check_taste(
    sink: &dyn crate::EventSink,
    cache: &CacheDir,
    uri_str: &str,
    domain: &str,
    hash: &str,
) -> Result<(), crate::HyphaError> {
    check_taste_for_operation(
        sink,
        cache,
        uri_str,
        domain,
        hash,
        substrate::GateOperation::Spawn,
    )
}

fn check_taste_for_operation(
    sink: &dyn crate::EventSink,
    cache: &CacheDir,
    uri_str: &str,
    domain: &str,
    hash: &str,
    operation: substrate::GateOperation,
) -> Result<(), crate::HyphaError> {
    let domain_cache = cache.domain(domain);

    let cached_taste = domain_cache.load_taste(hash);
    let verdict = cached_taste.as_ref().map(|taste| taste.verdict);

    match substrate::TasteVerdict::gate_action_for(operation, verdict) {
        substrate::GateAction::Block if cached_taste.is_none() => {
            Err(crate::HyphaError::with_hint(
                "NOT_TASTED",
                "Spore has not been tasted",
                format!(
                    "run: hypha taste {} && hypha taste {} --verdict safe",
                    uri_str, uri_str
                ),
            ))
        }
        substrate::GateAction::Block => {
            let note_suffix = cached_taste
                .and_then(|taste| taste.notes.as_ref().map(|note| format!(": {}", note)))
                .unwrap_or_default();
            Err(crate::HyphaError::new(
                "TOXIC",
                format!("Spore was marked as toxic{}", note_suffix),
            ))
        }
        substrate::GateAction::Warn => {
            let note_suffix = cached_taste
                .and_then(|taste| taste.notes.as_ref().map(|note| format!(": {}", note)))
                .unwrap_or_default();
            sink.emit(crate::HyphaEvent::Warn {
                message: format!(
                    "Spore was marked as rotten{}. Recommend sandboxed environment.",
                    note_suffix
                ),
            });
            Ok(())
        }
        substrate::GateAction::Proceed => Ok(()),
    }
}

pub fn check_taste_verdict_for_replicate(
    out: &Output,
    cache: &CacheDir,
    uri_str: &str,
    domain: &str,
    hash: &str,
) -> Result<(), ExitCode> {
    check_taste_verdict(
        out,
        cache,
        uri_str,
        domain,
        hash,
        substrate::GateOperation::Replicate,
    )
}

/// CLI wrapper for taste check — used by `handle_*` functions and `spore::handle_replicate`.
fn check_taste_verdict(
    out: &Output,
    cache: &CacheDir,
    uri_str: &str,
    domain: &str,
    hash: &str,
    operation: substrate::GateOperation,
) -> Result<(), ExitCode> {
    let sink = crate::api::OutSink(out);
    check_taste_for_operation(&sink, cache, uri_str, domain, hash, operation)
        .map_err(|e| out.error_hypha(&e))
}

/// Share a signed taste report to a Synapse instance via pulse (library level, no Output).
#[allow(clippy::too_many_arguments)]
async fn share_taste_report_lib(
    uri_str: &str,
    verdict: substrate::TasteVerdict,
    notes: Option<&str>,
    signing_domain: &str,
    synapse_url: &str,
    synapse_token: Option<&str>,
    now_epoch_ms: u64,
) -> Result<(), String> {
    crate::site::validate_site_domain_path(signing_domain)?;
    let site = crate::site::SiteDir::new(signing_domain);

    if !site.private_key_path().exists() {
        return Err(format!("No identity found for domain '{}'", signing_domain));
    }

    let target_uri = substrate::normalize_taste_target_uri(uri_str)
        .map_err(|e| format!("Invalid target URI '{}': {}", uri_str, e))?;

    let identity = crate::auth::get_identity_with_site(signing_domain, &site)
        .map_err(|e| format!("Failed to load identity: {}", e))?;

    let core = substrate::TasteCore {
        target_uri: target_uri.clone(),
        domain: signing_domain.to_string(),
        key: identity.public_key.clone(),
        verdict,
        notes: notes.map(|note| vec![note.to_string()]).unwrap_or_default(),
        tasted_at_epoch_ms: now_epoch_ms,
    };

    let core_signature = crate::auth::sign_json_with_site(&site, &core).map_err(|e| match e {
        crate::auth::JsonSignError::Jcs(message) => message,
        crate::auth::JsonSignError::Sign(err) => format!("Failed to sign core: {}", err),
    })?;

    let mut payload = substrate::Taste {
        schema: substrate::TASTE_SCHEMA.to_string(),
        capsule: substrate::TasteCapsule {
            uri: String::new(),
            core,
            core_signature,
        },
        capsule_signature: String::new(),
    };

    let taste_hash = payload
        .computed_uri_hash()
        .map_err(|e| format!("JCS hash input serialization failed: {}", e))?;
    payload.capsule.uri = substrate::build_taste_uri(signing_domain, &taste_hash);

    let capsule_signature =
        crate::auth::sign_json_with_site(&site, &payload.capsule).map_err(|e| match e {
            crate::auth::JsonSignError::Jcs(message) => message,
            crate::auth::JsonSignError::Sign(err) => format!("Failed to sign capsule: {}", err),
        })?;
    payload.capsule_signature = capsule_signature;

    // Validate against schema before writing/sending
    let payload_value = serde_json::to_value(&payload)
        .map_err(|e| format!("Failed to serialize taste report: {}", e))?;
    if let Err(e) = substrate::validate_schema(&payload_value) {
        return Err(format!("Taste schema validation failed: {}", e));
    }

    // Write to site's taste dir for local persistence
    let taste_dir = site.taste_dir();
    if let Err(e) = std::fs::create_dir_all(&taste_dir) {
        return Err(format!("Failed to create taste dir: {}", e));
    }
    let taste_path = taste_dir.join(format!("{}.json", taste_hash));
    let payload_json = payload
        .to_pretty_json()
        .map_err(|e| format!("Failed to format taste report: {}", e))?;
    std::fs::write(&taste_path, &payload_json).map_err(|e| {
        format!(
            "Failed to write taste report to {}: {}",
            taste_path.display(),
            e
        )
    })?;

    let client = substrate::client::http_client(30)
        .map_err(|e| format!("Failed to create HTTP client: {}", e))?;
    let opts = fetch_opts(synapse_token);
    substrate::client::post_synapse_pulse(&client, synapse_url, &payload_value, opts)
        .await
        .map_err(|e| e.to_string())?;

    Ok(())
}

/// Fetch taste reports for a spore from a Synapse instance
async fn fetch_taste_reports(
    synapse_url: &str,
    hash: &str,
    token: Option<&str>,
) -> Result<serde_json::Value, String> {
    let client = substrate::client::http_client(30)
        .map_err(|e| format!("Failed to create HTTP client: {}", e))?;
    substrate::client::fetch_taste_reports(&client, synapse_url, hash, fetch_opts(token))
        .await
        .map_err(|e| e.to_string())
}

#[allow(clippy::too_many_arguments)]
pub async fn handle_taste(
    out: &Output,
    uri_str: &str,
    verdict: Option<substrate::TasteVerdict>,
    notes: Option<&str>,
    synapse_url: Option<&str>,
    synapse_token_secret: Option<&str>,
    domain_for_signing: Option<&str>,
) -> ExitCode {
    let sink = crate::api::OutSink(out);
    match taste_at(
        uri_str,
        verdict,
        notes,
        synapse_url,
        synapse_token_secret,
        domain_for_signing,
        crate::time::now_epoch_ms(),
        &sink,
    )
    .await
    {
        Ok(output) => out.ok(serde_json::to_value(output).unwrap_or_default()),
        Err(e) => out.error_hypha(&e),
    }
}
