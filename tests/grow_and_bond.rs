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
