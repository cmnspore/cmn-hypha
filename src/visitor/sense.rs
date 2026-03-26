use super::*;

/// Resolve a CMN URI and return structured output.
///
/// The returned [`SenseOutput`](crate::output::SenseOutput) contains:
/// - `data`: `{"mycelium": ...}` or `{"spore": ...}`.
/// - `trace`: hypha metadata (`uri`, `cmn`, `verified`).
///
/// Cache-write warnings are emitted to `sink`; pass [`crate::NoopSink`] to discard.
pub async fn sense(
    uri_str: &str,
    sink: &dyn crate::EventSink,
) -> Result<crate::output::SenseOutput, crate::HyphaError> {
    let uri = CmnUri::parse(uri_str).map_err(|e| crate::HyphaError::new("invalid_uri", e))?;

    let cache = CacheDir::new();
    let domain_cache = cache.domain(&uri.domain);

    let cmn_cached = domain_cache.load_cmn().is_some();
    let cmn_cached_at = mtime_epoch_ms(domain_cache.cmn_path());

    let entry = get_cmn_entry(sink, &domain_cache, cache.cmn_ttl_ms).await?;

    let trace = json!({
        "uri": uri_str,
        "cmn": {
            "resolved": true,
            "cached": cmn_cached,
            "cached_at_epoch_ms": cmn_cached_at,
        },
    });

    let (data, trace) = match uri.hash.as_deref() {
        None => sense_mycelium_data(&entry, trace).await?,
        Some(hash) => sense_spore_data(hash, &entry, trace).await?,
    };

    Ok(crate::output::SenseOutput {
        uri: uri_str.to_string(),
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

async fn sense_mycelium_data(
    entry: &CmnEntry,
    trace: serde_json::Value,
) -> Result<(serde_json::Value, serde_json::Value), crate::HyphaError> {
    let capsule =
        primary_capsule(entry).map_err(|e| crate::HyphaError::new("manifest_failed", e.message))?;
    let client = substrate::client::http_client(30).map_err(|e| {
        crate::HyphaError::new("manifest_failed", format!("HTTP client error: {e}"))
    })?;

    let mut fallback: Option<&str> = None;
    let manifest =
        match substrate::client::fetch_mycelium(&client, capsule, Default::default()).await {
            Ok(m) => m,
            Err(domain_err) => {
                // Try synapse fallback
                let cfg = crate::config::HyphaConfig::load();
                let domain = &CmnUri::parse(&capsule.uri)
                    .map(|u| u.domain)
                    .unwrap_or_default();
                let cache = CacheDir::new();
                let domain_cache = cache.domain(domain);
                if can_synapse_fallback(&domain_cache, &capsule.key, &cfg.cache) {
                    if let Some((synapse_url, synapse_token)) = resolve_default_synapse_url(&cfg) {
                        // Fetch cmn.json from synapse, then get mycelium by hash
                        let cmn_resp = substrate::client::fetch_synapse_cmn(
                            &client,
                            &synapse_url,
                            domain,
                            fetch_opts(synapse_token.as_deref()),
                        )
                        .await
                        .map_err(|e| {
                            crate::HyphaError::new(
                                "manifest_failed",
                                format!("Domain: {domain_err}; Synapse cmn: {e}"),
                            )
                        })?;
                        // Extract primary capsule hash from cmn.json to fetch mycelium
                        let cmn_entry: substrate::CmnEntry =
                            serde_json::from_value(cmn_resp.result.cmn).map_err(|e| {
                                crate::HyphaError::new(
                                    "manifest_failed",
                                    format!("Failed to parse synapse cmn.json: {e}"),
                                )
                            })?;
                        let cmn_capsule = cmn_entry.primary_capsule().map_err(|e| {
                            crate::HyphaError::new("manifest_failed", e.to_string())
                        })?;
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
                                format!("Domain: {domain_err}; Synapse mycelium: {e}"),
                            )
                        })?;
                        fallback = Some("synapse");
                        myc_resp.result.mycelium
                    } else {
                        return Err(crate::HyphaError::new(
                            "manifest_failed",
                            domain_err.to_string(),
                        ));
                    }
                } else {
                    return Err(crate::HyphaError::new(
                        "manifest_failed",
                        domain_err.to_string(),
                    ));
                }
            }
        };

    let pk = &capsule.key;
    let core_ok = verify_manifest_core_signature(&manifest, pk).is_ok();
    let capsule_ok = verify_manifest_capsule_signature(&manifest, pk).is_ok();
    let trace = with_verified_trace(trace, core_ok, capsule_ok, fallback);

    Ok((json!({ "mycelium": manifest }), trace))
}

async fn sense_spore_data(
    hash: &str,
    entry: &CmnEntry,
    trace: serde_json::Value,
) -> Result<(serde_json::Value, serde_json::Value), crate::HyphaError> {
    let capsule =
        primary_capsule(entry).map_err(|e| crate::HyphaError::new("manifest_failed", e.message))?;

    let mut fallback: Option<&str> = None;
    let manifest = match fetch_spore_manifest(capsule, hash).await {
        Ok(m) => m,
        Err(domain_err) => {
            let cfg = crate::config::HyphaConfig::load();
            let domain = &CmnUri::parse(&capsule.uri)
                .map(|u| u.domain)
                .unwrap_or_default();
            let cache = CacheDir::new();
            let domain_cache = cache.domain(domain);
            if can_synapse_fallback(&domain_cache, &capsule.key, &cfg.cache) {
                if let Some((synapse_url, synapse_token)) = resolve_default_synapse_url(&cfg) {
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
                            format!("Domain: {domain_err}; Synapse: {e}"),
                        )
                    })?;
                    fallback = Some("synapse");
                    resp.result.spore
                } else {
                    return Err(crate::HyphaError::new("manifest_failed", domain_err));
                }
            } else {
                return Err(crate::HyphaError::new("manifest_failed", domain_err));
            }
        }
    };

    let host_key = &capsule.key;
    let trace = with_verified_trace(
        trace,
        verify_manifest_core_signature(&manifest, host_key).is_ok(),
        verify_manifest_capsule_signature(&manifest, host_key).is_ok(),
        fallback,
    );

    Ok((json!({ "spore": manifest }), trace))
}

/// Handle the `sense` command — thin CLI wrapper around [`sense`].
pub async fn handle_sense(out: &Output, uri_str: &str) -> ExitCode {
    let sink = crate::api::OutSink(out);
    match sense(uri_str, &sink).await {
        Ok(output) => out.ok_trace(output.data, output.trace),
        Err(e) => out.error_hypha(&e),
    }
}
