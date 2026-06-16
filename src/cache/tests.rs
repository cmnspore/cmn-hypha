#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use tempfile::TempDir;

use super::*;

fn keypair(seed: u8) -> ([u8; 32], String) {
    let private_key = [seed; 32];
    let signing_key = ed25519_dalek::SigningKey::from_bytes(&private_key);
    let public_key = substrate::format_key(
        substrate::KeyAlgorithm::Ed25519,
        &signing_key.verifying_key().to_bytes(),
    );
    (private_key, public_key)
}

fn cmn_entry(serial: u64, key: String) -> substrate::CmnEntry {
    substrate::CmnEntry::new(vec![substrate::CmnCapsuleEntry {
        uri: "cmn://example.com".to_string(),
        serial,
        key,
        history: vec![],
        endpoints: vec![substrate::CmnEndpoint {
            kind: "taste".to_string(),
            url: "https://example.com/cmn/taste/{hash}.json".to_string(),
            hash: String::new(),
            hashes: vec![],
            format: None,
            delta_url: None,
        }],
    }])
}

fn rotation_entry(
    from_private: &[u8; 32],
    from_key: &str,
    to_key: &str,
    serial: u64,
) -> substrate::KeyHistoryEntry {
    let retired_at_epoch_ms = 1_710_000_000_000;
    let statement = substrate::build_key_rotation_statement(
        "example.com",
        from_key,
        to_key,
        serial,
        retired_at_epoch_ms,
    );
    let rotation_signature = substrate::compute_signature(
        &statement,
        substrate::SignatureAlgorithm::Ed25519,
        from_private,
    )
    .unwrap();
    substrate::KeyHistoryEntry {
        key: from_key.to_string(),
        status: substrate::KeyHistoryStatus::Retired,
        retired_at_epoch_ms,
        replaced_by: Some(to_key.to_string()),
        effective_serial: Some(serial),
        rotation_signature: Some(rotation_signature),
        revoked_at_epoch_ms: None,
    }
}

#[test]
fn test_fetch_status_success() {
    let status = FetchStatus::success();
    assert!(status.fetched_at_epoch_ms.is_some());
    assert!(status.failed_at_epoch_ms.is_none());
    assert_eq!(status.retry_count, 0);
}

#[test]
fn test_fetch_status_failure() {
    let status = FetchStatus::failure("connection timeout", None);
    assert!(status.failed_at_epoch_ms.is_some());
    assert_eq!(status.retry_count, 1);
    assert_eq!(status.error, Some("connection timeout".to_string()));
}

#[test]
fn test_fetch_status_retry() {
    let first = FetchStatus::failure("error 1", None);
    let second = FetchStatus::failure("error 2", Some(&first));
    assert_eq!(second.retry_count, 2);
}

#[test]
fn test_domain_cache_paths() {
    let temp = TempDir::new().unwrap();
    let cache = CacheDir::with_root(temp.path().to_path_buf());

    let domain = cache.domain("example.com");
    assert!(domain.cmn_path().ends_with("mycelium/cmn.json"));
    assert!(domain.status_path().ends_with("mycelium/status.json"));
}

#[test]
fn test_spore_path_new_structure() {
    let temp = TempDir::new().unwrap();
    let cache = CacheDir::with_root(temp.path().to_path_buf());

    let path = cache.spore_path("example.com", "b3.3yMR7vZQ9hL");
    assert!(path.to_string_lossy().contains("spore/b3.3yMR7vZQ9hL"));
}

#[test]
fn test_cache_dir_default_ttl_values() {
    let temp = TempDir::new().unwrap();
    let cache = CacheDir::with_root(temp.path().to_path_buf());
    assert_eq!(cache.cmn_ttl_ms, 300 * 1000);
    assert_eq!(cache.spore_max_download_bytes, 1024 * 1024 * 1024);
    assert_eq!(cache.spore_max_extract_bytes, 512 * 1024 * 1024);
    assert_eq!(cache.spore_max_extract_files, 100_000);
    assert_eq!(cache.spore_max_extract_file_bytes, 256 * 1024 * 1024);
    assert_eq!(
        cache.spore_reject_path_components,
        vec![".git".to_string(), ".cmn".to_string()]
    );
}

#[test]
fn test_cache_dir_custom_ttl() {
    let temp = TempDir::new().unwrap();
    let cache = CacheDir {
        root: temp.path().to_path_buf(),
        cmn_ttl_ms: 10_000,
        spore_max_download_bytes: 1024 * 1024 * 1024,
        spore_max_extract_bytes: 512 * 1024 * 1024,
        spore_max_extract_files: 100_000,
        spore_max_extract_file_bytes: 256 * 1024 * 1024,
        spore_reject_path_components: vec![".git".to_string(), ".cmn".to_string()],
    };
    assert_eq!(cache.cmn_ttl_ms, 10_000);
}

#[test]
fn test_cache_dir_from_config_file() {
    let _lock = crate::config::ENV_LOCK.lock().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let hypha_dir = dir.path().join("hypha");
    std::fs::create_dir_all(&hypha_dir).unwrap();
    std::fs::write(hypha_dir.join("config.toml"), "[cache]\ncmn_ttl_s = 30\n").unwrap();

    std::env::set_var("CMN_HOME", dir.path().to_str().unwrap());
    let cache = CacheDir::new().unwrap();
    std::env::remove_var("CMN_HOME");

    assert_eq!(cache.cmn_ttl_ms, 30 * 1000);
}

#[test]
fn test_cache_dir_from_config_custom_path() {
    let _lock = crate::config::ENV_LOCK.lock().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let custom_cache = dir.path().join("my-custom-cache");
    let hypha_dir = dir.path().join("hypha");
    std::fs::create_dir_all(&hypha_dir).unwrap();
    std::fs::write(
        hypha_dir.join("config.toml"),
        format!("[cache]\npath = \"{}\"\n", custom_cache.display()),
    )
    .unwrap();

    std::env::set_var("CMN_HOME", dir.path().to_str().unwrap());
    let cache = CacheDir::new().unwrap();
    std::env::remove_var("CMN_HOME");

    assert_eq!(cache.root, custom_cache);
}

#[test]
fn test_fetch_status_is_fresh_respects_ttl() {
    let status = FetchStatus::success();
    assert!(status.is_fresh(1000));
    assert!(status.is_fresh(3_600_000));
    assert!(!status.is_fresh(0));
}

#[test]
fn test_taste_verdict_roundtrip() {
    let temp = TempDir::new().unwrap();
    let cache = CacheDir::with_root(temp.path().to_path_buf());
    let domain = cache.domain("example.com");

    let verdict = TasteVerdictCache {
        verdict: substrate::TasteVerdict::Safe,
        notes: Some("Reviewed source code".to_string()),
        tasted_at_epoch_ms: 1700000000000,
    };

    domain.save_taste("b3.3yMR7vZQ9hL", &verdict).unwrap();
    let loaded = domain.load_taste("b3.3yMR7vZQ9hL").unwrap();

    assert_eq!(loaded.verdict, substrate::TasteVerdict::Safe);
    assert_eq!(loaded.notes, Some("Reviewed source code".to_string()));
    assert_eq!(loaded.tasted_at_epoch_ms, 1700000000000);
}

#[test]
fn test_taste_verdict_not_found() {
    let temp = TempDir::new().unwrap();
    let cache = CacheDir::with_root(temp.path().to_path_buf());
    let domain = cache.domain("example.com");

    assert!(domain.load_taste("b3.nonexistent").is_none());
}

#[test]
fn test_status_update() {
    let temp = TempDir::new().unwrap();
    let cache = CacheDir::with_root(temp.path().to_path_buf());
    let domain = cache.domain("example.com");

    domain.update_cmn_status(false, Some("404 not found"));
    let status = domain.load_status();
    assert!(status.cmn.failed_at_epoch_ms.is_some());
    assert_eq!(status.cmn.error, Some("404 not found".to_string()));
}

#[test]
fn test_key_trust_retirement_cutoff() {
    let temp = TempDir::new().unwrap();
    let cache = CacheDir::with_root(temp.path().to_path_buf());
    let domain = cache.domain("example.com");
    let retired_at = 1_710_000_000_000;

    domain
        .save_key_trust_with_retirement("ed25519.previous", Some(retired_at))
        .unwrap();

    assert!(domain.is_key_trusted_for_time("ed25519.previous", retired_at, 60_000, 0));
    assert!(domain.is_key_trusted_for_time("ed25519.previous", retired_at - 1, 60_000, 0));
    assert!(!domain.is_key_trusted_for_time("ed25519.previous", retired_at + 1, 60_000, 0));
}

#[test]
fn test_domain_state_pins_first_verified_cmn() {
    let temp = TempDir::new().unwrap();
    let cache = CacheDir::with_root(temp.path().to_path_buf());
    let domain = cache.domain("example.com");
    let (_, key) = keypair(1);
    let entry = cmn_entry(1, key.clone());

    domain.validate_and_pin_cmn_state(&entry).unwrap();
    let pin = domain.load_domain_state().unwrap();
    assert_eq!(pin.serial, 1);
    assert_eq!(pin.current_key, key);
    assert_eq!(pin.capsules_digest, entry.capsules_digest().unwrap());
}

#[test]
fn test_domain_state_rejects_rollback() {
    let temp = TempDir::new().unwrap();
    let cache = CacheDir::with_root(temp.path().to_path_buf());
    let domain = cache.domain("example.com");
    let (_, key) = keypair(2);

    domain
        .validate_and_pin_cmn_state(&cmn_entry(2, key.clone()))
        .unwrap();
    let err = domain
        .validate_and_pin_cmn_state(&cmn_entry(1, key))
        .unwrap_err();
    assert_eq!(err.code, "domain_state_rollback");
}

#[test]
fn test_domain_state_rejects_same_serial_equivocation() {
    let temp = TempDir::new().unwrap();
    let cache = CacheDir::with_root(temp.path().to_path_buf());
    let domain = cache.domain("example.com");
    let (_, key) = keypair(3);
    let entry = cmn_entry(1, key.clone());
    domain.validate_and_pin_cmn_state(&entry).unwrap();

    let mut fork = cmn_entry(1, key);
    fork.capsules[0].endpoints[0].url = "https://fork.example/cmn/taste/{hash}.json".to_string();
    let err = domain.validate_and_pin_cmn_state(&fork).unwrap_err();
    assert_eq!(err.code, "domain_state_equivocation");
}

#[test]
fn test_domain_state_rejects_large_forward_jump() {
    let temp = TempDir::new().unwrap();
    let cache = CacheDir::with_root(temp.path().to_path_buf());
    let domain = cache.domain("example.com");
    let (_, key) = keypair(4);

    domain
        .validate_and_pin_cmn_state(&cmn_entry(1, key.clone()))
        .unwrap();
    let err = domain
        .validate_and_pin_cmn_state(&cmn_entry(1 + DOMAIN_STATE_JUMP_THRESHOLD + 1, key))
        .unwrap_err();
    assert_eq!(err.code, "domain_state_jump");
}

#[test]
fn test_domain_state_allows_proven_key_rotation() {
    let temp = TempDir::new().unwrap();
    let cache = CacheDir::with_root(temp.path().to_path_buf());
    let domain = cache.domain("example.com");
    let (old_private, old_key) = keypair(5);
    let (_, new_key) = keypair(6);

    domain
        .validate_and_pin_cmn_state(&cmn_entry(1, old_key.clone()))
        .unwrap();
    let mut rotated = cmn_entry(2, new_key.clone());
    rotated.capsules[0].history = vec![rotation_entry(&old_private, &old_key, &new_key, 2)];
    domain.validate_and_pin_cmn_state(&rotated).unwrap();
    assert_eq!(domain.load_domain_state().unwrap().current_key, new_key);
}

#[test]
fn test_domain_state_rejects_unproven_key_rotation() {
    let temp = TempDir::new().unwrap();
    let cache = CacheDir::with_root(temp.path().to_path_buf());
    let domain = cache.domain("example.com");
    let (_, old_key) = keypair(7);
    let (_, new_key) = keypair(8);

    domain
        .validate_and_pin_cmn_state(&cmn_entry(1, old_key))
        .unwrap();
    let err = domain
        .validate_and_pin_cmn_state(&cmn_entry(2, new_key))
        .unwrap_err();
    assert_eq!(err.code, "domain_key_rotation_unproven");
}
