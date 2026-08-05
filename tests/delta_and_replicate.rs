#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod common;
use common::*;
use std::fs;

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
    assert!(refs[0]["cmn_url"].as_str().unwrap().contains("example.com"));
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
    let _uri = release_result["result"]["cmn_url"].as_str().unwrap();

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
