use crate::site::SiteDir;
use ed25519_dalek::pkcs8::{DecodePrivateKey, DecodePublicKey, EncodePrivateKey, EncodePublicKey};
use ed25519_dalek::{Signer, SigningKey, VerifyingKey};
use rand::RngExt;
use serde::Serialize;
use std::fs;
use substrate::{
    compute_signature, format_key, format_signature, KeyAlgorithm, SignatureAlgorithm,
};
use zeroize::Zeroize;

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

#[derive(serde::Serialize)]
pub struct IdentityInfo {
    pub domain: String,
    pub public_key: String,
    /// True when a new keypair was generated (first time), false when loaded existing.
    pub newly_created: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum JsonSignError {
    #[error("JCS serialization failed: {0}")]
    Jcs(String),
    #[error(transparent)]
    Sign(#[from] anyhow::Error),
}

pub fn init_identity_with_site(domain: &str, site: &SiteDir) -> anyhow::Result<IdentityInfo> {
    // Create directories
    site.create_dirs()?;

    let newly_created = !site.private_key_path().exists();
    let verifying_key = if !newly_created {
        // Identity already exists - load existing keypair and update DNS record
        let pem_content = fs::read_to_string(site.private_key_path())?;
        let signing_key = SigningKey::from_pkcs8_pem(&pem_content)
            .map_err(|e| anyhow::anyhow!("Invalid private key PEM: {}", e))?;
        signing_key.verifying_key()
    } else {
        // Generate new keypair
        let mut secret_bytes = [0u8; 32];
        rand::rng().fill(&mut secret_bytes[..]);
        let signing_key = SigningKey::from_bytes(&secret_bytes);
        secret_bytes.zeroize(); // Clear secret material from stack
        let verifying_key = signing_key.verifying_key();
        // SigningKey implements ZeroizeOnDrop via ed25519-dalek "zeroize" feature

        // Save private key in PEM format (PKCS#8, OpenSSL compatible)
        let private_key_path = site.private_key_path();
        let private_pem = signing_key
            .to_pkcs8_pem(ed25519_dalek::pkcs8::spki::der::pem::LineEnding::LF)
            .map_err(|e| anyhow::anyhow!("Failed to encode private key: {}", e))?;

        // Write with 0o600 from creation to avoid transient world-readable window
        #[cfg(unix)]
        {
            use std::io::Write;
            use std::os::unix::fs::OpenOptionsExt;
            let mut f = fs::OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .mode(0o600)
                .open(&private_key_path)?;
            f.write_all(private_pem.as_bytes())?;
        }
        #[cfg(not(unix))]
        fs::write(&private_key_path, private_pem.as_bytes())?;

        verifying_key
    };

    // Always update public key (in case format changed)
    let public_key = format_key(KeyAlgorithm::Ed25519, &verifying_key.to_bytes());
    let public_pem = verifying_key
        .to_public_key_pem(ed25519_dalek::pkcs8::spki::der::pem::LineEnding::LF)
        .map_err(|e| anyhow::anyhow!("Failed to encode public key: {}", e))?;
    fs::write(site.public_key_path(), &public_pem)?;

    Ok(IdentityInfo {
        domain: domain.to_string(),
        public_key,
        newly_created,
    })
}

pub fn get_identity_with_site(domain: &str, site: &SiteDir) -> anyhow::Result<IdentityInfo> {
    if !site.public_key_path().exists() {
        anyhow::bail!("No identity found at {}", site.root.display());
    }

    // Read PEM-encoded public key
    let pem_content = fs::read_to_string(site.public_key_path())?;
    let verifying_key = VerifyingKey::from_public_key_pem(&pem_content)
        .map_err(|e| anyhow::anyhow!("Invalid public key PEM: {}", e))?;

    let public_key = format_key(KeyAlgorithm::Ed25519, &verifying_key.to_bytes());

    Ok(IdentityInfo {
        domain: domain.to_string(),
        public_key,
        newly_created: false,
    })
}

pub fn sign_json_with_site<T: Serialize>(
    site: &SiteDir,
    value: &T,
) -> Result<String, JsonSignError> {
    let signing_key = load_signing_key_with_site(site)?;
    compute_signature(value, SignatureAlgorithm::Ed25519, &signing_key.to_bytes())
        .map_err(|e| JsonSignError::Jcs(e.to_string()))
}

/// Sign data using the site's private key.
///
/// Dispatches to the correct signing algorithm based on `SIGN_ALGORITHM`.
/// Returns signature in format `{algorithm}.{base58}`.
pub fn sign_data_with_site(site: &SiteDir, data: &[u8]) -> anyhow::Result<String> {
    let signing_key = load_signing_key_with_site(site)?;
    sign_ed25519(&signing_key, data)
}

fn load_signing_key_with_site(site: &SiteDir) -> anyhow::Result<SigningKey> {
    let private_key_path = site.private_key_path();

    if !private_key_path.exists() {
        anyhow::bail!("No private key found at {}", private_key_path.display());
    }

    #[cfg(unix)]
    {
        let metadata = fs::metadata(&private_key_path)?;
        let mode = metadata.permissions().mode() & 0o777;
        if mode != 0o600 {
            anyhow::bail!(
                "Private key has insecure permissions {:04o} (expected 0600).\n\
                 Fix with: chmod 600 {}",
                mode,
                private_key_path.display()
            );
        }
    }

    let pem_content = fs::read_to_string(private_key_path)?;
    SigningKey::from_pkcs8_pem(&pem_content)
        .map_err(|e| anyhow::anyhow!("Invalid private key PEM: {}", e))
}

fn sign_ed25519(signing_key: &SigningKey, data: &[u8]) -> anyhow::Result<String> {
    let signature = signing_key.sign(data);
    Ok(format_signature(
        SignatureAlgorithm::Ed25519,
        &signature.to_bytes(),
    ))
}
