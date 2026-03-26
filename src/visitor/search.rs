use super::*;

/// Search spores via Synapse.
/// Pass [`crate::NoopSink`] if you don't need event notifications.
pub async fn search(
    query: &str,
    synapse_arg: Option<&str>,
    synapse_token_secret: Option<&str>,
    domain: Option<&str>,
    license: Option<&str>,
    limit: u32,
    _sink: &dyn crate::EventSink,
) -> Result<crate::output::SearchOutput, crate::HyphaError> {
    search_with_bond(
        query,
        synapse_arg,
        synapse_token_secret,
        domain,
        license,
        None,
        limit,
        _sink,
    )
    .await
}

/// Like [`search`] but also accepts a `bond` filter string.
///
/// `bond_filter` format: `"relation:uri"` or comma-separated for AND logic,
/// e.g. `"spawned_from:cmn://a.dev/b3.3yMR7vZQ9hL,follows:cmn://b.dev/b3.def"`.
#[allow(clippy::too_many_arguments)]
pub async fn search_with_bond(
    query: &str,
    synapse_arg: Option<&str>,
    synapse_token_secret: Option<&str>,
    domain: Option<&str>,
    license: Option<&str>,
    bond_filter: Option<&str>,
    limit: u32,
    _sink: &dyn crate::EventSink,
) -> Result<crate::output::SearchOutput, crate::HyphaError> {
    let resolved =
        crate::config::resolve_synapse(synapse_arg, synapse_token_secret).map_err(|e| {
            crate::HyphaError::with_hint(
                "synapse_error",
                &e,
                "Add a synapse node with: hypha synapse add <url>",
            )
        })?;

    let client = substrate::client::http_client(30)
        .map_err(|e| crate::HyphaError::new("synapse_error", format!("HTTP client error: {e}")))?;

    let response = substrate::client::search(
        &client,
        &resolved.url,
        query,
        domain,
        license,
        bond_filter,
        limit,
        fetch_opts(resolved.token_secret.as_deref()),
    )
    .await
    .map_err(|e| crate::HyphaError::new("synapse_error", e.to_string()))?;

    let results: Vec<serde_json::Value> = response
        .result
        .spores
        .iter()
        .filter_map(|r| serde_json::to_value(r).ok())
        .collect();

    Ok(crate::output::SearchOutput {
        query: query.to_string(),
        synapse: resolved.url,
        count: results.len(),
        results,
    })
}

/// Handle the `search` command — thin CLI wrapper around [`search_with_bond`].
#[allow(clippy::too_many_arguments)]
pub async fn handle_search(
    out: &Output,
    query: &str,
    synapse_arg: Option<&str>,
    synapse_token_secret: Option<&str>,
    domain: Option<&str>,
    license: Option<&str>,
    bond_filter: Option<&str>,
    limit: u32,
) -> ExitCode {
    let sink = crate::api::OutSink(out);
    match search_with_bond(
        query,
        synapse_arg,
        synapse_token_secret,
        domain,
        license,
        bond_filter,
        limit,
        &sink,
    )
    .await
    {
        Ok(output) => out.ok(serde_json::to_value(output).unwrap_or_default()),
        Err(e) => out.error_hypha(&e),
    }
}
