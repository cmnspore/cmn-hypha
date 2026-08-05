#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod common;
use common::*;
use std::fs;

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
    let event = parse_json_last_line(&stderr);
    assert_eq!(event["error"]["code"], "cli_invalid_argument_value");
    assert!(stderr.contains("invalid value for `--verdict`"));
    assert!(stderr.contains("sweet, fresh, safe, rotten, toxic"));
    assert!(
        !stderr.contains("delicious"),
        "closed-world parser errors must not echo raw values: {}",
        stderr
    );
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
