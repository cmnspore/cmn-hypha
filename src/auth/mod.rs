use crate::site::SiteDir;
use ed25519_dalek::pkcs8::{DecodePrivateKey, DecodePublicKey, EncodePrivateKey, EncodePublicKey};
use ed25519_dalek::{Signer, SigningKey, VerifyingKey};
use rand::RngExt;
use serde::Serialize;
use std::fs;
use substrate::{
    KeyAlgorithm, SignatureAlgorithm, compute_signature, format_key, format_signature,
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
    site.create_dirs()?;
    crate::cache::fs::with_dir_lock(&site.keys, || {
        init_identity_locked(domain, site)
            .map_err(|e| crate::HyphaError::new("identity_init_failed", e.to_string()))
    })
    .map_err(anyhow::Error::from)
}

fn init_identity_locked(domain: &str, site: &SiteDir) -> anyhow::Result<IdentityInfo> {
    let newly_created = !site.private_key_path().exists();
    let verifying_key = if !newly_created {
        // Identity already exists — load it (with the same permission check the
        // signing paths use) to derive the public key.
        load_signing_key_with_site(site)?.verifying_key()
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

        crate::cache::fs::atomic_write_file(
            &private_key_path,
            private_pem.as_bytes(),
            &site.keys.join(".cmn"),
            Some(0o600),
        )?;

        verifying_key
    };

    // Always update public key (in case format changed)
    let public_key = format_key(KeyAlgorithm::Ed25519, &verifying_key.to_bytes());
    let public_pem = verifying_key
        .to_public_key_pem(ed25519_dalek::pkcs8::spki::der::pem::LineEnding::LF)
        .map_err(|e| anyhow::anyhow!("Failed to encode public key: {}", e))?;
    crate::cache::fs::atomic_write_file(
        &site.public_key_path(),
        public_pem.as_bytes(),
        &site.keys.join(".cmn"),
        Some(0o644),
    )?;

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
    // Zeroize the raw scalar copy that `to_bytes()` produces.
    let key_bytes = zeroize::Zeroizing::new(signing_key.to_bytes());
    compute_signature(value, SignatureAlgorithm::Ed25519, &*key_bytes)
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

    // Hold the secret PEM in a buffer that is zeroized on drop.
    let pem_content = zeroize::Zeroizing::new(fs::read_to_string(private_key_path)?);
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

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::process::{Command, Stdio};

    #[test]
    fn identity_init_subprocess() {
        let Some(root) = std::env::var_os("CMN_HYPHA_TEST_IDENTITY_SITE") else {
            return;
        };
        let result_path = std::env::var_os("CMN_HYPHA_TEST_IDENTITY_RESULT").unwrap();
        let mut start = [0];
        std::io::stdin().read_exact(&mut start).unwrap();
        let site = SiteDir::with_path(root.into());
        let identity = init_identity_with_site("example.com", &site).unwrap();
        std::fs::write(result_path, serde_json::to_vec(&identity).unwrap()).unwrap();
    }

    #[test]
    fn concurrent_init_keeps_one_identity_and_matching_keypair() {
        let root = tempfile::tempdir().unwrap();
        let site = SiteDir::with_path(root.path().join("site"));
        let mut children = Vec::new();
        let mut result_paths = Vec::new();
        for index in 0..2 {
            let result_path = root.path().join(format!("identity-{index}.json"));
            let child = Command::new(std::env::current_exe().unwrap())
                .args(["--exact", "auth::tests::identity_init_subprocess"])
                .env("CMN_HYPHA_TEST_IDENTITY_SITE", &site.root)
                .env("CMN_HYPHA_TEST_IDENTITY_RESULT", &result_path)
                .stdin(Stdio::piped())
                .stdout(Stdio::null())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap();
            children.push(child);
            result_paths.push(result_path);
        }
        for child in &mut children {
            child.stdin.take().unwrap().write_all(b"1").unwrap();
        }
        for child in children {
            let output = child.wait_with_output().unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
        let identities: Vec<serde_json::Value> = result_paths
            .iter()
            .map(|path| serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap())
            .collect();
        assert_eq!(identities[0]["public_key"], identities[1]["public_key"]);
        assert_ne!(
            identities[0]["newly_created"],
            identities[1]["newly_created"]
        );
        let private = std::fs::read(site.private_key_path()).unwrap();
        let repeated = init_identity_with_site("example.com", &site).unwrap();
        assert!(!repeated.newly_created);
        assert_eq!(std::fs::read(site.private_key_path()).unwrap(), private);
        assert_eq!(
            load_signing_key_with_site(&site).unwrap().verifying_key(),
            VerifyingKey::from_public_key_pem(
                &std::fs::read_to_string(site.public_key_path()).unwrap()
            )
            .unwrap()
        );
        #[cfg(unix)]
        assert_eq!(
            std::fs::metadata(site.private_key_path())
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
    }
}
