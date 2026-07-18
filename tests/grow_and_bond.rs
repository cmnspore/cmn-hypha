#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod common;
use common::*;
use std::fs;

#[test]
fn test_spawn_bond_flag_help() {
    let env = TestEnv::new();
    let output = env.hypha(&["spawn", "--help"]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("--bond"),
        "spawn --help should show --bond flag: {}",
        stdout
    );
}

#[test]
fn test_grow_bond_flag_help() {
    let env = TestEnv::new();
    let output = env.hypha(&["grow", "--help"]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("--bond"),
        "grow --help should show --bond flag: {}",
        stdout
    );
}

#[test]
fn test_grow_synapse_flags_help() {
    let env = TestEnv::new();
    let output = env.hypha(&["grow", "--help"]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("--synapse"),
        "grow --help should show --synapse flag: {}",
        stdout
    );
    assert!(
        stdout.contains("--synapse-token-secret"),
        "grow --help should show --synapse-token-secret flag: {}",
        stdout
    );
}

#[test]
fn test_grow_requires_spore_core() {
    // grow in an empty directory should fail with meaningful error
    let env = TestEnv::new();
    let work_dir = env.dir.join("empty-project");
    fs::create_dir_all(&work_dir).expect("create work dir");
    let output = env.hypha_in_dir(&["grow"], &work_dir);
    assert!(
        !output.status.success(),
        "grow should fail without spore.core.json"
    );
    let stderr = combined_text(&output);
    assert!(
        stderr.contains("spore.core.json"),
        "error should mention spore.core.json: {}",
        stderr
    );
}

#[test]
fn test_grow_requires_spawned_from() {
    // grow with spore.core.json but no .cmn/spawned-from/spore.json should fail
    let env = TestEnv::new();
    let work_dir = env.dir.join("no-spawn-ref");
    fs::create_dir_all(&work_dir).expect("create work dir");
    fs::write(
        work_dir.join("spore.core.json"),
        r#"{"$schema":"https://cmn.dev/schemas/v1/spore-core.json","name":"test","domain":"example.com","key":"ed25519.5XmkQ9vZP8nL3xJdFtR7wNcA6sY2bKgU1eH9pXb4","synopsis":"test","intent":[],"license":"MIT","bonds":[],"tree":{"algorithm":"blob_tree_blake3_nfc"}}"#,
    )
    .expect("write spore.core.json");
    let output = env.hypha_in_dir(&["grow"], &work_dir);
    assert!(
        !output.status.success(),
        "grow should fail without spawned_from"
    );
    let stderr = combined_text(&output);
    assert!(
        stderr.contains("spawned_from") || stderr.contains("Not a spawned spore"),
        "error should mention spawned_from: {}",
        stderr
    );
}

#[test]
fn test_grow_reads_spawned_from_uri() {
    // grow should read the spawned_from URI from spore.core.json
    // and try to resolve synapse (which will fail without config)
    let env = TestEnv::new();
    let work_dir = env.dir.join("with-spawn-ref");
    fs::create_dir_all(&work_dir).expect("create work dir");
    fs::write(
        work_dir.join("spore.core.json"),
        r#"{
            "name": "test",
            "bonds": [
                {"uri": "cmn://example.com/b3.3yMR7vZQ9hL", "relation": "spawned_from"}
            ]
        }"#,
    )
    .expect("write spore.core.json");
    let output = env.hypha_in_dir(&["grow"], &work_dir);
    // Should fail at synapse/mycelium resolution, not at spore.core.json parsing
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        !stdout.contains("spore.core.json not found"),
        "should get past spore.core.json reading: {}",
        stdout
    );
    // Should have progressed past step 1 (RESOLVE)
    let stderr = combined_text(&output);
    assert!(
        stderr.contains("progress"),
        "should emit progress messages: {}",
        stderr
    );
}

#[test]
fn test_bond_help() {
    let env = TestEnv::new();
    let output = env.hypha(&["bond", "--help"]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("--clean"),
        "bond-fetch --help should show --clean: {}",
        stdout
    );
    assert!(
        stdout.contains("--status"),
        "bond-fetch --help should show --status: {}",
        stdout
    );
}

#[test]
fn test_replicate_help() {
    let env = TestEnv::new();
    let output = env.hypha(&["replicate", "--help"]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("--domain"),
        "replicate --help should show --domain: {}",
        stdout
    );
    assert!(
        stdout.contains("--refs"),
        "replicate --help should show --refs: {}",
        stdout
    );
}

// --- §3: load_draft must hard-error on a corrupt spore.core.json ---

#[test]
fn test_hatch_corrupt_spore_core_hard_errors_and_preserves_file() {
    let env = TestEnv::new();
    let work_dir = env.dir.join("corrupt-spore");
    fs::create_dir_all(&work_dir).expect("create work dir");
    let corrupt = "{ this is not valid json ";
    fs::write(work_dir.join("spore.core.json"), corrupt).expect("write corrupt spore.core.json");

    let output = env.hypha_in_dir(&["hatch", "--name", "x"], &work_dir);
    assert!(
        !output.status.success(),
        "hatch should fail on corrupt spore.core.json: {}",
        combined_text(&output)
    );

    let text = combined_text(&output);
    assert!(
        text.contains("spore_parse_failed"),
        "should report spore_parse_failed: {}",
        text
    );

    let after =
        fs::read_to_string(work_dir.join("spore.core.json")).expect("read spore.core.json back");
    assert_eq!(
        after, corrupt,
        "corrupt file must be left byte-for-byte untouched by the failed hatch"
    );
}

#[test]
fn test_hatch_missing_spore_core_bootstraps_default() {
    let env = TestEnv::new();

    // A resolvable domain so the write side (schema validation + key lookup)
    // succeeds too — this exercises load_draft's missing-file bootstrap path
    // end-to-end, not just parsing in isolation.
    env.hypha(&[
        "mycelium",
        "root",
        "test.local",
        "--endpoints-base",
        "https://test.local",
    ]);

    let work_dir = env.dir.join("fresh-spore");
    fs::create_dir_all(&work_dir).expect("create work dir");

    let output = env.hypha_in_dir(
        &[
            "hatch",
            "--name",
            "x",
            "--domain",
            "test.local",
            "--intent",
            "test",
        ],
        &work_dir,
    );
    assert!(
        output.status.success(),
        "hatch should bootstrap a default draft when spore.core.json is missing: {}",
        combined_text(&output)
    );
    assert!(
        work_dir.join("spore.core.json").exists(),
        "hatch should have written a fresh spore.core.json"
    );
}

// --- §2: bond set --with must replace, not merge ---

#[test]
fn test_bond_set_with_replaces_existing_with_object() {
    let env = TestEnv::new();
    let work_dir = env.dir.join("bond-replace");
    fs::create_dir_all(&work_dir).expect("create work dir");
    fs::write(
        work_dir.join("spore.core.json"),
        r#"{
            "name": "test",
            "domain": "example.com",
            "key": "ed25519.5XmkQ9vZP8nL3xJdFtR7wNcA6sY2bKgU1eH9pXb4",
            "synopsis": "test",
            "intent": [],
            "license": "MIT",
            "bonds": [
                {"uri": "cmn://example.com/b3.abc", "relation": "follows", "with": {"a": "x", "b": "y"}}
            ],
            "tree": {"algorithm": "blob_tree_blake3_nfc"}
        }"#,
    )
    .expect("write spore.core.json");

    let output = env.hypha_in_dir(
        &[
            "hatch",
            "bond",
            "set",
            "--uri",
            "cmn://example.com/b3.abc",
            "--with",
            "a=1",
        ],
        &work_dir,
    );
    assert!(
        output.status.success(),
        "bond set should succeed: {}",
        combined_text(&output)
    );

    let saved: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(work_dir.join("spore.core.json")).expect("read spore.core.json"),
    )
    .expect("parse spore.core.json");
    let with = saved["bonds"][0]["with"].clone();
    assert_eq!(
        with,
        serde_json::json!({"a": "1"}),
        "with must be replaced wholesale (b dropped), got: {}",
        with
    );
}

// --- §4: --with KEY=VALUE must be zero-coercion (bare value is always a string) ---

#[test]
fn test_bond_set_with_bare_value_is_always_a_string() {
    let env = TestEnv::new();
    let work_dir = env.dir.join("bond-no-coercion");
    fs::create_dir_all(&work_dir).expect("create work dir");
    fs::write(
        work_dir.join("spore.core.json"),
        r#"{"name":"test","domain":"example.com","key":"ed25519.5XmkQ9vZP8nL3xJdFtR7wNcA6sY2bKgU1eH9pXb4","synopsis":"test","intent":[],"license":"MIT","bonds":[],"tree":{"algorithm":"blob_tree_blake3_nfc"}}"#,
    )
    .expect("write spore.core.json");

    let output = env.hypha_in_dir(
        &[
            "hatch",
            "bond",
            "set",
            "--uri",
            "cmn://example.com/b3.def",
            "--relation",
            "follows",
            "--with",
            "x=true",
            "--with",
            "y=007",
            "--with",
            "z=[1,2]",
        ],
        &work_dir,
    );
    assert!(
        output.status.success(),
        "bond set should succeed: {}",
        combined_text(&output)
    );

    let saved: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(work_dir.join("spore.core.json")).expect("read spore.core.json"),
    )
    .expect("parse spore.core.json");
    let with = saved["bonds"][0]["with"].clone();
    assert_eq!(with["x"], serde_json::json!("true"), "got: {}", with);
    assert_eq!(with["y"], serde_json::json!("007"), "got: {}", with);
    assert_eq!(with["z"], serde_json::json!("[1,2]"), "got: {}", with);
}
