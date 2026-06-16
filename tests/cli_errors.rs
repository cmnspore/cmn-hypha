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
    let output = env.hypha(&["--version"]);
    let text = combined_text(&output);
    let json = parse_json_last_line(&text);
    assert_eq!(json["code"], "ok", "version should output ok: {}", text);
    assert!(
        json["result"]["version"].is_string(),
        "version should include version string: {}",
        text
    );
}

#[test]
fn test_root_help_is_top_level_only() {
    let env = TestEnv::new();
    let output = env.hypha(&["--help"]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "help should succeed: {}", stdout);
    assert!(
        stdout.contains("Commands:"),
        "root help should list first-level commands: {}",
        stdout
    );
    assert!(
        stdout.contains("  skill"),
        "root help should include the skill command: {}",
        stdout
    );
    assert!(
        !stdout.contains("═"),
        "root help should not recursively expand all commands: {}",
        stdout
    );
    assert!(
        !stdout.contains("URI types:"),
        "root help should not include subcommand detail sections: {}",
        stdout
    );
}

#[test]
fn test_recursive_help_is_recursive() {
    let env = TestEnv::new();
    let output = env.hypha(&["--help", "--recursive"]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success(),
        "recursive help should succeed: {}",
        stdout
    );
    assert!(
        stdout.contains("═"),
        "recursive help should retain recursive command sections: {}",
        stdout
    );
    assert!(
        stdout.contains("URI types:"),
        "recursive help should include subcommand detail sections: {}",
        stdout
    );
}

#[test]
fn test_nested_help_is_single_layer() {
    let env = TestEnv::new();
    let output = env.hypha(&["hatch", "--help"]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success(),
        "hatch help should succeed: {}",
        stdout
    );
    assert!(
        stdout.contains("Commands:"),
        "hatch help should list direct child commands: {}",
        stdout
    );
    assert!(
        !stdout.contains("═") && !stdout.contains("Usage: set"),
        "hatch help should not recursively expand grandchildren: {}",
        stdout
    );
}

#[test]
fn test_help_subcommand_exits_success() {
    let env = TestEnv::new();
    let output = env.hypha(&["sense", "--help"]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success(),
        "scoped help should succeed: {}",
        combined_text(&output)
    );
    assert!(
        stdout.contains("Usage:"),
        "scoped help should print normal help: {}",
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
    assert_eq!(json["code"], "grow_error", "should be grow_error: {}", text);
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
        json["hint"].as_str().is_some_and(|h| !h.is_empty()),
        "error should include an actionable hint: {}",
        stderr
    );
}

#[test]
fn test_cli_parse_error_has_afdata_hint() {
    let env = TestEnv::new();
    let output = env.hypha(&["--output", "xml", "sense", "cmn://cmn.dev"]);
    let text = combined_text(&output);
    assert!(
        !output.status.success(),
        "parse error should fail: {}",
        text
    );
    let json = parse_json_last_line(&text);
    assert_eq!(json["code"], "error", "should be an error: {}", text);
    assert_eq!(
        json["error_code"], "invalid_request",
        "should use standard CLI error shape: {}",
        text
    );
    assert_eq!(
        json["retryable"], false,
        "should not be retryable: {}",
        text
    );
    assert!(
        json["trace"]["duration_ms"].is_number(),
        "should include trace duration: {}",
        text
    );
    assert!(
        json["hint"].as_str().is_some_and(|h| !h.is_empty()),
        "should include hint: {}",
        text
    );
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
    assert_eq!(
        json["code"], "skill_install",
        "bad install output: {}",
        text
    );
    assert_eq!(
        json["installed"], true,
        "install should report success: {}",
        text
    );
    assert!(
        json["hint"].as_str().is_some_and(|h| !h.is_empty()),
        "install should include operator hint: {}",
        text
    );
    assert!(dir.path().join("hypha").join("SKILL.md").is_file());

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
    assert_eq!(json["code"], "skill_status", "bad status output: {}", text);
    assert_eq!(
        json["installed_all"], true,
        "skill should be installed: {}",
        text
    );
    assert_eq!(
        json["current_all"], true,
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
    assert_eq!(
        json["code"], "skill_uninstall",
        "bad uninstall output: {}",
        text
    );
    assert_eq!(
        json["removed_any"], true,
        "skill should be removed: {}",
        text
    );
    assert!(!dir.path().join("hypha").join("SKILL.md").exists());
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
        stderr.contains("invalid value 'yummy' for '--verdict <VERDICT>'"),
        "should explain invalid verdict value: {}",
        stderr
    );
    assert!(
        stderr.contains("sweet, fresh, safe, rotten, toxic"),
        "should list accepted verdicts: {}",
        stderr
    );
}
