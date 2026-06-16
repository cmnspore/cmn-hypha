#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod common;
use common::*;
use std::fs;
use std::process::Command;

#[test]
fn test_mycelium_root() {
    let env = TestEnv::new();

    // Default output is JSON
    let output = env.hypha(&["mycelium", "root", "test.local"]);
    assert!(
        output.status.success(),
        "root failed: {}",
        combined_text(&output)
    );

    // Verify JSON output
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("\"code\""),
        "should output JSON code: {}",
        stdout
    );
    assert!(
        stdout.contains("\"public_key\""),
        "should output JSON public_key: {}",
        stdout
    );

    let site_dir = env.site_dir("test.local");
    assert!(
        site_dir.join("keys/private.pem").exists(),
        "private key not found"
    );
    assert!(
        site_dir.join("keys/public.pem").exists(),
        "public key not found"
    );
    assert!(
        site_dir.join("public/.well-known/cmn.json").exists(),
        "cmn.json not found"
    );

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let metadata = fs::metadata(site_dir.join("keys/private.pem")).unwrap();
        let mode = metadata.permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "private key has wrong permissions: {:o}", mode);
    }
}

#[test]
fn test_mycelium_status() {
    let env = TestEnv::new();

    env.hypha(&["mycelium", "root", "test.local"]);

    // Test plain output with --output plain
    let output = env.hypha(&["--output", "plain", "mycelium", "status"]);
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("test.local"),
        "status should list test.local"
    );
}

#[test]
fn test_mycelium_status_json() {
    let env = TestEnv::new();

    env.hypha(&["mycelium", "root", "test.local"]);

    // Default output is JSON (no --output flag needed)
    let output = env.hypha(&["mycelium", "status", "test.local"]);
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("\"public_key\""),
        "JSON output should have public_key: {}",
        stdout
    );
    assert!(
        stdout.contains("\"code\""),
        "JSON output should have code: {}",
        stdout
    );
    assert!(
        stdout.contains("\"spore_count\""),
        "JSON output should have spore_count: {}",
        stdout
    );
}

#[test]
fn test_spore_hatch() {
    let env = TestEnv::new();
    let root_output = env.hypha(&["mycelium", "root", "test.local"]);
    assert!(
        root_output.status.success(),
        "mycelium root failed: {}",
        combined_text(&root_output)
    );

    let spore_dir = env.dir.join("spore");
    fs::create_dir_all(&spore_dir).unwrap();
    fs::write(spore_dir.join("index.js"), "console.log('test');").unwrap();

    let output = env.hypha_in_dir(
        &[
            "hatch",
            "--domain",
            "test.local",
            "--id",
            "test-spore",
            "--name",
            "test-spore",
            "--intent",
            "Initial release",
        ],
        &spore_dir,
    );

    assert!(
        output.status.success(),
        "hatch failed: {}",
        combined_text(&output)
    );
    assert!(
        spore_dir.join("spore.core.json").exists(),
        "spore.core.json not created"
    );

    // Verify JSON output (default)
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("\"code\""),
        "should output JSON: {}",
        stdout
    );
}

#[test]
fn test_spore_release() {
    let env = TestEnv::new();

    // Init site with endpoints
    env.hypha(&[
        "mycelium",
        "root",
        "test.local",
        "--endpoints-base",
        "https://test.local",
    ]);

    // Create spore
    let spore_dir = env.dir.join("spore");
    fs::create_dir_all(&spore_dir).unwrap();
    fs::write(spore_dir.join("index.js"), "console.log('test');").unwrap();

    // Hatch with id
    env.hypha_in_dir(
        &[
            "hatch",
            "--domain",
            "test.local",
            "--id",
            "test-spore",
            "--name",
            "test-spore",
            "--intent",
            "Initial release",
        ],
        &spore_dir,
    );

    // Release (archive is default)
    let output = env.hypha_in_dir(&["release", "--domain", "test.local"], &spore_dir);
    assert!(
        output.status.success(),
        "release failed: {}",
        combined_text(&output)
    );

    // Check spore files
    let public_dir = env.site_dir("test.local").join("public");
    let spores: Vec<_> = fs::read_dir(public_dir.join("cmn/spore"))
        .unwrap()
        .filter_map(|e| e.ok())
        .collect();
    assert!(!spores.is_empty(), "no spore manifest created");

    let archives: Vec<_> = fs::read_dir(public_dir.join("cmn/archive"))
        .unwrap()
        .filter_map(|e| e.ok())
        .collect();
    assert!(!archives.is_empty(), "no tarball created");
}

/// Release a spore, extract its archive, and verify the content hash roundtrips.
/// This catches the bug where verify_content_hash only computed the tree
/// hash instead of the full URI hash (code + core + core_signature).
#[test]
fn test_release_hash_roundtrip() {
    let env = TestEnv::new();

    // Init site
    env.hypha(&[
        "mycelium",
        "root",
        "test.local",
        "--endpoints-base",
        "https://test.local",
    ]);

    // Create spore with multiple files including .gitignore (a dotfile).
    // The default tree config uses follow_rules: [".gitignore"], so this
    // tests that dotfiles survive the full release → archive → extract →
    // verify_content_hash roundtrip (including unpack_tar).
    let spore_dir = env.dir.join("spore-hash-test");
    fs::create_dir_all(&spore_dir).unwrap();
    fs::write(spore_dir.join("lib.rs"), "pub fn hello() {}").unwrap();
    fs::write(spore_dir.join("README.md"), "# Hash Test").unwrap();
    fs::write(spore_dir.join(".gitignore"), ".DS_Store\n.cmn\n").unwrap();
    fs::write(spore_dir.join("run.sh"), "#!/bin/sh\necho hash-test\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(spore_dir.join("run.sh"), fs::Permissions::from_mode(0o755)).unwrap();
    }

    env.hypha_in_dir(
        &[
            "hatch",
            "--domain",
            "test.local",
            "--id",
            "hash-test",
            "--name",
            "Hash Test",
            "--intent",
            "Test hash roundtrip",
        ],
        &spore_dir,
    );

    // Release
    let output = env.hypha_in_dir(&["release", "--domain", "test.local"], &spore_dir);
    assert!(
        output.status.success(),
        "release failed: {}",
        combined_text(&output)
    );

    // Parse release output to get hash
    let stdout = String::from_utf8_lossy(&output.stdout);
    let release_json: serde_json::Value = stdout
        .lines()
        .filter_map(|l| serde_json::from_str(l).ok())
        .find(|v: &serde_json::Value| v.get("code").and_then(|c| c.as_str()) == Some("ok"))
        .expect("no ok response from release");
    let hash = release_json["result"]["hash"]
        .as_str()
        .expect("no hash in release result");

    // Find the archive file
    let public_dir = env.site_dir("test.local").join("public");
    let archive_path = public_dir
        .join("cmn/archive")
        .join(format!("{}.tar.zst", hash));
    assert!(
        archive_path.exists(),
        "archive not found at {:?}",
        archive_path
    );

    // Extract using the code's own unpack_tar (via zstd decoder), NOT system tar.
    // This tests the actual extraction path that taste/tendril uses.
    let extract_dir = env.dir.join("extracted");
    fs::create_dir_all(&extract_dir).unwrap();
    {
        let file = fs::File::open(&archive_path).expect("open archive");
        let decoder = zstd::Decoder::new(file).expect("zstd decoder");
        let mut archive = tar::Archive::new(decoder);
        archive.unpack(&extract_dir).expect("unpack archive");
    }

    // Verify .gitignore survived extraction
    assert!(
        extract_dir.join(".gitignore").exists(),
        "dotfile .gitignore was lost during archive extraction"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = fs::metadata(extract_dir.join("run.sh"))
            .unwrap()
            .permissions()
            .mode();
        assert_ne!(mode & 0o111, 0, "run.sh executable bit was not preserved");
    }

    // Read the manifest
    let manifest_path = public_dir.join("cmn/spore").join(format!("{}.json", hash));
    let manifest_str = fs::read_to_string(&manifest_path).expect("manifest not found");
    let manifest: serde_json::Value =
        serde_json::from_str(&manifest_str).expect("invalid manifest JSON");

    // Verify hash roundtrip using verify_content_hash (the same function taste uses)
    hypha::verify_content_hash(&extract_dir, hash, &manifest)
        .expect("Hash roundtrip failed: release hash does not match recomputed hash from archive + manifest");
}

#[test]
fn test_custom_site_path() {
    let env = TestEnv::new();
    let custom_site = env.dir.join("custom-site");

    let output = Command::new(env!("CARGO_BIN_EXE_hypha"))
        .args([
            "mycelium",
            "root",
            "test.local",
            "--site-path",
            custom_site.to_str().unwrap(),
        ])
        .env("CMN_HOME", &env.dir)
        .output()
        .unwrap();

    assert!(output.status.success());
    assert!(
        custom_site.join("keys/private.pem").exists(),
        "custom site not created"
    );
}

#[test]
fn test_permission_check() {
    let env = TestEnv::new();

    // Init site with endpoints
    env.hypha(&[
        "mycelium",
        "root",
        "test.local",
        "--endpoints-base",
        "https://test.local",
    ]);

    // Create spore
    let spore_dir = env.dir.join("spore");
    fs::create_dir_all(&spore_dir).unwrap();
    fs::write(spore_dir.join("index.js"), "console.log('test');").unwrap();

    env.hypha_in_dir(
        &[
            "hatch",
            "--domain",
            "test.local",
            "--id",
            "test",
            "--name",
            "test",
            "--intent",
            "Initial release",
        ],
        &spore_dir,
    );

    // Change permissions to insecure
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let key_path = env.site_dir("test.local").join("keys/private.pem");
        let mut perms = fs::metadata(&key_path).unwrap().permissions();
        perms.set_mode(0o644);
        fs::set_permissions(&key_path, perms).unwrap();

        // Try to release - should fail
        let output = env.hypha_in_dir(&["release", "--domain", "test.local"], &spore_dir);

        // Check exit code (should be non-zero)
        assert!(
            !output.status.success(),
            "should fail with insecure permissions"
        );

        // Check JSON error output (errors go to stderr)
        let stderr = combined_text(&output);
        assert!(
            stderr.contains("\"error\":"),
            "should output JSON error: {}",
            stderr
        );
        assert!(
            stderr.contains("SIGN_ERR") || stderr.contains("insecure") || stderr.contains("0644"),
            "should report permission error: {}",
            stderr
        );
    }
}

#[test]
fn test_duplicate_root_updates() {
    let env = TestEnv::new();

    // First root should succeed
    let output = env.hypha(&["mycelium", "root", "test.local"]);
    assert!(output.status.success());

    // Second root should also succeed (updates the site)
    let output = env.hypha(&["mycelium", "root", "test.local"]);
    assert!(
        output.status.success(),
        "Second root should succeed and update the site"
    );
}

#[test]
fn test_spore_validation() {
    let env = TestEnv::new();

    // Init site with endpoints
    env.hypha(&[
        "mycelium",
        "root",
        "test.local",
        "--endpoints-base",
        "https://test.local",
    ]);

    // Create spore directory with manual (incomplete) spore.core.json
    let spore_dir = env.dir.join("spore");
    fs::create_dir_all(&spore_dir).unwrap();
    fs::write(spore_dir.join("index.js"), "console.log('test');").unwrap();

    // Write incomplete spore.core.json (missing intent)
    let incomplete_json = r#"{
        "$schema": "https://cmn.dev/schemas/v1/spore-core.json",
        "id": "test",
        "name": "test",
        "domain": "test.local",
        "synopsis": "test spore",
        "license": "MIT",
        "tree": {
            "algorithm": "blob_tree_blake3_nfc"
        }
    }"#;
    fs::write(spore_dir.join("spore.core.json"), incomplete_json).unwrap();

    // Try to release - should fail validation
    let output = env.hypha_in_dir(&["release", "--domain", "test.local"], &spore_dir);
    assert!(!output.status.success(), "should fail with missing intent");

    let stderr = combined_text(&output);
    assert!(
        stderr.contains("schema_error"),
        "should return schema_error: {}",
        stderr
    );
    assert!(
        stderr.contains("intent"),
        "should mention intent field: {}",
        stderr
    );
}

#[test]
fn test_manual_spore_core_json() {
    let env = TestEnv::new();

    // Init site with endpoints
    env.hypha(&[
        "mycelium",
        "root",
        "test.local",
        "--endpoints-base",
        "https://test.local",
    ]);

    // Create spore directory with manually written spore.core.json
    let spore_dir = env.dir.join("spore");
    fs::create_dir_all(&spore_dir).unwrap();
    fs::write(spore_dir.join("index.js"), "console.log('test');").unwrap();

    let status_output = env.hypha(&["mycelium", "status", "test.local"]);
    assert!(
        status_output.status.success(),
        "mycelium status failed: {}",
        combined_text(&status_output)
    );
    let status_stdout = String::from_utf8_lossy(&status_output.stdout);
    let status_json = parse_json_last_line(&status_stdout);
    let public_key = status_json["result"]["public_key"]
        .as_str()
        .expect("public_key in mycelium status");

    // Write complete spore.core.json manually (without using hatch)
    let manual_json = serde_json::json!({
        "$schema": "https://cmn.dev/schemas/v1/spore-core.json",
        "id": "manual-spore",
        "name": "manual-spore",
        "domain": "test.local",
        "key": public_key,
        "synopsis": "Manually created spore",
        "intent": ["Manual creation test for CMN spore"],
        "license": "MIT",
        "mutations": [],
        "bonds": [],
        "tree": {
            "algorithm": "blob_tree_blake3_nfc",
            "exclude_names": [".git"],
            "follow_rules": [".gitignore"]
        }
    });
    fs::write(
        spore_dir.join("spore.core.json"),
        serde_json::to_string_pretty(&manual_json).unwrap(),
    )
    .unwrap();

    // Release should work with manually created spore.core.json
    let output = env.hypha_in_dir(&["release", "--domain", "test.local"], &spore_dir);
    assert!(
        output.status.success(),
        "release should work with manual spore.core.json: {}",
        combined_text(&output)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("\"code\":\"ok\""),
        "should succeed: {}",
        stdout
    );
    assert!(
        stdout.contains("manual-spore"),
        "should contain spore name: {}",
        stdout
    );
}

#[test]
fn test_release_default_archive() {
    let env = TestEnv::new();

    // Init site with endpoints
    env.hypha(&[
        "mycelium",
        "root",
        "test.local",
        "--endpoints-base",
        "https://test.local",
    ]);

    // Create spore
    let spore_dir = env.dir.join("spore");
    fs::create_dir_all(&spore_dir).unwrap();
    fs::write(spore_dir.join("index.js"), "console.log('test');").unwrap();

    env.hypha_in_dir(
        &[
            "hatch",
            "--domain",
            "test.local",
            "--id",
            "test",
            "--name",
            "test",
            "--intent",
            "Initial release",
        ],
        &spore_dir,
    );

    // Release without explicit --archive flag — should succeed with default zstd
    let output = env.hypha_in_dir(&["release", "--domain", "test.local"], &spore_dir);

    assert!(
        output.status.success(),
        "release should succeed with default archive: {}",
        combined_text(&output)
    );

    // Check for .tar.zst file (default format)
    let archive_dir = env.site_dir("test.local").join("public/cmn/archive");
    let zst_files: Vec<_> = fs::read_dir(&archive_dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().is_some_and(|ext| ext == "zst"))
        .collect();
    assert!(!zst_files.is_empty(), "default should create .tar.zst file");
}

#[test]
fn test_archive_format_zstd() {
    let env = TestEnv::new();

    // Init site with endpoints
    env.hypha(&[
        "mycelium",
        "root",
        "test.local",
        "--endpoints-base",
        "https://test.local",
    ]);

    // Create spore
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

    // Release with --archive zstd (default)
    let output = env.hypha_in_dir(
        &["release", "--domain", "test.local", "--archive", "zstd"],
        &spore_dir,
    );
    assert!(
        output.status.success(),
        "release with zstd failed: {}",
        combined_text(&output)
    );

    // Check for .tar.zst file
    let archive_dir = env.site_dir("test.local").join("public/cmn/archive");
    let zst_files: Vec<_> = fs::read_dir(&archive_dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().is_some_and(|ext| ext == "zst"))
        .collect();
    assert!(
        !zst_files.is_empty(),
        "no .tar.zst file created in {:?}",
        archive_dir
    );
}

#[test]
fn test_archive_format_gzip_rejected() {
    let env = TestEnv::new();

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

    // Release with --archive gzip (no longer supported for generation)
    let output = env.hypha_in_dir(
        &["release", "--domain", "test.local", "--archive", "gzip"],
        &spore_dir,
    );
    assert!(
        !output.status.success(),
        "gzip generation should be rejected"
    );
    let stderr = combined_text(&output);
    assert!(
        stderr.contains("INVALID_ARGS")
            || stderr.contains("Unsupported archive format")
            || stderr.contains("Use: zstd"),
        "should report unsupported gzip generation: {}",
        stderr
    );
}

#[test]
fn test_archive_format_xz_rejected() {
    let env = TestEnv::new();

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

    // Release with --archive xz (no longer supported for generation)
    let output = env.hypha_in_dir(
        &["release", "--domain", "test.local", "--archive", "xz"],
        &spore_dir,
    );
    assert!(!output.status.success(), "xz generation should be rejected");
    let stderr = combined_text(&output);
    assert!(
        stderr.contains("INVALID_ARGS")
            || stderr.contains("Unsupported archive format")
            || stderr.contains("Use: zstd"),
        "should report unsupported xz generation: {}",
        stderr
    );
}

#[test]
fn test_archive_format_zip_rejected() {
    let env = TestEnv::new();

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

    // Release with --archive zip (no longer supported for generation)
    let output = env.hypha_in_dir(
        &["release", "--domain", "test.local", "--archive", "zip"],
        &spore_dir,
    );
    assert!(
        !output.status.success(),
        "zip generation should be rejected"
    );
    let stderr = combined_text(&output);
    assert!(
        stderr.contains("INVALID_ARGS")
            || stderr.contains("Unsupported archive format")
            || stderr.contains("Use: zstd"),
        "should report unsupported zip generation: {}",
        stderr
    );
}

#[test]
fn test_archive_default_format() {
    let env = TestEnv::new();

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

    // Release with --archive (no format specified, should default to zstd)
    let output = env.hypha_in_dir(&["release", "--domain", "test.local"], &spore_dir);
    assert!(
        output.status.success(),
        "release with default archive failed: {}",
        combined_text(&output)
    );

    // Check for .tar.zst file (default)
    let archive_dir = env.site_dir("test.local").join("public/cmn/archive");
    let zst_files: Vec<_> = fs::read_dir(&archive_dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().is_some_and(|ext| ext == "zst"))
        .collect();
    assert!(!zst_files.is_empty(), "default should create .tar.zst file");
}

#[test]
fn test_archive_invalid_format() {
    let env = TestEnv::new();

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

    // Release with invalid archive format
    let output = env.hypha_in_dir(
        &["release", "--domain", "test.local", "--archive", "invalid"],
        &spore_dir,
    );

    assert!(
        !output.status.success(),
        "should fail with invalid archive format"
    );

    let stderr = combined_text(&output);
    assert!(
        stderr.contains("INVALID_ARGS")
            || stderr.contains("Unknown archive format")
            || stderr.contains("invalid"),
        "should report invalid format: {}",
        stderr
    );
}

#[test]
fn test_dist_git_requires_commit() {
    let env = TestEnv::new();

    // Init site with endpoints
    env.hypha(&[
        "mycelium",
        "root",
        "test.local",
        "--endpoints-base",
        "https://test.local",
    ]);

    // Create spore
    let spore_dir = env.dir.join("spore");
    fs::create_dir_all(&spore_dir).unwrap();
    fs::write(spore_dir.join("index.js"), "console.log('test');").unwrap();

    env.hypha_in_dir(
        &[
            "hatch",
            "--domain",
            "test.local",
            "--id",
            "test",
            "--name",
            "test",
            "--intent",
            "Initial release",
        ],
        &spore_dir,
    );

    // Release with --dist-git but no --dist-ref - should fail
    let output = env.hypha_in_dir(
        &[
            "release",
            "--domain",
            "test.local",
            "--dist-git",
            "https://github.com/test/test",
        ],
        &spore_dir,
    );

    assert!(!output.status.success(), "should fail without dist-ref");

    let stderr = combined_text(&output);
    assert!(
        stderr.contains("invalid_args"),
        "should return invalid_args: {}",
        stderr
    );
    assert!(
        stderr.contains("dist-ref"),
        "should mention dist-ref: {}",
        stderr
    );
}

#[test]
fn test_endpoints_base_parameter() {
    let env = TestEnv::new();

    // Init with --endpoints-base
    env.hypha(&[
        "mycelium",
        "root",
        "test.local",
        "--endpoints-base",
        "http://127.0.0.1:8080",
    ]);

    // Read generated cmn.json and check it has archive endpoint
    let cmn_path = env
        .site_dir("test.local")
        .join("public/.well-known/cmn.json");
    let cmn_content = fs::read_to_string(&cmn_path).unwrap();
    assert!(
        cmn_content.contains("http://127.0.0.1:8080"),
        "cmn.json should contain custom base URL: {}",
        cmn_content
    );
    assert!(
        cmn_content.contains("mycelium"),
        "cmn.json should contain mycelium endpoint: {}",
        cmn_content
    );
}

#[test]
fn test_mycelium_serve_help() {
    let env = TestEnv::new();

    // Test that serve command exists and shows in help
    let output = env.hypha(&["mycelium", "--help"]);
    assert!(output.status.success());

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("serve"),
        "help should mention serve command: {}",
        stdout
    );
}

#[test]
fn test_release_rejects_symlink() {
    use std::os::unix::fs::symlink;

    let env = TestEnv::new();

    // Init site
    env.hypha(&[
        "mycelium",
        "root",
        "test.local",
        "--endpoints-base",
        "https://test.local",
    ]);

    // Create spore with a symlink
    let spore_dir = env.dir.join("symlink-spore");
    fs::create_dir_all(&spore_dir).unwrap();
    fs::write(spore_dir.join("real.txt"), "content").unwrap();
    symlink("real.txt", spore_dir.join("link.txt")).unwrap();

    env.hypha_in_dir(
        &[
            "hatch",
            "--domain",
            "test.local",
            "--name",
            "symlink-test",
            "--intent",
            "test symlink rejection",
        ],
        &spore_dir,
    );

    let output = env.hypha_in_dir(&["release", "--domain", "test.local"], &spore_dir);
    assert!(
        !output.status.success(),
        "release should fail when symlink is present"
    );
    let text = combined_text(&output);
    assert!(
        text.contains("symlink found") || text.contains("SYMLINK_ERR"),
        "error should mention symlink: {}",
        text
    );
}

#[cfg(unix)]
#[test]
fn test_release_succeeds_with_excluded_symlink() {
    use std::os::unix::fs::symlink;

    let env = TestEnv::new();

    env.hypha(&[
        "mycelium",
        "root",
        "test.local",
        "--endpoints-base",
        "https://test.local",
    ]);

    let spore_dir = env.dir.join("excluded-symlink-spore");
    fs::create_dir_all(&spore_dir).unwrap();
    fs::write(spore_dir.join("real.txt"), "content").unwrap();

    // Put symlink inside a directory that will be excluded
    let ignored_dir = spore_dir.join("node_modules");
    fs::create_dir_all(&ignored_dir).unwrap();
    symlink("../real.txt", ignored_dir.join("link.txt")).unwrap();

    env.hypha_in_dir(
        &[
            "hatch",
            "--domain",
            "test.local",
            "--id",
            "excluded-symlink-test",
            "--name",
            "excluded-symlink-test",
            "--intent",
            "test excluded symlink",
        ],
        &spore_dir,
    );

    // Add node_modules to exclude_names
    let core_path = spore_dir.join("spore.core.json");
    let content = fs::read_to_string(&core_path).unwrap();
    let mut core: serde_json::Value = serde_json::from_str(&content).unwrap();
    core["tree"]["exclude_names"] = serde_json::json!([".git", ".cmn", "node_modules"]);
    fs::write(&core_path, serde_json::to_string_pretty(&core).unwrap()).unwrap();

    let output = env.hypha_in_dir(&["release", "--domain", "test.local"], &spore_dir);
    let text = combined_text(&output);
    assert!(
        output.status.success() || text.contains("ok"),
        "release should succeed when symlink is in excluded directory: {}",
        text
    );
}
