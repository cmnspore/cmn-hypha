#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::fs;
use std::path::PathBuf;
use std::process::Command;
use tempfile::TempDir;

struct TestEnv {
    _temp: TempDir, // Kept to prevent early cleanup
    dir: PathBuf,
}

impl TestEnv {
    fn new() -> Self {
        let temp = TempDir::new().expect("Failed to create temp dir");
        let dir = temp.path().to_path_buf();
        Self { _temp: temp, dir }
    }

    fn hypha(&self, args: &[&str]) -> std::process::Output {
        Command::new(env!("CARGO_BIN_EXE_hypha"))
            .args(args)
            .env("CMN_HOME", &self.dir)
            .output()
            .expect("Failed to run hypha")
    }

    fn hypha_in_dir(&self, args: &[&str], cwd: &PathBuf) -> std::process::Output {
        Command::new(env!("CARGO_BIN_EXE_hypha"))
            .args(args)
            .current_dir(cwd)
            .env("CMN_HOME", &self.dir)
            .output()
            .expect("Failed to run hypha")
    }

    fn site_dir(&self, domain: &str) -> PathBuf {
        self.dir.join("mycelium").join(domain)
    }

    #[allow(dead_code)]
    fn hypha_with_env(&self, args: &[&str], env_vars: &[(&str, &str)]) -> std::process::Output {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_hypha"));
        cmd.args(args).env("CMN_HOME", &self.dir);
        for (key, value) in env_vars {
            cmd.env(key, value);
        }
        cmd.output().expect("Failed to run hypha")
    }
}

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

fn combined_text(output: &std::process::Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

fn parse_json_last_line(text: &str) -> serde_json::Value {
    let parsed = text
        .lines()
        .rev()
        .find_map(|line| serde_json::from_str::<serde_json::Value>(line).ok());
    assert!(parsed.is_some(), "failed to parse JSONL output: {}", text);
    parsed.expect("checked by assertion above")
}

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
        stdout.contains("\"code\":\"ok\""),
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
        stdout.contains("\"code\":\"ok\""),
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
        stdout.contains("\"count\":1"),
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
        stdout.contains("\"removed\":1"),
        "should show 1 removed: {}",
        stdout
    );

    // Verify cache is empty
    let output = env.hypha(&["cache", "list"]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("\"count\":0"),
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

#[test]
fn test_delta_archive_generation() {
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
    fs::write(spore_dir.join("index.js"), "console.log('v1');").unwrap();
    fs::write(
        spore_dir.join("readme.txt"),
        "version 1 readme with some content to make it larger",
    )
    .unwrap();

    // Hatch
    env.hypha_in_dir(
        &[
            "hatch",
            "--domain",
            "test.local",
            "--id",
            "delta-test",
            "--name",
            "delta-test",
            "--intent",
            "Test delta archives",
        ],
        &spore_dir,
    );

    // First release
    let output1 = env.hypha_in_dir(
        &["release", "--domain", "test.local", "--archive", "zstd"],
        &spore_dir,
    );
    assert!(
        output1.status.success(),
        "first release failed: {}",
        combined_text(&output1)
    );

    let stdout1 = String::from_utf8_lossy(&output1.stdout);
    let result1: serde_json::Value = parse_json_last_line(&stdout1);
    let hash1 = result1["result"]["hash"].as_str().unwrap().to_string();

    // Modify spore for second release
    fs::write(spore_dir.join("index.js"), "console.log('v2');").unwrap();

    // Need to re-add mutations field for second release
    env.hypha_in_dir(
        &[
            "hatch",
            "--domain",
            "test.local",
            "--mutations",
            "Updated to v2",
        ],
        &spore_dir,
    );

    // Second release
    let output2 = env.hypha_in_dir(
        &["release", "--domain", "test.local", "--archive", "zstd"],
        &spore_dir,
    );
    assert!(
        output2.status.success(),
        "second release failed: {}",
        combined_text(&output2)
    );

    let stdout2 = String::from_utf8_lossy(&output2.stdout);
    let result2: serde_json::Value = parse_json_last_line(&stdout2);
    let hash2 = result2["result"]["hash"].as_str().unwrap().to_string();

    // Hashes should differ
    assert_ne!(hash1, hash2, "hashes should differ after content change");

    // Check delta file exists in archive dir
    let archive_dir = env.site_dir("test.local").join("public/cmn/archive");
    let delta_filename = format!("{}.from.{}.tar.zst", hash2, hash1);
    let delta_path = archive_dir.join(&delta_filename);
    assert!(
        delta_path.exists(),
        "delta archive not found: {}",
        delta_path.display()
    );

    // Delta should be smaller than the full archive
    let full_path = archive_dir.join(format!("{}.tar.zst", hash2));
    let delta_size = fs::metadata(&delta_path).unwrap().len();
    let full_size = fs::metadata(&full_path).unwrap().len();
    assert!(
        delta_size <= full_size,
        "delta ({} bytes) should be <= full archive ({} bytes)",
        delta_size,
        full_size
    );

    // Dist stays full-source only; delta discovery is endpoint-driven.
    let spore_manifest_path = env
        .site_dir("test.local")
        .join("public/cmn/spore")
        .join(format!("{}.json", hash2));
    let manifest_content = fs::read_to_string(&spore_manifest_path).unwrap();
    let manifest: serde_json::Value = serde_json::from_str(&manifest_content).unwrap();
    let dist = manifest["capsule"]["dist"].as_array().unwrap();

    let has_delta = dist.iter().any(|d| {
        d.get("type").and_then(|v| v.as_str()) == Some("archive_delta")
            || d.get("delta").is_some()
            || d.get("from").is_some()
    });
    assert!(
        !has_delta,
        "dist should not contain delta entries: {:?}",
        dist
    );

    let has_archive = dist
        .iter()
        .any(|d| d.get("type").and_then(|v| v.as_str()) == Some("archive"));
    assert!(
        has_archive,
        "dist array should still contain full archive entry"
    );
    let archive_entry = dist
        .iter()
        .find(|d| d.get("type").and_then(|v| v.as_str()) == Some("archive"))
        .unwrap();
    // filename is no longer required in dist archive entries (resolved via endpoints + hash)
    assert!(
        archive_entry.get("filename").is_none()
            || archive_entry["filename"].as_str() == Some(&format!("{}.tar.zst", hash2)),
        "filename should be absent or match hash"
    );
}

#[test]
fn test_delta_archive_roundtrip() {
    // Test that delta compression + decompression produces identical content
    use std::io::{Read, Write};

    // Create some tar content
    let mut raw_tar_v1 = Vec::new();
    {
        let mut tar = tar::Builder::new(&mut raw_tar_v1);
        let content = b"hello world v1";
        let mut header = tar::Header::new_gnu();
        header.set_size(content.len() as u64);
        header.set_mode(0o644);
        header.set_mtime(0);
        header.set_uid(0);
        header.set_gid(0);
        header.set_cksum();
        tar.append_data(&mut header, "file.txt", &content[..])
            .unwrap();
        tar.finish().unwrap();
    }

    // Create slightly modified tar content
    let mut raw_tar_v2 = Vec::new();
    {
        let mut tar = tar::Builder::new(&mut raw_tar_v2);
        let content = b"hello world v2";
        let mut header = tar::Header::new_gnu();
        header.set_size(content.len() as u64);
        header.set_mode(0o644);
        header.set_mtime(0);
        header.set_uid(0);
        header.set_gid(0);
        header.set_cksum();
        tar.append_data(&mut header, "file.txt", &content[..])
            .unwrap();
        tar.finish().unwrap();
    }

    // Compress v2 using v1 as dictionary
    let dict = zstd::dict::EncoderDictionary::copy(&raw_tar_v1, 19);
    let mut delta_bytes = Vec::new();
    {
        let mut encoder = zstd::Encoder::with_prepared_dictionary(&mut delta_bytes, &dict).unwrap();
        encoder.long_distance_matching(true).unwrap();
        encoder.write_all(&raw_tar_v2).unwrap();
        encoder.finish().unwrap();
    }

    // Decompress delta using v1 as dictionary
    let mut decoder =
        zstd::Decoder::with_dictionary(std::io::Cursor::new(&delta_bytes), &raw_tar_v1).unwrap();
    let mut decompressed = Vec::new();
    decoder.read_to_end(&mut decompressed).unwrap();

    // Verify roundtrip
    assert_eq!(
        decompressed, raw_tar_v2,
        "decompressed delta should equal original raw tar"
    );

    // Delta should be smaller than raw tar (for similar content)
    assert!(
        delta_bytes.len() < raw_tar_v2.len(),
        "delta ({} bytes) should be smaller than raw tar ({} bytes)",
        delta_bytes.len(),
        raw_tar_v2.len()
    );
}

#[test]
fn test_no_delta_on_first_release() {
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
    fs::write(spore_dir.join("index.js"), "console.log('first');").unwrap();

    env.hypha_in_dir(
        &[
            "hatch",
            "--domain",
            "test.local",
            "--id",
            "nodelta-test",
            "--name",
            "nodelta-test",
            "--intent",
            "Test no delta on first release",
        ],
        &spore_dir,
    );

    let output = env.hypha_in_dir(
        &["release", "--domain", "test.local", "--archive", "zstd"],
        &spore_dir,
    );
    assert!(output.status.success());

    let stdout = String::from_utf8_lossy(&output.stdout);
    let result: serde_json::Value = parse_json_last_line(&stdout);
    let hash = result["result"]["hash"].as_str().unwrap();

    // Check archive dir: should have only the full archive, no delta
    let archive_dir = env.site_dir("test.local").join("public/cmn/archive");
    let files: Vec<_> = fs::read_dir(&archive_dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .collect();

    assert_eq!(
        files.len(),
        1,
        "should have exactly 1 archive file (no delta)"
    );
    let filename = files[0].file_name().to_string_lossy().to_string();
    assert!(
        filename.starts_with(hash) && filename.ends_with(".tar.zst"),
        "should be the full archive: {}",
        filename
    );
    assert!(
        !filename.contains(".from."),
        "first release should not have a delta: {}",
        filename
    );
}

#[test]
fn test_bond_status_no_refs() {
    let env = TestEnv::new();

    // Create a spore dir with no depends_on bonds
    let spore_dir = env.dir.join("spore");
    fs::create_dir_all(&spore_dir).unwrap();
    fs::write(
        spore_dir.join("spore.core.json"),
        r#"{"id":"test","name":"Test","domain":"test.local","synopsis":"Test spore","intent":["test"],"license":"MIT","mutations":[],"bonds":[],"tree":{"algorithm":"blob_tree_blake3_nfc","exclude_names":[],"follow_rules":[]}}"#,
    )
    .unwrap();

    let output = env.hypha_in_dir(&["bond", "--status"], &spore_dir);
    assert!(
        output.status.success(),
        "bond-fetch --status failed: {}",
        combined_text(&output)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    let result: serde_json::Value = parse_json_last_line(&stdout);
    assert_eq!(
        result["result"]["bonds"].as_array().unwrap().len(),
        0,
        "should have no bonds"
    );
}

#[test]
fn test_bond_status_with_refs() {
    let env = TestEnv::new();

    // Create a spore dir with a depends_on bond
    let spore_dir = env.dir.join("spore");
    fs::create_dir_all(&spore_dir).unwrap();
    fs::write(
        spore_dir.join("spore.core.json"),
        r#"{"id":"test","name":"Test","domain":"test.local","synopsis":"Test spore","intent":["test"],"license":"MIT","mutations":[],"bonds":[{"uri":"cmn://example.com/b3.11111111111111111111111111111111111111111111","relation":"depends_on","reason":"Core lib"}],"tree":{"algorithm":"blob_tree_blake3_nfc","exclude_names":[],"follow_rules":[]}}"#,
    )
    .unwrap();

    let output = env.hypha_in_dir(&["bond", "--status"], &spore_dir);
    assert!(output.status.success());

    let stdout = String::from_utf8_lossy(&output.stdout);
    let result: serde_json::Value = parse_json_last_line(&stdout);
    let refs = result["result"]["bonds"].as_array().unwrap();
    assert_eq!(refs.len(), 1);
    assert_eq!(refs[0]["bonded"], false);
    assert!(refs[0]["uri"].as_str().unwrap().contains("example.com"));
}

#[test]
fn test_bond_clean_orphans() {
    let env = TestEnv::new();

    // Create spore dir with no depends_on bonds
    let spore_dir = env.dir.join("spore");
    fs::create_dir_all(&spore_dir).unwrap();
    fs::write(
        spore_dir.join("spore.core.json"),
        r#"{"id":"test","name":"Test","domain":"test.local","synopsis":"Test spore","intent":["test"],"license":"MIT","mutations":[],"bonds":[],"tree":{"algorithm":"blob_tree_blake3_nfc","exclude_names":[],"follow_rules":[]}}"#,
    )
    .unwrap();

    // Create an orphaned bond directory
    let orphan_dir = spore_dir.join(".cmn/bonds/orphan-lib");
    fs::create_dir_all(&orphan_dir).unwrap();
    fs::write(
        orphan_dir.join("spore.json"),
        r#"{"capsule":{"uri":"cmn://old.dev/b3.dead"}}"#,
    )
    .unwrap();

    assert!(
        orphan_dir.exists(),
        "orphan bond dir should exist before clean"
    );

    let output = env.hypha_in_dir(&["bond", "--clean"], &spore_dir);
    assert!(
        output.status.success(),
        "bond-fetch --clean failed: {}",
        combined_text(&output)
    );

    assert!(
        !orphan_dir.exists(),
        "orphan bond dir should be removed after clean"
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    let result: serde_json::Value = parse_json_last_line(&stdout);
    let cleaned = result["result"]["cleaned"].as_array().unwrap();
    assert_eq!(cleaned.len(), 1);
    assert_eq!(cleaned[0], "orphan-lib");
}

#[test]
fn test_bond_no_spore_core() {
    let env = TestEnv::new();

    // Try to bond-fetch in a directory without spore.core.json
    let empty_dir = env.dir.join("empty");
    fs::create_dir_all(&empty_dir).unwrap();

    let output = env.hypha_in_dir(&["bond"], &empty_dir);
    assert!(!output.status.success());

    let stderr = combined_text(&output);
    assert!(
        stderr.contains("bond_error"),
        "should return bond_error: {}",
        stderr
    );
}

#[test]
fn test_bond_only_spawned_from() {
    let env = TestEnv::new();

    // Create spore with only spawned_from — should report no bondable bonds
    let spore_dir = env.dir.join("spore");
    fs::create_dir_all(&spore_dir).unwrap();
    fs::write(
        spore_dir.join("spore.core.json"),
        r#"{"id":"test","name":"Test","domain":"test.local","synopsis":"Test spore","intent":["test"],"license":"MIT","mutations":[],"bonds":[
            {"uri":"cmn://cmn.dev/b3.11111111111111111111111111111111111111111111","relation":"spawned_from"}
        ],"tree":{"algorithm":"blob_tree_blake3_nfc","exclude_names":[],"follow_rules":[]}}"#,
    )
    .unwrap();

    let output = env.hypha_in_dir(&["bond"], &spore_dir);
    assert!(output.status.success());

    let stdout = String::from_utf8_lossy(&output.stdout);
    let result: serde_json::Value = parse_json_last_line(&stdout);
    assert!(
        result["result"]["message"]
            .as_str()
            .unwrap()
            .contains("No spore bonds"),
        "should report no bondable bonds: {}",
        stdout
    );
}

#[test]
fn test_bond_excludes_spawned_and_absorbed() {
    let env = TestEnv::new();

    // Create spore with spawned_from, absorbed_from, and follows bonds
    let spore_dir = env.dir.join("spore");
    fs::create_dir_all(&spore_dir).unwrap();
    fs::write(
        spore_dir.join("spore.core.json"),
        r#"{"id":"test","name":"Test","domain":"test.local","synopsis":"Test","intent":["test"],"license":"MIT","mutations":[],"bonds":[
            {"uri":"cmn://a.com/b3.11111111111111111111111111111111111111111111","relation":"spawned_from"},
            {"uri":"cmn://b.com/b3.22222222222222222222222222222222222222222222","relation":"absorbed_from"},
            {"uri":"cmn://c.com/b3.33333333333333333333333333333333333333333333","relation":"follows"}
        ],"tree":{"algorithm":"blob_tree_blake3_nfc","exclude_names":[],"follow_rules":[]}}"#,
    )
    .unwrap();

    // bond-fetch --status should show follows as bondable, spawned_from/absorbed_from as excluded
    let output = env.hypha_in_dir(&["bond", "--status"], &spore_dir);
    assert!(output.status.success());

    let stdout = String::from_utf8_lossy(&output.stdout);
    let result: serde_json::Value = parse_json_last_line(&stdout);
    let refs = result["result"]["bonds"].as_array().unwrap();
    assert_eq!(refs.len(), 3);

    // follows should show bonded: false (not yet fetched)
    let follows = refs.iter().find(|r| r["relation"] == "follows").unwrap();
    assert_eq!(follows["bonded"], false);

    // spawned_from and absorbed_from should show bonded: "excluded"
    let spawned = refs
        .iter()
        .find(|r| r["relation"] == "spawned_from")
        .unwrap();
    assert_eq!(spawned["bonded"], "excluded");

    let absorbed = refs
        .iter()
        .find(|r| r["relation"] == "absorbed_from")
        .unwrap();
    assert_eq!(absorbed["bonded"], "excluded");
}

#[test]
fn test_replicate_basic() {
    let env = TestEnv::new();

    // Init source site
    env.hypha(&[
        "mycelium",
        "root",
        "source.local",
        "--endpoints-base",
        "https://source.local",
    ]);

    // Init target site
    env.hypha(&[
        "mycelium",
        "root",
        "target.local",
        "--endpoints-base",
        "https://target.local",
    ]);

    // Create and release a spore on source
    let spore_dir = env.dir.join("spore");
    fs::create_dir_all(&spore_dir).unwrap();
    fs::write(spore_dir.join("index.js"), "console.log('hello');").unwrap();

    env.hypha_in_dir(
        &[
            "hatch",
            "--id",
            "rep-test",
            "--name",
            "Replicate Test",
            "--intent",
            "Test replication",
            "--domain",
            "source.local",
        ],
        &spore_dir,
    );

    let release_output = env.hypha_in_dir(&["release", "--domain", "source.local"], &spore_dir);
    assert!(release_output.status.success());

    let release_stdout = String::from_utf8_lossy(&release_output.stdout);
    let release_result: serde_json::Value = parse_json_last_line(&release_stdout);
    let _hash = release_result["result"]["hash"].as_str().unwrap();
    let _uri = release_result["result"]["uri"].as_str().unwrap();

    // Test the --refs mode with no refs to replicate
    let spore2_dir = env.dir.join("spore2");
    fs::create_dir_all(&spore2_dir).unwrap();
    fs::write(
        spore2_dir.join("spore.core.json"),
        r#"{"id":"test2","name":"Test2","domain":"source.local","synopsis":"Test","intent":["test"],"license":"MIT","mutations":[],"bonds":[],"tree":{"algorithm":"blob_tree_blake3_nfc","exclude_names":[],"follow_rules":[]}}"#,
    )
    .unwrap();

    let output = env.hypha_in_dir(
        &["replicate", "--refs", "--domain", "target.local"],
        &spore2_dir,
    );
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let result: serde_json::Value = parse_json_last_line(&stdout);
    assert!(
        result["result"]["message"]
            .as_str()
            .unwrap()
            .contains("No non-self"),
        "should report no refs to replicate: {}",
        stdout
    );
}

#[test]
fn test_replicate_already_exists() {
    let env = TestEnv::new();

    // Init source and target sites
    env.hypha(&[
        "mycelium",
        "root",
        "source.local",
        "--endpoints-base",
        "https://source.local",
    ]);
    env.hypha(&[
        "mycelium",
        "root",
        "target.local",
        "--endpoints-base",
        "https://target.local",
    ]);

    // Create and release a spore on source
    let spore_dir = env.dir.join("spore");
    fs::create_dir_all(&spore_dir).unwrap();
    fs::write(spore_dir.join("index.js"), "console.log('exists');").unwrap();

    env.hypha_in_dir(
        &[
            "hatch",
            "--id",
            "dup-test",
            "--name",
            "Dup Test",
            "--intent",
            "Test duplicate check",
            "--domain",
            "source.local",
        ],
        &spore_dir,
    );

    let release_output = env.hypha_in_dir(&["release", "--domain", "source.local"], &spore_dir);
    assert!(release_output.status.success());

    let release_stdout = String::from_utf8_lossy(&release_output.stdout);
    let release_result: serde_json::Value = parse_json_last_line(&release_stdout);
    let hash = release_result["result"]["hash"].as_str().unwrap();

    // Copy the spore manifest to target site (simulating it was already replicated)
    let source_manifest = env
        .site_dir("source.local")
        .join(format!("public/cmn/spore/{}.json", hash));
    let target_spore_dir = env.site_dir("target.local").join("public/cmn/spore");
    fs::create_dir_all(&target_spore_dir).unwrap();
    fs::copy(
        &source_manifest,
        target_spore_dir.join(format!("{}.json", hash)),
    )
    .unwrap();

    // Seed taste verdict for replicate
    let taste_dir = env
        .dir
        .join(format!("hypha/cache/source.local/spore/{}", hash));
    fs::create_dir_all(&taste_dir).unwrap();
    fs::write(
        taste_dir.join("taste.json"),
        r#"{"verdict":"safe","tasted_at_epoch_ms":1700000000000}"#,
    )
    .unwrap();

    // Try to replicate — should skip with already_exists
    let output = env.hypha_in_dir(
        &[
            "replicate",
            &format!("cmn://source.local/{}", hash),
            "--domain",
            "target.local",
        ],
        &spore_dir,
    );
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let result: serde_json::Value = parse_json_last_line(&stdout);
    assert_eq!(
        result["result"]["replicated"][0]["status"], "already_exists",
        "should report already_exists: {}",
        stdout
    );
}

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

#[test]
fn test_taste_sweet_verdict() {
    let env = TestEnv::new();

    // Seed a cached spore so taste record can find it
    let hash = "b3.111111111111111111111111111111111111111111";
    let spore_dir = env
        .dir
        .join(format!("hypha/cache/example.com/spore/{}", hash));
    fs::create_dir_all(&spore_dir).unwrap();
    fs::write(spore_dir.join("spore.json"), "{}").unwrap();

    let output = env.hypha(&[
        "taste",
        &format!("cmn://example.com/{}", hash),
        "--verdict",
        "sweet",
        "--notes",
        "Excellent quality, thoroughly reviewed",
    ]);
    assert!(
        output.status.success(),
        "sweet verdict should succeed: {}",
        combined_text(&output)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    let result: serde_json::Value = parse_json_last_line(&stdout);
    assert_eq!(result["result"]["verdict"], "sweet");
}

#[test]
fn test_taste_safe_verdict() {
    let env = TestEnv::new();

    let hash = "b3.111111111111111111111111111111111111111111";
    let spore_dir = env
        .dir
        .join(format!("hypha/cache/example.com/spore/{}", hash));
    fs::create_dir_all(&spore_dir).unwrap();
    fs::write(spore_dir.join("spore.json"), "{}").unwrap();

    let output = env.hypha(&[
        "taste",
        &format!("cmn://example.com/{}", hash),
        "--verdict",
        "safe",
        "--notes",
        "Quick scan, nothing suspicious",
    ]);
    assert!(
        output.status.success(),
        "safe verdict should succeed: {}",
        combined_text(&output)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    let result: serde_json::Value = parse_json_last_line(&stdout);
    assert_eq!(result["result"]["verdict"], "safe");
}

#[test]
fn test_taste_all_verdicts_accepted() {
    // Verify all 5 verdicts are accepted, and invalid ones are rejected
    let env = TestEnv::new();

    let hash = "b3.111111111111111111111111111111111111111111";
    let spore_dir = env
        .dir
        .join(format!("hypha/cache/example.com/spore/{}", hash));
    fs::create_dir_all(&spore_dir).unwrap();
    fs::write(spore_dir.join("spore.json"), "{}").unwrap();

    for verdict in &["sweet", "fresh", "safe", "rotten", "toxic"] {
        let output = env.hypha(&[
            "taste",
            &format!("cmn://example.com/{}", hash),
            "--verdict",
            verdict,
        ]);
        assert!(
            output.status.success(),
            "'{}' verdict should be accepted: {}",
            verdict,
            combined_text(&output)
        );
    }

    // Invalid verdict should fail
    let output = env.hypha(&[
        "taste",
        &format!("cmn://example.com/{}", hash),
        "--verdict",
        "delicious",
    ]);
    assert!(!output.status.success(), "'delicious' should be rejected");
    let stderr = combined_text(&output);
    assert!(stderr.contains("invalid value 'delicious' for '--verdict <VERDICT>'"));
    assert!(stderr.contains("sweet, fresh, safe, rotten, toxic"));
}

#[test]
fn test_taste_safe_allows_spawn() {
    let env = TestEnv::new();

    let hash = "b3.111111111111111111111111111111111111111111";

    // Seed a safe taste verdict
    let taste_dir = env
        .dir
        .join(format!("hypha/cache/example.com/spore/{}", hash));
    fs::create_dir_all(&taste_dir).unwrap();
    fs::write(
        taste_dir.join("taste.json"),
        r#"{"verdict":"safe","tasted_at_epoch_ms":1700000000000}"#,
    )
    .unwrap();

    // Try to spawn — should pass taste check (safe allows proceed)
    // Will fail at cmn.json fetch, but that proves taste was not the blocker
    let work_dir = env.dir.join("work");
    fs::create_dir_all(&work_dir).unwrap();

    let output = env.hypha_in_dir(
        &["spawn", &format!("cmn://example.com/{}", hash)],
        &work_dir,
    );
    let stderr = combined_text(&output);
    assert!(
        !stderr.contains("NOT_TASTED"),
        "safe should not trigger NOT_TASTED: {}",
        stderr
    );
    assert!(
        !stderr.contains("TOXIC"),
        "safe should not trigger TOXIC: {}",
        stderr
    );
}

#[test]
fn test_taste_sweet_allows_spawn() {
    let env = TestEnv::new();

    let hash = "b3.111111111111111111111111111111111111111111";

    // Seed a sweet taste verdict
    let taste_dir = env
        .dir
        .join(format!("hypha/cache/example.com/spore/{}", hash));
    fs::create_dir_all(&taste_dir).unwrap();
    fs::write(
        taste_dir.join("taste.json"),
        r#"{"verdict":"sweet","tasted_at_epoch_ms":1700000000000}"#,
    )
    .unwrap();

    let work_dir = env.dir.join("work");
    fs::create_dir_all(&work_dir).unwrap();

    let output = env.hypha_in_dir(
        &["spawn", &format!("cmn://example.com/{}", hash)],
        &work_dir,
    );
    let stderr = combined_text(&output);
    assert!(
        !stderr.contains("NOT_TASTED"),
        "sweet should not trigger NOT_TASTED: {}",
        stderr
    );
    assert!(
        !stderr.contains("TOXIC"),
        "sweet should not trigger TOXIC: {}",
        stderr
    );
}

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

#[cfg(unix)]
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
