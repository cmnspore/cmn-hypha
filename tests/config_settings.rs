#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod common;
use common::*;

#[test]
fn test_config_set_spore_cache_keys() {
    let env = TestEnv::new();
    for (key, value) in [
        ("cache.spore_max_download_bytes", "1000"),
        ("cache.spore_max_extract_bytes", "2000"),
        ("cache.spore_max_extract_files", "3000"),
        ("cache.spore_max_extract_file_bytes", "4000"),
        (
            "cache.spore_reject_path_components",
            r#"[".git", ".cmn", "control"]"#,
        ),
    ] {
        let output = env.hypha(&["config", "set", key, value]);
        assert!(
            output.status.success(),
            "config set {} failed: {}",
            key,
            combined_text(&output)
        );
    }

    let output = env.hypha(&["config", "list"]);
    assert!(
        output.status.success(),
        "config list failed: {}",
        combined_text(&output)
    );
    let json = parse_json_last_line(&String::from_utf8_lossy(&output.stdout));
    let cache = &json["result"]["config"]["cache"];
    assert_eq!(cache["spore_max_download_bytes"], 1000);
    assert_eq!(cache["spore_max_extract_bytes"], 2000);
    assert_eq!(cache["spore_max_extract_files"], 3000);
    assert_eq!(cache["spore_max_extract_file_bytes"], 4000);
    assert_eq!(
        cache["spore_reject_path_components"],
        serde_json::json!([".git", ".cmn", "control"])
    );
}

#[test]
fn test_config_set_rejects_legacy_spore_cache_keys() {
    let env = TestEnv::new();
    for key in [
        "cache.max_download_bytes",
        "cache.max_extract_bytes",
        "cache.max_extract_files",
        "cache.max_extract_file_bytes",
    ] {
        let output = env.hypha(&["config", "set", key, "1"]);
        let text = combined_text(&output);
        assert!(!output.status.success(), "{} should fail: {}", key, text);
        assert!(text.contains("unknown_key"), "unexpected error: {}", text);
        let valid_keys_text = text.split("Valid keys:").nth(1).unwrap_or("");
        assert!(
            !valid_keys_text.contains("cache.max_download_bytes")
                && !valid_keys_text.contains("cache.max_extract_bytes")
                && !valid_keys_text.contains("cache.max_extract_files")
                && !valid_keys_text.contains("cache.max_extract_file_bytes"),
            "legacy keys should not appear in valid-key help: {}",
            text
        );
    }
}
