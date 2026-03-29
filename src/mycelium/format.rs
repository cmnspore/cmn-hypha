use substrate::PrettyJson;

/// Format a mycelium JSON value with canonical key ordering.
/// Delegates to substrate's `Mycelium::to_pretty_json()` via value round-trip.
pub fn format_mycelium(value: &serde_json::Value) -> Result<String, crate::sink::HyphaError> {
    use crate::sink::HyphaError;
    let mycelium: substrate::Mycelium = serde_json::from_value(value.clone())
        .map_err(|e| HyphaError::new("format_error", format!("deserialize error: {}", e)))?;
    mycelium
        .to_pretty_json()
        .map_err(|e| HyphaError::new("format_error", format!("format error: {}", e)))
}
