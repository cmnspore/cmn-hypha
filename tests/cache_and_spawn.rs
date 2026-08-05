#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod common;
use common::*;
use std::fs;

#[test]
fn test_cache_list_empty() {
    let env = TestEnv::new();

    // Cache list on empty cache should succeed
    let output = env.hypha(&["cache", "list"]);
    assert!(
        output.status.success(),
        "cache list failed: {}",
        combined_text(&output)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("\"kind\":\"result\""),
        "should return success: {}",
        stdout
    );
}

#[test]
fn test_cache_clean_empty() {
    let env = TestEnv::new();

    // Cache clean on empty cache should succeed
    let output = env.hypha(&["cache", "clean", "--all"]);
    assert!(
        output.status.success(),
        "cache clean failed: {}",
        combined_text(&output)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("\"kind\":\"result\""),
        "should return success: {}",
        stdout
    );
}

#[test]
fn test_cache_path_not_cached() {
    let env = TestEnv::new();

    // Cache path for non-existent spore should fail
    let output = env.hypha(&["cache", "path", "cmn://example.com/b3.3yMR7vZQ9hL"]);

    assert!(
        !output.status.success(),
        "cache path should fail for non-cached spore"
    );

    let stderr = combined_text(&output);
    assert!(
        stderr.contains("\"error\":"),
        "should return error: {}",
        stderr
    );
}

#[test]
fn test_cache_list_with_manual_cache() {
    let env = TestEnv::new();

    // Create a mock cache directory structure (hypha/cache/domain/spore/hash/)
    let cache_dir = env.dir.join("hypha/cache/example.com/spore/b3.3yMR7vZQ9hL");
    fs::create_dir_all(&cache_dir).unwrap();

    // Create a minimal spore.json
    let manifest = serde_json::json!({
        "$schema": "https://cmn.dev/schemas/v1/spore.json",
        "capsule": {
            "uri": "cmn://example.com/b3.3yMR7vZQ9hL",
            "core": {
                "name": "test-spore",
                "domain": "example.com",
                "synopsis": "A test spore",
                "intent": ["Testing"],
                "license": "MIT",
                "mutations": [],
                "size_bytes": 0,
                "updated_at_epoch_ms": 0,
                "bonds": [],
                "tree": {"algorithm": "blob_tree_blake3_nfc", "exclude_names": [], "follow_rules": []}
            },
            "core_signature": "ed25519.fakesig",
            "dist": [{"type": "archive", "filename": "test.tar.zst"}]
        },
        "capsule_signature": "ed25519.fakesig"
    });
    fs::write(
        cache_dir.join("spore.json"),
        serde_json::to_string_pretty(&manifest).unwrap(),
    )
    .unwrap();

    // cache list should show the spore
    let output = env.hypha(&["cache", "list"]);
    assert!(output.status.success());

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("\"spore_count\":1"),
        "should show one cached spore: {}",
        stdout
    );
    assert!(
        stdout.contains("test-spore"),
        "should show spore name: {}",
        stdout
    );
    assert!(
        stdout.contains("example.com"),
        "should show domain: {}",
        stdout
    );
}

#[test]
fn test_cache_path_with_manual_cache() {
    let env = TestEnv::new();

    // Create a mock cache directory structure (hypha/cache/domain/spore/hash/)
    let cache_dir = env
        .dir
        .join("hypha/cache/example.com/spore/b3.8cQnH4xPmZ2v");
    let content_dir = cache_dir.join("content");
    fs::create_dir_all(&content_dir).unwrap();
    fs::write(content_dir.join("README.md"), "# Test").unwrap();

    // Create a minimal spore.json
    let manifest = serde_json::json!({
        "$schema": "https://cmn.dev/schemas/v1/spore.json",
        "capsule": {
            "uri": "cmn://example.com/b3.8cQnH4xPmZ2v",
            "core": {
                "name": "another-spore",
                "domain": "example.com",
                "synopsis": "Another test",
                "intent": ["Testing"],
                "license": "MIT"
            },
            "core_signature": "ed25519.fakesig",
            "dist": [{"type": "archive", "filename": "test.tar.zst"}]
        },
        "capsule_signature": "ed25519.fakesig"
    });
    fs::write(
        cache_dir.join("spore.json"),
        serde_json::to_string_pretty(&manifest).unwrap(),
    )
    .unwrap();

    // cache path should return the content directory
    let output = env.hypha(&["cache", "path", "cmn://example.com/b3.8cQnH4xPmZ2v"]);
    assert!(output.status.success());

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("content"),
        "should return content path: {}",
        stdout
    );
}

#[test]
fn test_cache_clean_all() {
    let env = TestEnv::new();

    // Create a mock cache directory structure (hypha/cache/domain/spore/hash/)
    let cache_dir = env
        .dir
        .join("hypha/cache/example.com/spore/b3.5HueCGU8rMjx");
    fs::create_dir_all(&cache_dir).unwrap();
    fs::write(cache_dir.join("spore.json"), "{}").unwrap();

    // cache clean --all should remove all
    let output = env.hypha(&["cache", "clean", "--all"]);
    assert!(output.status.success());

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("\"removed_count\":1"),
        "should show 1 removed: {}",
        stdout
    );

    // Verify cache is empty
    let output = env.hypha(&["cache", "list"]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("\"spore_count\":0"),
        "should be empty after clean: {}",
        stdout
    );
}

// ============== Directory Exists Error Tests ==============

#[test]
fn test_spawn_dir_exists_error() {
    let env = TestEnv::new();

    // Create a working directory with a pre-existing target directory
    let work_dir = env.dir.join("work");
    fs::create_dir_all(&work_dir).unwrap();

    // Create a directory that would conflict with the spore id
    let target_dir = work_dir.join("test-spore");
    fs::create_dir_all(&target_dir).unwrap();

    // Seed a taste verdict so spawn gets past the taste check
    let taste_dir = env
        .dir
        .join("hypha/cache/example.com/spore/b3.111111111111111111111111111111111111111111");
    fs::create_dir_all(&taste_dir).unwrap();
    fs::write(
        taste_dir.join("taste.json"),
        r#"{"verdict":"safe","tasted_at_epoch_ms":1700000000000}"#,
    )
    .unwrap();

    // Try to spawn to that path (explicitly)
    // The spawn should fail early with DIR_EXISTS before any network operations
    let output = env.hypha_in_dir(
        &[
            "spawn",
            "cmn://example.com/b3.111111111111111111111111111111111111111111",
            "test-spore",
        ],
        &work_dir,
    );

    let stderr = combined_text(&output);

    // The command should fail
    assert!(
        !output.status.success(),
        "spawn should fail, stderr: {}",
        stderr
    );

    // Check it's an error response (errors go to stderr)
    assert!(
        stderr.contains("\"error\":"),
        "should return error: {}",
        stderr
    );

    // Check error code is DIR_EXISTS
    assert!(
        stderr.contains("DIR_EXISTS"),
        "should return DIR_EXISTS error: {}",
        stderr
    );
}
