#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod common;
use common::*;
use std::fs;

// ═══════════════════════════════════════════
// Version output
// ═══════════════════════════════════════════

#[test]
fn test_version_json_output() {
    let env = TestEnv::new();
    let output = env.hypha(&["--version", "--output", "json"]);
    let text = combined_text(&output);
    let json = parse_json_last_line(&text);
    assert!(agent_first_data::validate_protocol_event(&json, true).is_ok());
    assert_eq!(
        json["kind"], "result",
        "version should output a result event: {}",
        text
    );
    assert!(
        json["result"]["version"].is_string(),
        "version should include version string: {}",
        text
    );
}

#[test]
fn test_root_help_lists_direct_discovery_commands() {
    let env = TestEnv::new();
    let output = env.hypha(&["--help"]);
    let text = combined_text(&output);
    assert!(output.status.success(), "help should succeed: {}", text);
    let json = parse_json_last_line(&text);
    assert!(agent_first_data::validate_protocol_event(&json, true).is_ok());
    assert_eq!(json["kind"], "result", "help should be a result: {}", text);
    assert_eq!(
        json["result"]["code"], "help",
        "help should use the standard result code: {}",
        text
    );
    let help = &json["result"]["help"];
    assert_eq!(help["schema"], "cli-help-v2", "bad help schema: {}", text);
    assert_eq!(
        help["command_path"], "hypha",
        "root help should identify the selected command: {}",
        text
    );
    let subcommands = help["subcommands"]
        .as_array()
        .expect("root help should list subcommands");
    assert!(
        subcommands
            .iter()
            .any(|command| command == "hypha skill --help"),
        "root help should include the skill command: {}",
        text
    );
    assert!(
        subcommands.iter().all(serde_json::Value::is_string),
        "help-v2 subcommands should be directly callable discovery commands: {}",
        text
    );
}

#[test]
fn test_docs_render_the_whole_registry() {
    let env = TestEnv::new();
    let output = env.hypha(&["--docs"]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "registry docs should succeed: {}",
        stderr
    );
    assert!(stderr.is_empty(), "docs should stay on stdout: {}", stderr);
    assert!(
        stdout.contains("hypha hatch bond set") && stdout.contains("hypha skill install"),
        "docs should render nested commands from the whole registry: {}",
        stdout
    );
}

#[test]
fn test_nested_help_lists_direct_discovery_commands() {
    let env = TestEnv::new();
    let output = env.hypha(&["hatch", "--help"]);
    let text = combined_text(&output);
    assert!(
        output.status.success(),
        "hatch help should succeed: {}",
        text
    );
    let json = parse_json_last_line(&text);
    let help = &json["result"]["help"];
    assert_eq!(help["schema"], "cli-help-v2", "bad help schema: {}", text);
    assert_eq!(
        help["command_path"], "hypha hatch",
        "hatch help should identify its command path: {}",
        text
    );
    let subcommands = help["subcommands"]
        .as_array()
        .expect("hatch help should list subcommands");
    assert!(
        subcommands
            .iter()
            .any(|command| command == "hypha hatch bond --help")
            && subcommands
                .iter()
                .any(|command| command == "hypha hatch tree --help"),
        "hatch help should list its direct child commands: {}",
        text
    );
    assert!(
        subcommands.iter().all(serde_json::Value::is_string),
        "help-v2 subcommands should be directly callable discovery commands: {}",
        text
    );
}

#[test]
fn test_help_subcommand_exits_success() {
    let env = TestEnv::new();
    let output = env.hypha(&["sense", "--help"]);
    let text = combined_text(&output);
    assert!(
        output.status.success(),
        "scoped help should succeed: {}",
        text
    );
    let json = parse_json_last_line(&text);
    assert_eq!(
        json["result"]["code"], "help",
        "scoped help should use the standard result code: {}",
        text
    );
    let help = &json["result"]["help"];
    assert_eq!(
        help["command_path"], "hypha sense",
        "scoped help should identify its command path: {}",
        text
    );
    assert!(
        help["shapes"]
            .as_array()
            .is_some_and(|shapes| shapes.iter().any(|shape| shape["usage"]
                .as_str()
                .is_some_and(|usage| usage.contains("<CMN_URL>")))),
        "sense help should describe its URI argument: {}",
        text
    );
}

#[test]
fn test_explicit_plain_help_is_conventional_text() {
    let env = TestEnv::new();
    let output = env.hypha(&["sense", "--help", "--output", "plain"]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success(),
        "explicit plain help should succeed: {}",
        combined_text(&output)
    );
    assert!(
        stdout.contains("hypha sense") && stdout.contains("<CMN_URL>"),
        "plain help should render the registered invocation shape: {}",
        stdout
    );
}

// ═══════════════════════════════════════════
// Grow / Absorb / Bond / Lineage error paths
// ═══════════════════════════════════════════

#[test]
fn test_grow_not_spawned_dir() {
    let env = TestEnv::new();
    let dir = tempfile::tempdir().unwrap();
    let spore_dir = dir.path().to_path_buf();
    // Create a bare spore.core.json but no .cmn/spawned-from/
    fs::write(
        spore_dir.join("spore.core.json"),
        r#"{"$schema":"https://cmn.dev/schemas/v1/spore-core.json","name":"test","synopsis":"t","intent":["t"],"license":"MIT","tree":{"algorithm":"blob_tree_blake3_nfc","exclude_names":[],"follow_rules":[]}}"#,
    )
    .unwrap();
    let output = env.hypha_in_dir(&["grow"], &spore_dir);
    let text = combined_text(&output);
    assert!(!output.status.success(), "grow should fail: {}", text);
    let json = parse_json_last_line(&text);
    assert_eq!(
        json["error"]["code"], "grow_error",
        "should be grow_error: {}",
        text
    );
}

#[test]
fn test_absorb_no_uri() {
    let env = TestEnv::new();
    let output = env.hypha(&["absorb"]);
    let text = combined_text(&output);
    assert!(
        !output.status.success(),
        "absorb with no args should fail: {}",
        text
    );
}

#[test]
fn test_bond_empty_dir() {
    let env = TestEnv::new();
    let dir = tempfile::tempdir().unwrap();
    let output = env.hypha_in_dir(&["bond"], &dir.path().to_path_buf());
    let text = combined_text(&output);
    assert!(!output.status.success(), "bond should fail: {}", text);
}

#[test]
fn test_lineage_no_synapse() {
    let env = TestEnv::new();
    let output = env.hypha(&["lineage", "cmn://example.com/b3.test"]);
    let text = combined_text(&output);
    assert!(
        !output.status.success(),
        "lineage without synapse should fail: {}",
        text
    );
}
#[test]
fn test_error_returns_json() {
    let env = TestEnv::new();

    // Try to get status of non-existent site
    let output = env.hypha(&["mycelium", "status", "nonexistent.local"]);

    // Should fail
    assert!(!output.status.success());

    // Should output valid JSON error (errors go to stderr)
    let stderr = combined_text(&output);
    assert!(
        stderr.contains("\"error\":"),
        "error should be JSON: {}",
        stderr
    );
    assert!(
        stderr.contains("\"code\":"),
        "error should have code field: {}",
        stderr
    );
    assert!(
        stderr.contains("\"error\""),
        "error should have error message: {}",
        stderr
    );
    let json = parse_json_last_line(&stderr);
    assert!(
        json["error"]["hint"]
            .as_str()
            .is_some_and(|h| !h.is_empty()),
        "error should include an actionable hint: {}",
        stderr
    );
}

#[test]
fn test_cli_parse_error_has_afdata_hint() {
    let env = TestEnv::new();
    let output = env.hypha(&["sense", "cmn://cmn.dev", "--output", "xml"]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !output.status.success(),
        "parse error should fail: {}",
        stderr
    );
    assert!(
        stdout.is_empty(),
        "split routing must keep CLI errors off stdout: {}",
        stdout
    );
    let json = parse_json_last_line(&stderr);
    assert!(agent_first_data::validate_protocol_event(&json, true).is_ok());
    assert_eq!(
        json["kind"], "error",
        "should be an error event: {}",
        stderr
    );
    assert_eq!(
        json["error"]["code"], "cli_invalid_argument_value",
        "should use the precise closed-world CLI error code: {}",
        stderr
    );
    assert_eq!(
        json["error"]["retryable"], false,
        "should not be retryable: {}",
        stderr
    );
    assert!(
        json["error"]["hint"]
            .as_str()
            .is_some_and(|h| !h.is_empty()),
        "should include hint: {}",
        stderr
    );
}

#[test]
fn test_default_split_routes_result_and_diagnostics_separately() {
    let env = TestEnv::new();
    let output = env.hypha(&["config", "list", "--log", "startup"]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "config list failed: {}", stderr);

    let result = parse_json_last_line(&stdout);
    assert_eq!(result["kind"], "result", "bad stdout: {}", stdout);
    assert!(agent_first_data::validate_protocol_event(&result, true).is_ok());

    let log = parse_json_last_line(&stderr);
    assert_eq!(log["kind"], "log", "bad stderr: {}", stderr);
    assert_eq!(log["log"]["level"], "info", "bad stderr: {}", stderr);
    assert!(agent_first_data::validate_protocol_event(&log, true).is_ok());
    assert!(
        !stderr.contains("hypha_version") && !stderr.contains(env!("CARGO_PKG_VERSION")),
        "startup logs must not eagerly disclose version metadata: {}",
        stderr
    );
}

#[test]
fn test_output_to_stdout_collapses_error_stream() {
    let env = TestEnv::new();
    let output = env.hypha(&["sense", "invalid-uri", "--output-to", "stdout"]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success(), "invalid URI should fail");
    assert!(
        stderr.is_empty(),
        "stdout routing must not write stderr: {}",
        stderr
    );
    let event = parse_json_last_line(&stdout);
    assert_eq!(event["kind"], "error", "bad stdout: {}", stdout);
    assert!(agent_first_data::validate_protocol_event(&event, true).is_ok());
}

#[test]
fn test_output_to_stderr_collapses_result_stream() {
    let env = TestEnv::new();
    let output = env.hypha(&["config", "list", "--output-to", "stderr"]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "config list failed: {}", stderr);
    assert!(
        stdout.is_empty(),
        "stderr routing must not write stdout: {}",
        stdout
    );
    let event = parse_json_last_line(&stderr);
    assert_eq!(event["kind"], "result", "bad stderr: {}", stderr);
    assert!(agent_first_data::validate_protocol_event(&event, true).is_ok());
}

#[test]
fn test_invalid_output_to_is_structured_cli_error() {
    let env = TestEnv::new();
    let output = env.hypha(&["config", "list", "--output-to", "sideways"]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !output.status.success(),
        "invalid output target should fail"
    );
    assert!(stdout.is_empty(), "CLI error leaked to stdout: {}", stdout);
    let event = parse_json_last_line(&stderr);
    assert_eq!(
        event["error"]["code"], "cli_invalid_argument_value",
        "bad stderr: {}",
        stderr
    );
    assert!(agent_first_data::validate_protocol_event(&event, true).is_ok());
}

#[test]
fn test_startup_log_redacts_secret_arguments() {
    let env = TestEnv::new();
    let secret = "synapse-test-secret-value";
    let output = env.hypha(&[
        "taste",
        "invalid-uri",
        "--log",
        "startup",
        "--synapse-token-secret",
        secret,
    ]);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success(), "invalid URI should fail");
    assert!(
        !stderr.contains(secret),
        "startup diagnostics exposed a secret: {}",
        stderr
    );
    assert!(
        stderr.contains("***"),
        "startup diagnostics should include an AFDATA redaction marker: {}",
        stderr
    );
    for line in stderr.lines() {
        let event: serde_json::Value =
            serde_json::from_str(line).expect("every diagnostic line must be JSON");
        assert!(agent_first_data::validate_protocol_event(&event, true).is_ok());
        if event["kind"] == "log" {
            assert_eq!(event["log"]["args"]["cmn_url"], "invalid-uri");
            assert!(
                event["log"]["args"].get("uri").is_none(),
                "Hypha startup diagnostics must use the AFDATA outer name: {}",
                stderr
            );
        }
    }
}

#[test]
fn test_startup_log_redacts_synapse_url_selector() {
    let env = TestEnv::new();
    let password = "selector-password-canary";
    let query_secret = "selector-query-canary";
    let synapse = format!("https://user:{password}@127.0.0.1:1?token_secret={query_secret}");
    let output = env.hypha(&["search", "probe", "--log", "startup", "--synapse", &synapse]);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success(), "credentialed URL should fail");
    assert!(
        !stderr.contains(password) && !stderr.contains(query_secret),
        "startup diagnostics exposed URL credentials: {}",
        stderr
    );
    assert!(
        stderr.contains("***"),
        "startup diagnostics should include an AFDATA redaction marker: {}",
        stderr
    );

    let startup: serde_json::Value = serde_json::from_str(
        stderr
            .lines()
            .find(|line| line.contains("\"event\":\"startup\""))
            .expect("startup log must be present"),
    )
    .expect("startup log must be JSON");
    assert_eq!(
        startup["log"]["args"]["synapse_selector"],
        "https://user:***@127.0.0.1:1?token_secret=***"
    );
    assert!(
        startup["log"]["args"].get("synapse").is_none(),
        "Hypha startup diagnostics must use the AFDATA selector name: {}",
        stderr
    );
    assert!(agent_first_data::validate_protocol_event(&startup, true).is_ok());
}

#[test]
fn test_short_help_is_structured_cli_error() {
    let env = TestEnv::new();
    let output = env.hypha(&["-h"]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success(), "short help should be rejected");
    assert!(
        stdout.is_empty(),
        "short help leaked plain stdout: {}",
        stdout
    );
    let event = parse_json_last_line(&stderr);
    assert_eq!(event["kind"], "error", "bad stderr: {}", stderr);
    assert_eq!(
        event["error"]["code"], "cli_unknown_argument",
        "bad stderr: {}",
        stderr
    );
    assert!(agent_first_data::validate_protocol_event(&event, true).is_ok());
}

#[test]
fn test_skill_install_status_uninstall_custom_dir() {
    let env = TestEnv::new();
    let dir = tempfile::tempdir().unwrap();
    let skills_dir = dir.path().to_string_lossy().to_string();

    let install = env.hypha(&[
        "skill",
        "install",
        "--agent",
        "codex",
        "--skills-dir",
        &skills_dir,
    ]);
    let text = combined_text(&install);
    assert!(install.status.success(), "skill install failed: {}", text);
    let json = parse_json_last_line(&text);
    assert!(agent_first_data::validate_protocol_event(&json, true).is_ok());
    assert_eq!(
        json["result"]["code"], "skill_install",
        "bad install output: {}",
        text
    );
    assert_eq!(
        json["result"]["installed"], true,
        "install should report success: {}",
        text
    );
    assert!(
        json["result"]["hint"]
            .as_str()
            .is_some_and(|h| !h.is_empty()),
        "install should include operator hint: {}",
        text
    );
    assert!(dir.path().join("cmn-hypha").join("SKILL.md").is_file());

    let status = env.hypha(&[
        "skill",
        "status",
        "--agent",
        "codex",
        "--skills-dir",
        &skills_dir,
    ]);
    let text = combined_text(&status);
    assert!(status.status.success(), "skill status failed: {}", text);
    let json = parse_json_last_line(&text);
    assert!(agent_first_data::validate_protocol_event(&json, true).is_ok());
    assert_eq!(
        json["result"]["code"], "skill_status",
        "bad status output: {}",
        text
    );
    assert_eq!(
        json["result"]["installed_all"], true,
        "skill should be installed: {}",
        text
    );
    assert_eq!(
        json["result"]["current_all"], true,
        "skill should be current: {}",
        text
    );

    let uninstall = env.hypha(&[
        "skill",
        "uninstall",
        "--agent",
        "codex",
        "--skills-dir",
        &skills_dir,
    ]);
    let text = combined_text(&uninstall);
    assert!(
        uninstall.status.success(),
        "skill uninstall failed: {}",
        text
    );
    let json = parse_json_last_line(&text);
    assert!(agent_first_data::validate_protocol_event(&json, true).is_ok());
    assert_eq!(
        json["result"]["code"], "skill_uninstall",
        "bad uninstall output: {}",
        text
    );
    assert_eq!(
        json["result"]["removed_any"], true,
        "skill should be removed: {}",
        text
    );
    assert!(!dir.path().join("cmn-hypha").join("SKILL.md").exists());
}

#[test]
fn test_exit_codes() {
    let env = TestEnv::new();

    // Successful operation
    let output = env.hypha(&["mycelium", "root", "test.local"]);
    assert_eq!(
        output.status.code(),
        Some(0),
        "success should return exit code 0"
    );

    // Second root also succeeds (updates the site)
    let output = env.hypha(&["mycelium", "root", "test.local"]);
    assert_eq!(
        output.status.code(),
        Some(0),
        "update should also return exit code 0"
    );

    // Failed operation (invalid command)
    let output = env.hypha(&["mycelium", "invalid-command"]);
    assert_ne!(
        output.status.code(),
        Some(0),
        "invalid command should return non-zero exit code"
    );
}

#[test]
fn test_sense_invalid_uri() {
    let env = TestEnv::new();

    // sense with invalid URI should fail
    let output = env.hypha(&["sense", "invalid-uri"]);

    assert!(
        !output.status.success(),
        "sense should fail with invalid URI"
    );

    let stderr = combined_text(&output);
    assert!(
        stderr.contains("\"error\":"),
        "should return error: {}",
        stderr
    );
}

#[test]
fn test_sense_without_network() {
    let env = TestEnv::new();

    // Set up a site with a released spore
    env.hypha(&[
        "mycelium",
        "root",
        "test.local",
        "--endpoints-base",
        "https://test.local",
    ]);

    let spore_dir = env.dir.join("spore");
    fs::create_dir_all(&spore_dir).unwrap();
    fs::write(spore_dir.join("index.js"), "console.log('test');").unwrap();

    env.hypha_in_dir(
        &[
            "hatch",
            "--domain",
            "test.local",
            "--id",
            "test-spore",
            "--name",
            "test",
            "--intent",
            "Test release",
        ],
        &spore_dir,
    );

    // Release the spore
    let output = env.hypha_in_dir(&["release", "--domain", "test.local"], &spore_dir);
    assert!(output.status.success());

    // Find the spore hash from generated files
    let spore_dir_path = env.site_dir("test.local").join("public/cmn/spore");
    let spore_files: Vec<_> = fs::read_dir(&spore_dir_path)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.path()
                .file_name()
                .is_some_and(|n| n.to_string_lossy().starts_with("b3."))
        })
        .collect();

    if spore_files.is_empty() {
        return; // Skip if no spore files found
    }

    // Extract hash from filename (b3.xxxx.json -> b3.xxxx)
    let filename = spore_files[0]
        .path()
        .file_stem()
        .unwrap()
        .to_string_lossy()
        .to_string();

    // Create CMN URI
    let uri = format!("cmn://test.local/{}", filename);

    // Sense will fail because we can't fetch cmn.json from network in tests
    let output = env.hypha(&["sense", &uri]);

    let text = combined_text(&output);

    // Should fail with a cmn.json fetch error (network unreachable), not a parse error
    assert!(
        text.contains("cmn_failed") || text.contains("Failed to fetch"),
        "should fail at cmn.json fetch stage, got: {}",
        text
    );
}

#[test]
fn test_taste_invalid_uri() {
    let env = TestEnv::new();

    // taste with invalid URI should fail
    let output = env.hypha(&["taste", "invalid-uri"]);

    assert!(
        !output.status.success(),
        "taste should fail with invalid URI"
    );

    let stderr = combined_text(&output);
    assert!(
        stderr.contains("\"error\":"),
        "should return error: {}",
        stderr
    );
}

#[test]
fn test_taste_record_not_cached() {
    let env = TestEnv::new();

    // Try to record a taste verdict for a spore that isn't cached
    let output = env.hypha(&[
        "taste",
        "cmn://example.com/b3.111111111111111111111111111111111111111111",
        "--verdict",
        "safe",
    ]);

    assert!(
        !output.status.success(),
        "taste record should fail for non-cached spore"
    );

    let stderr = combined_text(&output);
    assert!(
        stderr.contains("NOT_CACHED"),
        "should return NOT_CACHED: {}",
        stderr
    );
}

#[test]
fn test_taste_invalid_verdict() {
    let env = TestEnv::new();

    // Try to record an invalid taste verdict
    let output = env.hypha(&[
        "taste",
        "cmn://example.com/b3.111111111111111111111111111111111111111111",
        "--verdict",
        "yummy",
    ]);

    assert!(
        !output.status.success(),
        "taste should fail with invalid verdict"
    );

    let stderr = combined_text(&output);
    assert!(
        stderr.contains("invalid value for `--verdict`"),
        "should explain invalid verdict value: {}",
        stderr
    );
    assert!(
        stderr.contains("sweet, fresh, safe, rotten, toxic"),
        "should list accepted verdicts: {}",
        stderr
    );
}
