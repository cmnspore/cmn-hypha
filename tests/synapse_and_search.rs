#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod common;
use common::*;
use std::fs;

// ═══════════════════════════════════════════
// Synapse subcommand tests
// ═══════════════════════════════════════════

#[test]
fn test_synapse_list_empty() {
    let env = TestEnv::new();

    let output = env.hypha(&["synapse", "list"]);
    assert!(
        output.status.success(),
        "synapse list failed: {}",
        combined_text(&output)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    let json: serde_json::Value = parse_json_last_line(&stdout);
    assert_eq!(json["code"], "ok");
    assert_eq!(json["result"]["count"], 0);
    assert_eq!(json["result"]["nodes"], serde_json::json!([]));
    assert_eq!(json["result"]["default"], serde_json::Value::Null);
}

#[test]
fn test_synapse_add() {
    let env = TestEnv::new();

    let output = env.hypha(&["synapse", "add", "https://synapse.cmn.dev"]);
    assert!(
        output.status.success(),
        "synapse add failed: {}",
        combined_text(&output)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    let json: serde_json::Value = parse_json_last_line(&stdout);
    assert_eq!(json["code"], "ok");
    assert_eq!(json["result"]["domain"], "synapse.cmn.dev");
    assert_eq!(json["result"]["url"], "https://synapse.cmn.dev");
    // First node becomes default
    assert_eq!(json["result"]["default"], true);

    // Verify per-node config.toml was created
    let node_config = env
        .dir
        .join("hypha")
        .join("synapse")
        .join("synapse.cmn.dev")
        .join("config.toml");
    assert!(
        node_config.exists(),
        "per-node config.toml should be created"
    );

    let content = fs::read_to_string(&node_config).unwrap();
    assert!(content.contains("https://synapse.cmn.dev"));

    // Verify permissions are 0600 (may contain token secrets)
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = fs::metadata(&node_config).unwrap().permissions().mode();
        assert_eq!(
            mode & 0o777,
            0o600,
            "node config.toml should be 0600, got {:o}",
            mode & 0o777
        );
    }
}

#[test]
fn test_synapse_add_rejects_clearnet_http() {
    let env = TestEnv::new();

    let output = env.hypha(&["synapse", "add", "http://synapse.cmn.dev"]);
    assert!(
        !output.status.success(),
        "synapse add should reject clearnet http"
    );

    let text = combined_text(&output);
    assert!(
        text.contains("Insecure cleartext transport rejected"),
        "should explain cleartext rejection: {}",
        text
    );
}

#[test]
fn test_synapse_add_multiple_and_list() {
    let env = TestEnv::new();

    env.hypha(&["synapse", "add", "https://first.example.com"]);
    env.hypha(&["synapse", "add", "https://second.example.com"]);

    let output = env.hypha(&["synapse", "list"]);
    assert!(output.status.success());

    let stdout = String::from_utf8_lossy(&output.stdout);
    let json: serde_json::Value = parse_json_last_line(&stdout);
    assert_eq!(json["result"]["count"], 2);
    // First node should be default
    assert_eq!(json["result"]["default"], "first.example.com");

    let nodes = json["result"]["nodes"].as_array().unwrap();
    assert_eq!(nodes.len(), 2);
}

#[test]
fn test_synapse_remove() {
    let env = TestEnv::new();

    env.hypha(&["synapse", "add", "https://test.example.com"]);

    let output = env.hypha(&["synapse", "remove", "test.example.com"]);
    assert!(
        output.status.success(),
        "synapse remove failed: {}",
        combined_text(&output)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    let json: serde_json::Value = parse_json_last_line(&stdout);
    assert_eq!(json["code"], "ok");
    assert_eq!(json["result"]["removed"], "test.example.com");

    // Verify list is empty
    let list_output = env.hypha(&["synapse", "list"]);
    let list_stdout = String::from_utf8_lossy(&list_output.stdout);
    let list_json: serde_json::Value = parse_json_last_line(&list_stdout);
    assert_eq!(list_json["result"]["count"], 0);
    // Default should be cleared since we removed the default node
    assert_eq!(list_json["result"]["default"], serde_json::Value::Null);
}

#[test]
fn test_synapse_remove_nonexistent() {
    let env = TestEnv::new();

    let output = env.hypha(&["synapse", "remove", "nope.example.com"]);
    assert!(!output.status.success());

    let stderr = combined_text(&output);
    assert!(
        stderr.contains("not found"),
        "should report not found: {}",
        stderr
    );
}

#[test]
fn test_synapse_use() {
    let env = TestEnv::new();

    env.hypha(&["synapse", "add", "https://alpha.example.com"]);
    env.hypha(&["synapse", "add", "https://beta.example.com"]);

    // Switch default to beta
    let output = env.hypha(&["synapse", "use", "beta.example.com"]);
    assert!(
        output.status.success(),
        "synapse use failed: {}",
        combined_text(&output)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    let json: serde_json::Value = parse_json_last_line(&stdout);
    assert_eq!(json["result"]["default"], "beta.example.com");
    assert_eq!(json["result"]["url"], "https://beta.example.com");

    // Verify list shows beta as default
    let list_output = env.hypha(&["synapse", "list"]);
    let list_stdout = String::from_utf8_lossy(&list_output.stdout);
    let list_json: serde_json::Value = parse_json_last_line(&list_stdout);
    assert_eq!(list_json["result"]["default"], "beta.example.com");
}

#[test]
fn test_synapse_use_nonexistent() {
    let env = TestEnv::new();

    let output = env.hypha(&["synapse", "use", "nope.example.com"]);
    assert!(!output.status.success());

    let stderr = combined_text(&output);
    assert!(
        stderr.contains("not found"),
        "should report not found: {}",
        stderr
    );
}

#[test]
fn test_synapse_config_token_set_and_clear() {
    let env = TestEnv::new();

    env.hypha(&["synapse", "add", "https://test.example.com"]);

    // Set token
    let output = env.hypha(&[
        "synapse",
        "config",
        "test.example.com",
        "--token-secret",
        "sk-secret123",
    ]);
    assert!(
        output.status.success(),
        "synapse config token set failed: {}",
        combined_text(&output)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    let json: serde_json::Value = parse_json_last_line(&stdout);
    assert_eq!(json["result"]["token_set"], true);

    // Verify token is stored in per-node config.toml
    let node_config = env
        .dir
        .join("hypha")
        .join("synapse")
        .join("test.example.com")
        .join("config.toml");
    let content = fs::read_to_string(&node_config).unwrap();
    assert!(
        content.contains("sk-secret123"),
        "token should be in node config.toml"
    );

    // Verify list shows has_token=true
    let list_output = env.hypha(&["synapse", "list"]);
    let list_stdout = String::from_utf8_lossy(&list_output.stdout);
    let list_json: serde_json::Value = parse_json_last_line(&list_stdout);
    let node = &list_json["result"]["nodes"][0];
    assert_eq!(node["has_token"], true);

    // Clear token with empty string
    let clear_output = env.hypha(&[
        "synapse",
        "config",
        "test.example.com",
        "--token-secret",
        "",
    ]);
    assert!(clear_output.status.success());

    let clear_stdout = String::from_utf8_lossy(&clear_output.stdout);
    let clear_json: serde_json::Value = parse_json_last_line(&clear_stdout);
    assert_eq!(clear_json["result"]["token_set"], false);

    // Verify token is cleared
    let content2 = fs::read_to_string(&node_config).unwrap();
    assert!(
        !content2.contains("sk-secret123"),
        "token should be cleared"
    );
}

#[test]
fn test_synapse_config_nonexistent_node() {
    let env = TestEnv::new();

    let output = env.hypha(&[
        "synapse",
        "config",
        "nope.example.com",
        "--token-secret",
        "tok",
    ]);
    assert!(!output.status.success());

    let stderr = combined_text(&output);
    assert!(
        stderr.contains("not found"),
        "should report not found: {}",
        stderr
    );
}

#[test]
fn test_synapse_add_overwrites_existing() {
    let env = TestEnv::new();

    // Adding the same domain twice overwrites
    env.hypha(&["synapse", "add", "https://test.example.com"]);
    env.hypha(&["synapse", "add", "https://test.example.com/v2"]);

    let output = env.hypha(&["synapse", "list"]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let json: serde_json::Value = parse_json_last_line(&stdout);
    assert_eq!(json["result"]["count"], 1);
    assert_eq!(
        json["result"]["nodes"][0]["url"],
        "https://test.example.com/v2"
    );
}

#[test]
fn test_synapse_remove_clears_default() {
    let env = TestEnv::new();

    env.hypha(&["synapse", "add", "https://alpha.example.com"]);
    env.hypha(&["synapse", "add", "https://beta.example.com"]);
    env.hypha(&["synapse", "use", "beta.example.com"]);

    // Remove beta (the default)
    env.hypha(&["synapse", "remove", "beta.example.com"]);

    let output = env.hypha(&["synapse", "list"]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let json: serde_json::Value = parse_json_last_line(&stdout);
    assert_eq!(json["result"]["count"], 1);
    // Default should be cleared since beta was removed
    assert_eq!(json["result"]["default"], serde_json::Value::Null);
}

#[test]
fn test_search_no_synapse_configured() {
    let env = TestEnv::new();

    // Search without -s and no default → error
    let output = env.hypha(&["search", "test"]);
    assert!(!output.status.success());

    let stderr = combined_text(&output);
    assert!(
        stderr.contains("No synapse specified") || stderr.contains("no default"),
        "should report no synapse: {}",
        stderr
    );
}

#[test]
fn test_ancestors_no_synapse_configured() {
    let env = TestEnv::new();

    let output = env.hypha(&["lineage", "cmn://cmn.dev/b3.3yMR7vZQ9hL"]);
    assert!(!output.status.success());

    let stderr = combined_text(&output);
    assert!(
        stderr.contains("No synapse specified") || stderr.contains("no default"),
        "should report no synapse: {}",
        stderr
    );
}

#[test]
fn test_search_with_named_synapse_not_found() {
    let env = TestEnv::new();

    let output = env.hypha(&["search", "test", "--synapse", "nonexistent.example.com"]);
    assert!(!output.status.success());

    let stderr = combined_text(&output);
    assert!(
        stderr.contains("not found"),
        "should report node not found: {}",
        stderr
    );
}

#[test]
fn test_search_bonds_flag_accepted() {
    let env = TestEnv::new();

    // --bonds is a valid flag; fails because no synapse is configured, not because of bad args
    let output = env.hypha(&[
        "search",
        "http client",
        "--bonds",
        "spawned_from:cmn://cmn.dev/b3.3yMR7vZQ9hL",
    ]);
    assert!(!output.status.success());

    let stderr = combined_text(&output);
    // Should fail with "no synapse" error, NOT "unexpected argument"
    assert!(
        !stderr.contains("unexpected argument"),
        "--bonds should be accepted as a valid flag: {}",
        stderr
    );
    assert!(
        stderr.contains("synapse") || stderr.contains("No synapse"),
        "should fail because no synapse configured, not arg parsing: {}",
        stderr
    );
}

#[test]
fn test_search_bonds_flag_with_named_synapse() {
    let env = TestEnv::new();

    // --bonds with --synapse pointing to non-existent node → node not found error
    let output = env.hypha(&[
        "search",
        "test",
        "--synapse",
        "nonexistent.example.com",
        "--bonds",
        "follows:cmn://cmn.dev/b3.xyz",
    ]);
    assert!(!output.status.success());

    let stderr = combined_text(&output);
    assert!(
        stderr.contains("not found"),
        "should report synapse node not found: {}",
        stderr
    );
}

#[test]
fn test_search_bonds_flag_comma_separated() {
    let env = TestEnv::new();

    // Multiple bond filters comma-separated — flag parsing should accept it
    let output = env.hypha(&[
        "search",
        "tools",
        "--bonds",
        "spawned_from:cmn://a.dev/b3.aaa,follows:cmn://b.dev/b3.bbb",
    ]);
    assert!(!output.status.success());

    let stderr = combined_text(&output);
    assert!(
        !stderr.contains("unexpected argument"),
        "comma-separated --bonds should be accepted: {}",
        stderr
    );
}

#[test]
fn test_search_help_shows_bonds() {
    let env = TestEnv::new();

    let output = env.hypha(&["search", "--help"]);
    assert!(output.status.success());

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("--bonds"),
        "search help should document --bonds flag: {}",
        stdout
    );
}

#[test]
fn test_synapse_help() {
    let env = TestEnv::new();

    let output = env.hypha(&["synapse", "--help"]);
    assert!(output.status.success());

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Manage Synapse node connections"));
    assert!(stdout.contains("add"));
    assert!(stdout.contains("remove"));
    assert!(stdout.contains("list"));
    assert!(stdout.contains("use"));
    assert!(stdout.contains("config"));
    assert!(stdout.contains("health"));
    assert!(stdout.contains("discover"));
}

#[test]
fn test_synapse_node_directory_structure() {
    let env = TestEnv::new();

    env.hypha(&["synapse", "add", "https://synapse.cmn.dev"]);

    // Verify directory structure: $CMN_HOME/hypha/synapse/<domain>/config.toml
    let node_dir = env
        .dir
        .join("hypha")
        .join("synapse")
        .join("synapse.cmn.dev");
    assert!(node_dir.is_dir(), "node directory should exist");
    assert!(
        node_dir.join("config.toml").exists(),
        "config.toml should exist in node dir"
    );

    // Verify config.toml has defaults.synapse set
    let config_path = env.dir.join("hypha").join("config.toml");
    assert!(config_path.exists(), "hypha config.toml should exist");
    let content = fs::read_to_string(&config_path).unwrap();
    assert!(
        content.contains("synapse.cmn.dev"),
        "config.toml should contain default synapse domain"
    );
}
