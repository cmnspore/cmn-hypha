#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use super::*;

#[test]
fn test_default_values() {
    let cfg = HyphaConfig::default();
    assert_eq!(cfg.cache.cmn_ttl_s, 300);
    assert_eq!(cfg.cache.spore_max_download_bytes, 1024 * 1024 * 1024);
    assert_eq!(cfg.cache.spore_max_extract_bytes, 512 * 1024 * 1024);
    assert_eq!(cfg.cache.spore_max_extract_files, 100_000);
    assert_eq!(cfg.cache.spore_max_extract_file_bytes, 256 * 1024 * 1024);
    assert_eq!(
        cfg.cache.spore_reject_path_components,
        vec![".git".to_string(), ".cmn".to_string()]
    );
    assert!(cfg.defaults.synapse.is_none());
}

#[test]
fn test_parse_full_toml() {
    let toml_str = r#"
[defaults]
synapse = "synapse.cmn.dev"

[cache]
cmn_ttl_s = 60
spore_max_download_bytes = 1073741824
spore_max_extract_bytes = 536870912
spore_max_extract_files = 100000
spore_max_extract_file_bytes = 268435456
spore_reject_path_components = [".git", ".cmn"]
"#;
    let cfg: HyphaConfig = toml::from_str(toml_str).unwrap();
    assert_eq!(cfg.cache.cmn_ttl_s, 60);
    assert_eq!(cfg.cache.spore_max_download_bytes, 1_073_741_824);
    assert_eq!(cfg.cache.spore_max_extract_bytes, 536_870_912);
    assert_eq!(cfg.cache.spore_max_extract_files, 100_000);
    assert_eq!(cfg.cache.spore_max_extract_file_bytes, 268_435_456);
    assert_eq!(
        cfg.cache.spore_reject_path_components,
        vec![".git".to_string(), ".cmn".to_string()]
    );
    assert_eq!(cfg.defaults.synapse.as_deref(), Some("synapse.cmn.dev"));
}

#[test]
fn test_parse_partial_toml_cmn_only() {
    let toml_str = r#"
[cache]
cmn_ttl_s = 10
"#;
    let cfg: HyphaConfig = toml::from_str(toml_str).unwrap();
    assert_eq!(cfg.cache.cmn_ttl_s, 10);
}

#[test]
fn test_parse_empty_toml() {
    let cfg: HyphaConfig = toml::from_str("").unwrap();
    assert_eq!(cfg.cache.cmn_ttl_s, 300);
}

#[test]
fn test_parse_empty_cache_section() {
    let toml_str = "[cache]\n";
    let cfg: HyphaConfig = toml::from_str(toml_str).unwrap();
    assert_eq!(cfg.cache.cmn_ttl_s, 300);
    assert_eq!(
        cfg.cache.spore_reject_path_components,
        vec![".git".to_string(), ".cmn".to_string()]
    );
}

#[test]
fn test_legacy_cache_spore_limit_keys_are_unsupported() {
    let toml_str = r#"
[cache]
max_download_bytes = 1
max_extract_bytes = 1
max_extract_files = 1
max_extract_file_bytes = 1
"#;
    let err = toml::from_str::<HyphaConfig>(toml_str).unwrap_err();
    assert!(
        err.to_string().contains("unknown field"),
        "unexpected error: {}",
        err
    );
}

#[test]
fn test_invalid_toml_falls_back_to_default() {
    let bad_toml = "this is not valid toml {{{{";
    let cfg: HyphaConfig = toml::from_str(bad_toml).unwrap_or_default();
    assert_eq!(cfg.cache.cmn_ttl_s, 300);
}

#[test]
fn test_require_domain_first_key_default_true() {
    let cfg = HyphaConfig::default();
    assert!(cfg.cache.require_domain_first_key);
}

#[test]
fn test_require_domain_first_key_toml_parse() {
    let toml_str = r#"
[cache]
require_domain_first_key = false
"#;
    let cfg: HyphaConfig = toml::from_str(toml_str).unwrap();
    assert!(!cfg.cache.require_domain_first_key);
}

#[test]
fn test_require_domain_first_key_absent_defaults_true() {
    let toml_str = r#"
[cache]
cmn_ttl_s = 60
"#;
    let cfg: HyphaConfig = toml::from_str(toml_str).unwrap();
    assert!(cfg.cache.require_domain_first_key);
}

#[test]
fn test_zero_ttl_allowed() {
    let toml_str = r#"
[cache]
cmn_ttl_s = 0
"#;
    let cfg: HyphaConfig = toml::from_str(toml_str).unwrap();
    assert_eq!(cfg.cache.cmn_ttl_s, 0);
}

#[test]
fn test_config_save_load() {
    let _lock = super::ENV_LOCK.lock().unwrap();
    let dir = tempfile::tempdir().unwrap();
    std::env::set_var("CMN_HOME", dir.path().to_str().unwrap());

    let mut cfg = HyphaConfig::default();
    cfg.defaults.synapse = Some("synapse.cmn.dev".to_string());
    cfg.cache.cmn_ttl_s = 999;
    cfg.save().unwrap();

    let loaded = HyphaConfig::load().unwrap();
    assert_eq!(loaded.defaults.synapse.as_deref(), Some("synapse.cmn.dev"));
    assert_eq!(loaded.cache.cmn_ttl_s, 999);

    std::env::remove_var("CMN_HOME");
}

#[test]
fn test_config_load_invalid_file_returns_error() {
    let _lock = super::ENV_LOCK.lock().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let hypha_dir = dir.path().join("hypha");
    std::fs::create_dir_all(&hypha_dir).unwrap();
    std::fs::write(hypha_dir.join("config.toml"), "this is not valid toml {{{{").unwrap();
    std::env::set_var("CMN_HOME", dir.path().to_str().unwrap());

    let err = HyphaConfig::load().unwrap_err();
    assert_eq!(err.code, "config_parse_failed");
    assert!(err.message.contains("config.toml"));
    assert_eq!(
        err.hint.as_deref(),
        Some("fix the file or remove it to use defaults")
    );

    std::env::remove_var("CMN_HOME");
}

#[test]
fn test_synapse_node_roundtrip() {
    let toml_str = r#"
url = "https://synapse.cmn.dev"
token_secret = "sk-abc123"
"#;
    let node: SynapseNode = toml::from_str(toml_str).unwrap();
    assert_eq!(node.url, "https://synapse.cmn.dev");
    assert_eq!(node.token_secret.as_deref(), Some("sk-abc123"));

    let serialized = toml::to_string_pretty(&node).unwrap();
    let parsed: SynapseNode = toml::from_str(&serialized).unwrap();
    assert_eq!(parsed.url, "https://synapse.cmn.dev");
    assert_eq!(parsed.token_secret.as_deref(), Some("sk-abc123"));
}

#[test]
fn test_synapse_node_no_token() {
    let toml_str = "url = \"https://synapse.cmn.dev\"\n";
    let node: SynapseNode = toml::from_str(toml_str).unwrap();
    assert!(node.token_secret.is_none());

    let serialized = toml::to_string_pretty(&node).unwrap();
    assert!(!serialized.contains("token_secret"));
}

#[test]
fn test_save_load_synapse_node() {
    let _lock = super::ENV_LOCK.lock().unwrap();
    let dir = tempfile::tempdir().unwrap();
    std::env::set_var("CMN_HOME", dir.path().to_str().unwrap());

    let node = SynapseNode {
        url: "https://synapse.cmn.dev".to_string(),
        token_secret: Some("tok".to_string()),
    };
    save_synapse_node("synapse.cmn.dev", &node).unwrap();

    let node_dir = dir
        .path()
        .join("hypha")
        .join("synapse")
        .join("synapse.cmn.dev");
    assert!(node_dir.join("config.toml").exists());

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        let mode = std::fs::metadata(node_dir.join("config.toml"))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(
            mode & 0o777,
            0o600,
            "config.toml should be 0600, got {:o}",
            mode & 0o777
        );
    }

    let loaded = load_synapse_node("synapse.cmn.dev").unwrap();
    assert_eq!(loaded.url, "https://synapse.cmn.dev");
    assert_eq!(loaded.token_secret.as_deref(), Some("tok"));

    std::env::remove_var("CMN_HOME");
}

#[test]
fn test_list_synapse_domains() {
    let _lock = super::ENV_LOCK.lock().unwrap();
    let dir = tempfile::tempdir().unwrap();
    std::env::set_var("CMN_HOME", dir.path().to_str().unwrap());

    save_synapse_node(
        "beta.example.com",
        &SynapseNode {
            url: "https://beta.example.com".to_string(),
            token_secret: None,
        },
    )
    .unwrap();
    save_synapse_node(
        "alpha.example.com",
        &SynapseNode {
            url: "https://alpha.example.com".to_string(),
            token_secret: None,
        },
    )
    .unwrap();

    let domains = list_synapse_domains();
    assert_eq!(domains, vec!["alpha.example.com", "beta.example.com"]);

    std::env::remove_var("CMN_HOME");
}

#[test]
fn test_remove_synapse_node() {
    let _lock = super::ENV_LOCK.lock().unwrap();
    let dir = tempfile::tempdir().unwrap();
    std::env::set_var("CMN_HOME", dir.path().to_str().unwrap());

    save_synapse_node(
        "test.example.com",
        &SynapseNode {
            url: "https://test.example.com".to_string(),
            token_secret: None,
        },
    )
    .unwrap();

    assert!(load_synapse_node("test.example.com").is_some());

    remove_synapse_node("test.example.com").unwrap();
    assert!(load_synapse_node("test.example.com").is_none());
    assert!(list_synapse_domains().is_empty());

    std::env::remove_var("CMN_HOME");
}

#[test]
fn test_domain_from_url() {
    assert_eq!(
        domain_from_url("https://synapse.cmn.dev").unwrap(),
        "synapse.cmn.dev"
    );
    assert_eq!(
        domain_from_url("http://localhost:8080").unwrap(),
        "localhost"
    );
    assert_eq!(
        domain_from_url("https://example.com/path").unwrap(),
        "example.com"
    );
    assert!(domain_from_url("not-a-url").is_err());
    assert!(domain_from_url("https://..").is_err());
}

#[test]
fn test_validate_synapse_url_rejects_clearnet_http() {
    let err = validate_synapse_url("http://synapse.cmn.dev").unwrap_err();
    assert_eq!(err.code, "invalid_synapse_url");
    assert!(err
        .message
        .contains("Insecure cleartext transport rejected"));
}

#[test]
fn test_validate_synapse_url_allows_https_and_anonymous_http() {
    assert!(validate_synapse_url("https://synapse.cmn.dev").is_ok());
    assert!(validate_synapse_url("http://abc123.onion").is_ok());
    assert!(validate_synapse_url("http://site.i2p").is_ok());
}

#[test]
fn test_validate_synapse_url_rejects_known_cleartext_schemes() {
    assert_eq!(
        validate_synapse_url("ws://synapse.cmn.dev")
            .unwrap_err()
            .code,
        "invalid_synapse_url"
    );
    assert_eq!(
        validate_synapse_url("ftp://synapse.cmn.dev")
            .unwrap_err()
            .code,
        "invalid_synapse_url"
    );
}

#[test]
fn test_validate_synapse_url_rejects_ip_literals() {
    for url in ["https://127.0.0.1", "https://[::1]", "https://203.0.113.10"] {
        assert_eq!(
            validate_synapse_url(url).unwrap_err().code,
            "invalid_synapse_url",
            "{} should be rejected",
            url
        );
    }
}

#[test]
fn test_resolve_synapse_env_var_override() {
    let _lock = super::ENV_LOCK.lock().unwrap();
    let dir = tempfile::tempdir().unwrap();
    std::env::set_var("CMN_HOME", dir.path().to_str().unwrap());

    save_synapse_node(
        "test.example.com",
        &SynapseNode {
            url: "https://test.example.com".to_string(),
            token_secret: Some("config-token".to_string()),
        },
    )
    .unwrap();

    std::env::set_var("SYNAPSE_TOKEN_SECRET", "env-token");
    let resolved = resolve_synapse(Some("test.example.com"), None).unwrap();
    assert_eq!(resolved.token_secret.as_deref(), Some("env-token"));

    let resolved = resolve_synapse(Some("test.example.com"), Some("cli-token")).unwrap();
    assert_eq!(resolved.token_secret.as_deref(), Some("cli-token"));

    let resolved = resolve_synapse(Some("test.example.com"), Some("")).unwrap();
    assert!(resolved.token_secret.is_none());

    std::env::remove_var("SYNAPSE_TOKEN_SECRET");

    let resolved = resolve_synapse(Some("test.example.com"), None).unwrap();
    assert_eq!(resolved.token_secret.as_deref(), Some("config-token"));

    std::env::remove_var("CMN_HOME");
}

#[test]
fn test_load_missing_node_returns_none() {
    let _lock = super::ENV_LOCK.lock().unwrap();
    let dir = tempfile::tempdir().unwrap();
    std::env::set_var("CMN_HOME", dir.path().to_str().unwrap());

    assert!(load_synapse_node("nonexistent.example.com").is_none());

    std::env::remove_var("CMN_HOME");
}
