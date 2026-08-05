use super::*;

/// Trace the spawn/inspiration chain (outbound lineage) of a spore.
pub async fn lineage_out(
    uri_str: &str,
    synapse_arg: Option<&str>,
    synapse_token_secret: Option<&str>,
    max_depth: u32,
    _sink: &dyn crate::EventSink,
) -> Result<crate::output::BondsOutput, crate::HyphaError> {
    lineage_direction(
        uri_str,
        "outbound",
        synapse_arg,
        synapse_token_secret,
        max_depth,
        _sink,
    )
    .await
}

/// Find descendants (inbound lineage / forks/evolutions) of a spore.
pub async fn lineage_in(
    uri_str: &str,
    synapse_arg: Option<&str>,
    synapse_token_secret: Option<&str>,
    max_depth: u32,
    _sink: &dyn crate::EventSink,
) -> Result<crate::output::BondsOutput, crate::HyphaError> {
    lineage_direction(
        uri_str,
        "inbound",
        synapse_arg,
        synapse_token_secret,
        max_depth,
        _sink,
    )
    .await
}

/// Shared implementation for lineage in/out.
async fn lineage_direction(
    uri_str: &str,
    direction: &str,
    synapse_arg: Option<&str>,
    synapse_token_secret: Option<&str>,
    max_depth: u32,
    _sink: &dyn crate::EventSink,
) -> Result<crate::output::BondsOutput, crate::HyphaError> {
    let resolved = crate::config::resolve_synapse(synapse_arg, synapse_token_secret)
        .map_err(|e| crate::HyphaError::new("synapse_error", e.to_string()))?;

    let uri = CmnUri::parse(uri_str).map_err(|e| crate::HyphaError::new("invalid_uri", e))?;

    let hash = uri
        .hash
        .as_deref()
        .ok_or_else(|| crate::HyphaError::new("invalid_uri", "spore URI must include a hash"))?;

    let client = substrate::client::http_client(30)
        .map_err(|e| crate::HyphaError::new("synapse_error", format!("HTTP client error: {e}")))?;

    let bonds = substrate::client::fetch_lineage(
        &client,
        &resolved.synapse_url,
        hash,
        direction,
        max_depth,
        fetch_opts(resolved.token_secret.as_deref()),
    )
    .await
    .map_err(|e| crate::HyphaError::new("synapse_error", e.to_string()))?;

    let bonds_val = &bonds.result.bonds;
    let depth_reached = bonds
        .trace
        .as_ref()
        .map(|t| t.max_depth_reached)
        .unwrap_or(false);

    let bond_values = bonds_val
        .iter()
        .map(|bond| crate::output::LineageNode {
            cmn_url: bond.uri.clone(),
            domain: bond.domain.clone(),
            name: bond.name.clone(),
            synopsis: bond.synopsis.clone(),
            license: bond.license.clone(),
            intent: bond.intent.clone(),
            relation: bond.relation.clone(),
        })
        .collect();

    Ok(crate::output::BondsOutput {
        cmn_url: uri_str.to_string(),
        hash: hash.to_string(),
        synapse_url: resolved.synapse_url,
        direction: direction.to_string(),
        max_depth: bonds.result.query.max_depth,
        max_depth_reached: depth_reached,
        bond_count: bonds_val.len(),
        bonds: bond_values,
    })
}

pub async fn handle_lineage(
    out: &Output,
    uri_str: &str,
    direction: Option<&str>,
    synapse_arg: Option<&str>,
    synapse_token_secret: Option<&str>,
    max_depth: u32,
) -> ExitCode {
    let sink = crate::api::OutSink(out);
    let dir = if direction == Some("out") {
        "outbound"
    } else {
        "inbound"
    };
    match lineage_direction(
        uri_str,
        dir,
        synapse_arg,
        synapse_token_secret,
        max_depth,
        &sink,
    )
    .await
    {
        Ok(output) => out.ok(output),
        Err(e) => out.error_hypha(&e),
    }
}
