use super::*;

/// Resolve a CMN URI and return structured output.
///
/// The returned [`SenseOutput`](crate::output::SenseOutput) contains:
/// - `data`: `{"mycelium": ...}` or `{"spore": ...}`.
/// - `trace`: Hypha metadata (`cmn_url`, `cmn`, `verified`).
///
/// Cache-write warnings are emitted to `sink`; pass [`crate::NoopSink`] to discard.
pub async fn sense(
    uri_str: &str,
    sink: &dyn crate::EventSink,
) -> Result<crate::output::SenseOutput, crate::HyphaError> {
    sense_with_id(uri_str, None, sink).await
}

/// Resolve a CMN URI, optionally using a spore id to select the latest spore
/// from the domain's mycelium inventory before returning the spore manifest.
pub async fn sense_with_id(
    uri_str: &str,
    spore_id: Option<&str>,
    sink: &dyn crate::EventSink,
) -> Result<crate::output::SenseOutput, crate::HyphaError> {
    let uri = CmnUri::parse(uri_str).map_err(|e| crate::HyphaError::new("invalid_uri", e))?;
    if spore_id.is_some() && uri.hash.is_some() {
        return Err(crate::HyphaError::new(
            "invalid_args",
            "--id can only be used with a domain URI such as cmn://example.com",
        ));
    }

    let cache = CacheDir::new()?;
    let domain_cache = cache.domain(&uri.domain);

    let cmn_cached = domain_cache.load_cmn().is_some();
    let cmn_cached_at = mtime_epoch_ms(domain_cache.cmn_path());

    let entry = get_cmn_entry(sink, &domain_cache, cache.cmn_ttl_ms).await?;

    let trace = json!({
        "cmn_url": uri_str,
        "cmn": {
            "resolved": true,
            "cached": cmn_cached,
            "cached_at_epoch_ms": cmn_cached_at,
        },
    });

    let (data, trace, output_uri) = match (uri.hash.as_deref(), spore_id) {
        (None, Some(spore_id)) => {
            sense_spore_id_data(&uri.domain, spore_id, &entry, trace, sink).await?
        }
        (None, None) => {
            let (data, trace) = sense_mycelium_data(&entry, trace, sink).await?;
            (data, trace, uri_str.to_string())
        }
        (Some(hash), None) => {
            let (data, trace) = sense_spore_data(hash, &entry, trace, sink).await?;
            (data, trace, uri_str.to_string())
        }
        (Some(_), Some(_)) => unreachable!("validated above"),
    };

    Ok(crate::output::SenseOutput {
        cmn_url: output_uri,
        data,
        trace,
    })
}

fn with_verified_trace(
    trace: serde_json::Value,
    core_signature: bool,
    capsule_signature: bool,
    fallback: Option<&str>,
) -> serde_json::Value {
    match trace {
        serde_json::Value::Object(mut fields) => {
            fields.insert(
                "verified".to_string(),
                json!({
                    "core_signature": core_signature,
                    "capsule_signature": capsule_signature,
                }),
            );
            if let Some(source) = fallback {
                fields.insert("fallback".to_string(), json!(source));
            }
            serde_json::Value::Object(fields)
        }
        other => other,
    }
}

fn with_resolved_trace(
    trace: serde_json::Value,
    resolved: &impl serde::Serialize,
    mycelium_verified: Option<serde_json::Value>,
) -> Result<serde_json::Value, crate::HyphaError> {
    Ok(match trace {
        serde_json::Value::Object(mut fields) => {
            let mut resolved_value = serde_json::to_value(resolved).map_err(|e| {
                crate::HyphaError::new(
                    "serialize_error",
                    format!("Failed to serialize resolved spore metadata: {e}"),
                )
            })?;
            if let (serde_json::Value::Object(ref mut resolved_fields), Some(verified)) =
                (&mut resolved_value, mycelium_verified)
            {
                resolved_fields.insert("mycelium_verified".to_string(), verified);
            }
            fields.insert("resolved".to_string(), resolved_value);
            serde_json::Value::Object(fields)
        }
        other => other,
    })
}

/// Emit a prominent warning when sensed data failed signature verification.
/// `sense` is inspection-only and still returns the data, but the caller must
/// not mistake unverified (or synapse-served) data for trusted.
fn warn_if_unverified(
    sink: &dyn crate::EventSink,
    core_ok: bool,
    capsule_ok: bool,
    fallback: Option<&str>,
) {
    if core_ok && capsule_ok {
        return;
    }
    let via = if fallback == Some("synapse") {
        " — served via synapse fallback, treat as untrusted"
    } else {
        ""
    };
    sink.emit(crate::HyphaEvent::Warn {
        message: format!(
            "Signature verification failed (core={}, capsule={}){}",
            core_ok, capsule_ok, via
        ),
    });
}

/// If the trust policy permits a synapse fallback and one is configured, run
/// `fetch` against the resolved synapse `(synapse_url, token_secret)`. Otherwise return
/// `domain_err` unchanged. Centralizes the fallback/trust decision shared by the
/// mycelium and spore sense paths.
async fn with_synapse_fallback<T, F, Fut>(
    capsule: &substrate::CmnCapsuleEntry,
    domain_err: crate::HyphaError,
    fetch: F,
) -> Result<T, crate::HyphaError>
where
    F: FnOnce(String, Option<String>) -> Fut,
    Fut: std::future::Future<Output = Result<T, crate::HyphaError>>,
{
    let cfg = crate::config::HyphaConfig::load()?;
    let domain = CmnUri::parse(&capsule.uri)
        .map(|u| u.domain)
        .unwrap_or_default();
    let domain_cache = CacheDir::new()?.domain(&domain);
    if can_synapse_fallback(&domain_cache, &capsule.key, &cfg.cache) {
        if let Some((synapse_url, synapse_token)) = resolve_default_synapse_url(&cfg) {
            return fetch(synapse_url, synapse_token).await;
        }
    }
    Err(domain_err)
}

async fn sense_mycelium_data(
    entry: &CmnEntry,
    trace: serde_json::Value,
    sink: &dyn crate::EventSink,
) -> Result<(serde_json::Value, serde_json::Value), crate::HyphaError> {
    let capsule =
        primary_capsule(entry).map_err(|e| crate::HyphaError::new("manifest_failed", e.message))?;
    let client = substrate::client::http_client(30).map_err(|e| {
        crate::HyphaError::new("manifest_failed", format!("HTTP client error: {e}"))
    })?;

    let mut fallback: Option<&str> = None;
    let manifest = match substrate::client::fetch_mycelium(&client, capsule, fetch_opts(None)).await
    {
        Ok(m) => m,
        Err(domain_err) => {
            let domain_err = crate::HyphaError::new("manifest_failed", domain_err.to_string());
            let domain_err_str = domain_err.message.clone();
            let domain = CmnUri::parse(&capsule.uri)
                .map(|u| u.domain)
                .unwrap_or_default();
            let m = with_synapse_fallback(
                capsule,
                domain_err,
                |synapse_url, synapse_token| async move {
                    // Fetch cmn.json from synapse, then get mycelium by hash.
                    let cmn_resp = substrate::client::fetch_synapse_cmn(
                        &client,
                        &synapse_url,
                        &domain,
                        fetch_opts(synapse_token.as_deref()),
                    )
                    .await
                    .map_err(|e| {
                        crate::HyphaError::new(
                            "manifest_failed",
                            format!("Domain: {domain_err_str}; Synapse cmn: {e}"),
                        )
                    })?;
                    // Validate the untrusted synapse cmn.json against the schema
                    // before relying on any of its fields.
                    substrate::validate_schema(&cmn_resp.result.cmn).map_err(|e| {
                        crate::HyphaError::new(
                            "manifest_failed",
                            format!("Synapse cmn.json failed schema validation: {e}"),
                        )
                    })?;
                    let cmn_entry: substrate::CmnEntry =
                        serde_json::from_value(cmn_resp.result.cmn).map_err(|e| {
                            crate::HyphaError::new(
                                "manifest_failed",
                                format!("Failed to parse synapse cmn.json: {e}"),
                            )
                        })?;
                    let cmn_capsule = cmn_entry
                        .primary_capsule()
                        .map_err(|e| crate::HyphaError::new("manifest_failed", e.to_string()))?;
                    let mycelium_hash = cmn_capsule.mycelium_hash().ok_or_else(|| {
                        crate::HyphaError::new(
                            "manifest_failed",
                            "No mycelium hash in synapse cmn.json",
                        )
                    })?;
                    let myc_resp = substrate::client::fetch_synapse_mycelium_by_hash(
                        &client,
                        &synapse_url,
                        mycelium_hash,
                        fetch_opts(synapse_token.as_deref()),
                    )
                    .await
                    .map_err(|e| {
                        crate::HyphaError::new(
                            "manifest_failed",
                            format!("Domain: {domain_err_str}; Synapse mycelium: {e}"),
                        )
                    })?;
                    Ok(myc_resp.result.mycelium)
                },
            )
            .await?;
            fallback = Some("synapse");
            m
        }
    };

    let pk = &capsule.key;
    let core_ok = verify_manifest_core_signature(&manifest, pk).is_ok();
    let capsule_ok = verify_manifest_capsule_signature(&manifest, pk).is_ok();
    warn_if_unverified(sink, core_ok, capsule_ok, fallback);
    let trace = with_verified_trace(trace, core_ok, capsule_ok, fallback);

    Ok((json!({ "mycelium": manifest }), trace))
}

async fn sense_spore_data(
    hash: &str,
    entry: &CmnEntry,
    trace: serde_json::Value,
    sink: &dyn crate::EventSink,
) -> Result<(serde_json::Value, serde_json::Value), crate::HyphaError> {
    let capsule =
        primary_capsule(entry).map_err(|e| crate::HyphaError::new("manifest_failed", e.message))?;

    let mut fallback: Option<&str> = None;
    let manifest = match fetch_spore_manifest(capsule, hash).await {
        Ok(m) => m,
        Err(domain_err) => {
            let domain_err_str = domain_err.message.clone();
            let m = with_synapse_fallback(
                capsule,
                domain_err,
                |synapse_url, synapse_token| async move {
                    let client = substrate::client::http_client(30).map_err(|e| {
                        crate::HyphaError::new("manifest_failed", format!("HTTP client error: {e}"))
                    })?;
                    let resp = substrate::client::fetch_synapse_spore(
                        &client,
                        &synapse_url,
                        hash,
                        fetch_opts(synapse_token.as_deref()),
                    )
                    .await
                    .map_err(|e| {
                        crate::HyphaError::new(
                            "manifest_failed",
                            format!("Domain: {domain_err_str}; Synapse: {e}"),
                        )
                    })?;
                    Ok(resp.result.spore)
                },
            )
            .await?;
            fallback = Some("synapse");
            m
        }
    };

    let host_key = &capsule.key;
    let core_ok = verify_manifest_core_signature(&manifest, host_key).is_ok();
    let capsule_ok = verify_manifest_capsule_signature(&manifest, host_key).is_ok();
    warn_if_unverified(sink, core_ok, capsule_ok, fallback);
    let trace = with_verified_trace(trace, core_ok, capsule_ok, fallback);

    Ok((json!({ "spore": manifest }), trace))
}

async fn sense_spore_id_data(
    domain: &str,
    spore_id: &str,
    entry: &CmnEntry,
    trace: serde_json::Value,
    sink: &dyn crate::EventSink,
) -> Result<(serde_json::Value, serde_json::Value, String), crate::HyphaError> {
    let (mycelium_data, trace) = sense_mycelium_data(entry, trace, sink).await?;
    let mycelium_value = mycelium_data
        .get("mycelium")
        .cloned()
        .ok_or_else(|| crate::HyphaError::new("manifest_failed", "Missing mycelium result"))?;
    let mycelium: substrate::Mycelium = serde_json::from_value(mycelium_value).map_err(|e| {
        crate::HyphaError::new("manifest_failed", format!("Invalid mycelium manifest: {e}"))
    })?;
    let resolved = crate::mycelium::resolve_spore_ref(domain, spore_id, &mycelium, None)?;
    let mycelium_verified = trace.get("verified").cloned();
    let trace = with_resolved_trace(trace, &resolved, mycelium_verified)?;
    let (data, trace) = sense_spore_data(&resolved.hash, entry, trace, sink).await?;
    Ok((data, trace, resolved.cmn_url))
}

/// Handle the `sense` command — thin CLI wrapper around [`sense`].
pub async fn handle_sense(out: &Output, uri_str: &str, spore_id: Option<&str>) -> ExitCode {
    let sink = crate::api::OutSink(out);
    match sense_with_id(uri_str, spore_id, &sink).await {
        Ok(output) => out.ok_trace(output.data, output.trace),
        Err(e) => out.error_hypha(&e),
    }
}
