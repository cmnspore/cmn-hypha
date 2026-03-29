use crate::auth;
use crate::site::SiteDir;
use substrate::{build_mycelium_uri, CmnCapsuleEntry, CmnEndpoint, CmnEntry, Mycelium};

mod format;
mod init;
mod inventory;
mod nutrients;
mod serve;

pub use format::format_mycelium;
pub use init::handle_init;
pub use inventory::{handle_status, update_inventory};
pub use nutrients::{handle_nutrient_add, handle_nutrient_clear, handle_nutrient_remove};
pub use serve::{handle_pulse, handle_serve};

pub struct InitArgs<'a> {
    pub domain: Option<&'a str>,
    pub hub: Option<&'a str>,
    pub site_path: Option<&'a str>,
    pub name: Option<&'a str>,
    pub synopsis: Option<&'a str>,
    pub bio: Option<&'a str>,
    pub endpoints_base: Option<&'a str>,
}

fn with_warning(mut data: serde_json::Value, warning: String) -> serde_json::Value {
    if let serde_json::Value::Object(ref mut fields) = data {
        fields.insert("warning".to_string(), serde_json::Value::String(warning));
    }
    data
}

/// Error type for mycelium sign-and-save operations.
#[derive(Debug, thiserror::Error)]
pub(crate) enum MyceliumError {
    #[error("{0}")]
    Identity(#[from] anyhow::Error),
    #[error("JCS canonicalization failed: {0}")]
    Jcs(String),
    #[error("signing failed: {0}")]
    Sign(String),
    #[error("{0}")]
    Io(#[from] std::io::Error),
    #[error("serialization failed: {0}")]
    Serialize(#[from] serde_json::Error),
    #[error("schema validation failed: {0}")]
    Schema(String),
    #[error("{0}")]
    Format(String),
}

impl MyceliumError {
    /// Return an error code suitable for Agent-First Data output.
    pub(crate) fn code(&self) -> &'static str {
        match self {
            Self::Identity(_) => "identity_error",
            Self::Jcs(_) => "jcs_error",
            Self::Sign(_) => "sign_error",
            Self::Io(_) => "write_error",
            Self::Serialize(_) => "serialize_error",
            Self::Schema(_) => "schema_error",
            Self::Format(_) => "serialize_error",
        }
    }
}

/// Sign mycelium core, compute hash, write mycelium file and update cmn.json.
/// Returns the new mycelium hash on success.
fn sign_and_save_mycelium(
    site: &SiteDir,
    domain: &str,
    mycelium: &mut Mycelium,
    endpoints: Vec<CmnEndpoint>,
    now_epoch_ms: u64,
) -> Result<String, MyceliumError> {
    let identity = auth::get_identity_with_site(domain, site)?;
    mycelium.capsule.core.key = identity.public_key.clone();
    mycelium.capsule.core.updated_at_epoch_ms = now_epoch_ms;

    // Sign core
    let core_signature = match auth::sign_json_with_site(site, &mycelium.capsule.core) {
        Ok(sig) => sig,
        Err(auth::JsonSignError::Jcs(message)) => return Err(MyceliumError::Jcs(message)),
        Err(auth::JsonSignError::Sign(err)) => return Err(MyceliumError::Sign(err.to_string())),
    };
    mycelium.capsule.core_signature = core_signature.clone();

    // Compute hash
    let mycelium_hash = mycelium
        .computed_uri_hash()
        .map_err(|e| MyceliumError::Jcs(e.to_string()))?;

    // Write mycelium file
    mycelium.capsule.uri = build_mycelium_uri(domain, &mycelium_hash);
    mycelium.capsule_signature = match auth::sign_json_with_site(site, &mycelium.capsule) {
        Ok(sig) => sig,
        Err(auth::JsonSignError::Jcs(message)) => return Err(MyceliumError::Jcs(message)),
        Err(auth::JsonSignError::Sign(err)) => return Err(MyceliumError::Sign(err.to_string())),
    };

    let mycelium_dir = site.mycelium_dir();
    std::fs::create_dir_all(&mycelium_dir)?;
    let mycelium_file = mycelium_dir.join(format!("{}.json", mycelium_hash));
    let mycelium_value = serde_json::to_value(&*mycelium)?;
    substrate::validate_schema(&mycelium_value)
        .map_err(|e| MyceliumError::Schema(format!("Mycelium: {}", e)))?;
    let mycelium_json =
        format_mycelium(&mycelium_value).map_err(|e| MyceliumError::Format(e.to_string()))?;
    std::fs::write(&mycelium_file, &mycelium_json)?;

    let endpoints = endpoints
        .into_iter()
        .map(|mut endpoint| {
            if endpoint.kind == "mycelium" {
                endpoint.hash = mycelium_hash.clone();
            }
            endpoint
        })
        .collect();
    let entry = CmnEntry::new(vec![CmnCapsuleEntry {
        uri: substrate::build_domain_uri(domain),
        key: identity.public_key.clone(),
        previous_keys: vec![],
        endpoints,
    }]);
    let capsule_sig = match auth::sign_json_with_site(site, &entry.capsules) {
        Ok(sig) => sig,
        Err(auth::JsonSignError::Jcs(message)) => return Err(MyceliumError::Jcs(message)),
        Err(auth::JsonSignError::Sign(err)) => return Err(MyceliumError::Sign(err.to_string())),
    };
    let signed_entry = CmnEntry {
        capsule_signature: capsule_sig,
        ..entry
    };
    let entry_value = serde_json::to_value(&signed_entry)?;
    substrate::validate_schema(&entry_value)
        .map_err(|e| MyceliumError::Schema(format!("CMN: {}", e)))?;
    let entry_json = signed_entry
        .to_pretty_json_deep()
        .map_err(|e| MyceliumError::Format(format!("Failed to format cmn.json: {}", e)))?;
    let cmn_path = site.cmn_json_path();
    if let Some(parent) = cmn_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&cmn_path, &entry_json)?;

    Ok(mycelium_hash)
}

/// Load existing mycelium and configured endpoints from a site's cmn.json.
fn load_existing_mycelium(site: &SiteDir) -> Option<(Mycelium, Vec<CmnEndpoint>)> {
    let cmn_path = site.cmn_json_path();
    let content = std::fs::read_to_string(&cmn_path).ok()?;
    let existing = serde_json::from_str::<CmnEntry>(&content).ok()?;
    let capsule = existing.primary_capsule().ok()?;
    let endpoints = capsule.endpoints.clone();
    let mycelium_hash = capsule.mycelium_hash()?;
    let filename = format!("{}.json", mycelium_hash);
    let mycelium_path = site.mycelium_dir().join(filename);
    let mc = std::fs::read_to_string(&mycelium_path).ok()?;
    let m = serde_json::from_str::<Mycelium>(&mc).ok()?;
    Some((m, endpoints))
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {

    use super::*;

    #[test]
    fn test_format_mycelium_core_key_order() {
        let mut m = Mycelium::new("example.com", "Example", "A test site", 1);
        m.capsule.core.key = "ed25519.testkey".to_string();
        let value = serde_json::to_value(&m).unwrap();
        let formatted = format_mycelium(&value).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&formatted).unwrap();
        let core = parsed["capsule"]["core"].as_object().unwrap();
        let keys: Vec<&String> = core.keys().collect();
        // Verify order: domain, key, name, synopsis, ... then remaining
        let domain_pos = keys.iter().position(|k| *k == "domain").unwrap();
        let key_pos = keys.iter().position(|k| *k == "key").unwrap();
        let name_pos = keys.iter().position(|k| *k == "name").unwrap();
        let synopsis_pos = keys.iter().position(|k| *k == "synopsis").unwrap();
        assert!(domain_pos < key_pos);
        assert!(key_pos < name_pos);
        assert!(name_pos < synopsis_pos);
    }

    #[test]
    fn test_format_mycelium_roundtrip() {
        let mut m = Mycelium::new("example.com", "Example", "A test site", 1);
        m.capsule.core.bio = "Some bio".to_string();
        m.capsule.core.nutrients.push(substrate::Nutrient {
            kind: "lightning_address".to_string(),
            address: Some("user@example.com".to_string()),
            recipient: None,
            url: None,
            label: None,
            chain_id: None,
            token: None,
            asset_id: None,
        });
        let value = serde_json::to_value(&m).unwrap();
        let formatted = format_mycelium(&value).unwrap();
        let parsed: Mycelium = serde_json::from_str(&formatted).unwrap();
        assert_eq!(parsed.capsule.core.domain, "example.com");
        assert_eq!(parsed.capsule.core.name, "Example");
        assert_eq!(parsed.capsule.core.synopsis, "A test site");
        assert_eq!(parsed.capsule.core.bio, "Some bio");
        assert_eq!(parsed.capsule.core.nutrients.len(), 1);
        assert_eq!(parsed.capsule.core.nutrients[0].kind, "lightning_address");
        assert_eq!(
            parsed.capsule.core.nutrients[0].address,
            Some("user@example.com".to_string())
        );
    }
}
