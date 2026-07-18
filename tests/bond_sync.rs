#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod common;
use common::*;
use std::fs;
use std::path::{Path, PathBuf};

fn init_domain(env: &TestEnv, domain: &str) {
    let output = env.hypha(&[
        "mycelium",
        "root",
        domain,
        "--endpoints-base",
        &format!("https://{domain}"),
    ]);
    assert!(
        output.status.success(),
        "mycelium root failed: {}",
        combined_text(&output)
    );
}

fn hatch_spore(env: &TestEnv, dir: &PathBuf, domain: &str, id: &str) {
    fs::create_dir_all(dir).expect("create spore dir");
    let output = env.hypha_in_dir(
        &[
            "hatch", "--domain", domain, "--id", id, "--name", id, "--intent", "test",
        ],
        dir,
    );
    assert!(
        output.status.success(),
        "hatch failed for {}: {}",
        id,
        combined_text(&output)
    );
}

fn read_core(dir: &Path) -> serde_json::Value {
    let content = fs::read_to_string(dir.join("spore.core.json")).expect("read spore.core.json");
    serde_json::from_str(&content).expect("parse spore.core.json")
}

fn sync(
    env: &TestEnv,
    dir: &PathBuf,
    relation: &str,
    spec_json: &str,
    extra_args: &[&str],
) -> std::process::Output {
    fs::write(dir.join("spec.json"), spec_json).expect("write spec.json");
    let mut args = vec![
        "hatch",
        "bond",
        "sync",
        "--relation",
        relation,
        "--spec",
        "spec.json",
    ];
    args.extend_from_slice(extra_args);
    env.hypha_in_dir(&args, dir)
}

// ── Explicit `uri` path: add, replace-`with`, and per-entry matching ───────

#[test]
fn test_bond_sync_explicit_uri_adds_bonds_in_spec_order() {
    let env = TestEnv::new();
    init_domain(&env, "test.local");
    let spore_dir = env.dir.join("spore-explicit");
    hatch_spore(&env, &spore_dir, "test.local", "spore-explicit");

    let spec = r#"[
        {"id": "a", "uri": "cmn://test.local/b3.aaa", "reason": "r1"},
        {"id": "b", "uri": "cmn://test.local/b3.bbb", "with": {"pinned": true}}
    ]"#;
    let output = sync(&env, &spore_dir, "follows", spec, &[]);
    assert!(
        output.status.success(),
        "sync should succeed: {}",
        combined_text(&output)
    );
    let json = parse_json_last_line(&combined_text(&output));
    assert_eq!(json["result"]["applied"].as_array().unwrap().len(), 2);
    assert_eq!(json["result"]["removed"].as_array().unwrap().len(), 0);
    assert_eq!(json["result"]["unchanged"].as_array().unwrap().len(), 0);

    let core = read_core(&spore_dir);
    let bonds = core["bonds"].as_array().unwrap();
    assert_eq!(bonds.len(), 2, "unexpected bonds: {}", core["bonds"]);
    assert_eq!(bonds[0]["uri"], "cmn://test.local/b3.aaa");
    assert_eq!(bonds[0]["relation"], "follows");
    assert_eq!(bonds[1]["uri"], "cmn://test.local/b3.bbb");
    assert_eq!(bonds[1]["with"], serde_json::json!({"pinned": true}));
}

#[test]
fn test_bond_sync_is_idempotent_second_run_reports_zero_changes() {
    let env = TestEnv::new();
    init_domain(&env, "test.local");
    let spore_dir = env.dir.join("spore-idempotent");
    hatch_spore(&env, &spore_dir, "test.local", "spore-idempotent");

    let spec = r#"[
        {"id": "a", "uri": "cmn://test.local/b3.aaa", "reason": "r1"},
        {"id": "b", "uri": "cmn://test.local/b3.bbb", "with": {"pinned": true}}
    ]"#;
    let first = sync(&env, &spore_dir, "follows", spec, &[]);
    assert!(first.status.success());

    let before = fs::read_to_string(spore_dir.join("spore.core.json")).unwrap();

    let second = sync(&env, &spore_dir, "follows", spec, &[]);
    assert!(
        second.status.success(),
        "second sync should succeed: {}",
        combined_text(&second)
    );
    let json = parse_json_last_line(&combined_text(&second));
    assert_eq!(
        json["result"]["applied"].as_array().unwrap().len(),
        0,
        "second run should apply nothing: {}",
        json
    );
    assert_eq!(json["result"]["removed"].as_array().unwrap().len(), 0);
    assert_eq!(json["result"]["unchanged"].as_array().unwrap().len(), 2);

    let after = fs::read_to_string(spore_dir.join("spore.core.json")).unwrap();
    assert_eq!(
        before, after,
        "spore.core.json must be byte-for-byte identical after a no-op sync"
    );
}

#[test]
fn test_bond_sync_with_replace_semantics_converges_in_one_pass() {
    // §2/§1 interaction: a spec that drops a `with` key must fully replace the
    // existing `with` object (not merge), and the very next run must be a no-op.
    let env = TestEnv::new();
    init_domain(&env, "test.local");
    let spore_dir = env.dir.join("spore-with-replace");
    hatch_spore(&env, &spore_dir, "test.local", "spore-with-replace");

    // Seed an existing follows bond with two `with` keys via `hatch bond set`.
    let seed = env.hypha_in_dir(
        &[
            "hatch",
            "bond",
            "set",
            "--uri",
            "cmn://test.local/b3.aaa",
            "--relation",
            "follows",
            "--id",
            "a",
            "--with",
            "a=x",
            "--with",
            "b=y",
        ],
        &spore_dir,
    );
    assert!(
        seed.status.success(),
        "seed failed: {}",
        combined_text(&seed)
    );

    // Spec only keeps key "a" (with a new value) — "b" must be dropped.
    let spec = r#"[{"id": "a", "uri": "cmn://test.local/b3.aaa", "with": {"a": "z"}}]"#;
    let output = sync(&env, &spore_dir, "follows", spec, &[]);
    assert!(
        output.status.success(),
        "sync failed: {}",
        combined_text(&output)
    );

    let core = read_core(&spore_dir);
    let bonds = core["bonds"].as_array().unwrap();
    assert_eq!(bonds.len(), 1);
    assert_eq!(
        bonds[0]["with"],
        serde_json::json!({"a": "z"}),
        "with must be replaced wholesale, got: {}",
        bonds[0]["with"]
    );

    // Converges in exactly one pass.
    let second = sync(&env, &spore_dir, "follows", spec, &[]);
    let json = parse_json_last_line(&combined_text(&second));
    assert_eq!(
        json["result"]["applied"].as_array().unwrap().len(),
        0,
        "with-key removal should converge in a single sync pass: {}",
        json
    );
}

// ── Other relations are left untouched ──────────────────────────────────

#[test]
fn test_bond_sync_preserves_other_relations_exactly() {
    let env = TestEnv::new();
    init_domain(&env, "test.local");
    let spore_dir = env.dir.join("spore-preserve");
    hatch_spore(&env, &spore_dir, "test.local", "spore-preserve");

    // Seed bonds: depends_on, then follows (to be replaced), then extends.
    for (relation, uri, id) in [
        ("depends_on", "cmn://test.local/b3.dep1", "dep1"),
        ("follows", "cmn://test.local/b3.prev", "prev"),
        ("extends", "cmn://test.local/b3.ext1", "ext1"),
    ] {
        let output = env.hypha_in_dir(
            &[
                "hatch",
                "bond",
                "set",
                "--uri",
                uri,
                "--relation",
                relation,
                "--id",
                id,
            ],
            &spore_dir,
        );
        assert!(
            output.status.success(),
            "seed bond failed: {}",
            combined_text(&output)
        );
    }

    let before = read_core(&spore_dir);
    let depends_on_before = before["bonds"][0].clone();
    let extends_before = before["bonds"][2].clone();
    assert_eq!(depends_on_before["relation"], "depends_on");
    assert_eq!(extends_before["relation"], "extends");

    let spec = r#"[{"id": "new", "uri": "cmn://test.local/b3.new"}]"#;
    let output = sync(&env, &spore_dir, "follows", spec, &[]);
    assert!(
        output.status.success(),
        "sync failed: {}",
        combined_text(&output)
    );

    let after = read_core(&spore_dir);
    let bonds = after["bonds"].as_array().unwrap();

    // The follows relation now contains exactly the spec.
    let follows: Vec<_> = bonds
        .iter()
        .filter(|b| b["relation"] == "follows")
        .collect();
    assert_eq!(follows.len(), 1);
    assert_eq!(follows[0]["uri"], "cmn://test.local/b3.new");

    // depends_on and extends bonds are byte-for-byte unchanged...
    let depends_on_after: Vec<_> = bonds
        .iter()
        .filter(|b| b["relation"] == "depends_on")
        .collect();
    let extends_after: Vec<_> = bonds
        .iter()
        .filter(|b| b["relation"] == "extends")
        .collect();
    assert_eq!(depends_on_after.len(), 1);
    assert_eq!(extends_after.len(), 1);
    assert_eq!(*depends_on_after[0], depends_on_before);
    assert_eq!(*extends_after[0], extends_before);

    // ...and keep their relative order (depends_on still before extends).
    let depends_on_pos = bonds
        .iter()
        .position(|b| b["relation"] == "depends_on")
        .unwrap();
    let extends_pos = bonds
        .iter()
        .position(|b| b["relation"] == "extends")
        .unwrap();
    assert!(
        depends_on_pos < extends_pos,
        "depends_on/extends relative order must be preserved: {:?}",
        bonds
    );
}

// ── Empty spec clears the relation ──────────────────────────────────────

#[test]
fn test_bond_sync_empty_spec_clears_relation() {
    let env = TestEnv::new();
    init_domain(&env, "test.local");
    let spore_dir = env.dir.join("spore-clear");
    hatch_spore(&env, &spore_dir, "test.local", "spore-clear");

    for (relation, uri, id) in [
        ("follows", "cmn://test.local/b3.f1", "f1"),
        ("follows", "cmn://test.local/b3.f2", "f2"),
        ("depends_on", "cmn://test.local/b3.d1", "d1"),
    ] {
        let output = env.hypha_in_dir(
            &[
                "hatch",
                "bond",
                "set",
                "--uri",
                uri,
                "--relation",
                relation,
                "--id",
                id,
            ],
            &spore_dir,
        );
        assert!(output.status.success());
    }

    let output = sync(&env, &spore_dir, "follows", "[]", &[]);
    assert!(
        output.status.success(),
        "sync failed: {}",
        combined_text(&output)
    );
    let json = parse_json_last_line(&combined_text(&output));
    assert_eq!(json["result"]["removed"].as_array().unwrap().len(), 2);

    let core = read_core(&spore_dir);
    let bonds = core["bonds"].as_array().unwrap();
    assert_eq!(bonds.len(), 1, "only depends_on should remain: {:?}", bonds);
    assert_eq!(bonds[0]["relation"], "depends_on");
}

// ── `--check`: diff only, no write, distinct drift exit code ────────────

#[test]
fn test_bond_sync_check_reports_drift_without_writing() {
    let env = TestEnv::new();
    init_domain(&env, "test.local");
    let spore_dir = env.dir.join("spore-check");
    hatch_spore(&env, &spore_dir, "test.local", "spore-check");

    let seed = env.hypha_in_dir(
        &[
            "hatch",
            "bond",
            "set",
            "--uri",
            "cmn://test.local/b3.aaa",
            "--relation",
            "follows",
            "--id",
            "a",
            "--reason",
            "original",
        ],
        &spore_dir,
    );
    assert!(seed.status.success());

    let before = fs::read_to_string(spore_dir.join("spore.core.json")).unwrap();

    // Drifted spec: same id/uri but a different reason, plus a brand new entry.
    let spec = r#"[
        {"id": "a", "uri": "cmn://test.local/b3.aaa", "reason": "changed"},
        {"id": "b", "uri": "cmn://test.local/b3.bbb"}
    ]"#;
    let output = sync(&env, &spore_dir, "follows", spec, &["--check"]);
    assert_eq!(
        output.status.code(),
        Some(7),
        "drifted --check should exit with the distinct drift code: {}",
        combined_text(&output)
    );
    let json = parse_json_last_line(&combined_text(&output));
    assert_eq!(json["result"]["drift"], true);
    assert_eq!(json["result"]["to_update"].as_array().unwrap().len(), 1);
    assert_eq!(json["result"]["to_add"].as_array().unwrap().len(), 1);
    assert_eq!(json["result"]["to_remove"].as_array().unwrap().len(), 0);

    let after = fs::read_to_string(spore_dir.join("spore.core.json")).unwrap();
    assert_eq!(before, after, "--check must never write spore.core.json");
}

#[test]
fn test_bond_sync_check_no_drift_exits_zero() {
    let env = TestEnv::new();
    init_domain(&env, "test.local");
    let spore_dir = env.dir.join("spore-check-clean");
    hatch_spore(&env, &spore_dir, "test.local", "spore-check-clean");

    let spec = r#"[{"id": "a", "uri": "cmn://test.local/b3.aaa"}]"#;
    let apply = sync(&env, &spore_dir, "follows", spec, &[]);
    assert!(apply.status.success());

    let output = sync(&env, &spore_dir, "follows", spec, &["--check"]);
    assert!(
        output.status.success(),
        "clean --check should exit 0: {}",
        combined_text(&output)
    );
    let json = parse_json_last_line(&combined_text(&output));
    assert_eq!(json["result"]["drift"], false);
}

// ── URI resolution: deployed inventory + release --dry-run fallback ─────

#[test]
fn test_bond_sync_resolves_uri_via_dry_run_then_deployed_inventory() {
    let env = TestEnv::new();
    init_domain(&env, "test.local");

    // Sibling spore sources live side-by-side under a common parent, matching
    // the monorepo convention (`spores/<id>/`) the resolution fallback assumes.
    let workspace = env.dir.join("workspace");
    let sibling_dir = workspace.join("sibling-a");
    let main_dir = workspace.join("main-spore");
    hatch_spore(&env, &sibling_dir, "test.local", "sibling-a");
    hatch_spore(&env, &main_dir, "test.local", "main-spore");

    // sibling-a has not been released yet — resolution must fall back to a
    // `release --dry-run`-equivalent computation against ../sibling-a.
    let spec = r#"[{"id": "sibling-a"}]"#;
    let output = sync(
        &env,
        &main_dir,
        "depends_on",
        spec,
        &["--domain", "test.local"],
    );
    assert!(
        output.status.success(),
        "dry-run fallback resolution should succeed: {}",
        combined_text(&output)
    );
    let json = parse_json_last_line(&combined_text(&output));
    let applied = json["result"]["applied"].as_array().unwrap();
    assert_eq!(applied.len(), 1);
    let dry_run_uri = applied[0]["uri"].as_str().unwrap().to_string();
    assert!(
        dry_run_uri.starts_with("cmn://test.local/b3."),
        "unexpected resolved uri: {}",
        dry_run_uri
    );

    // Now actually release sibling-a, and re-sync with the *same* spec: this
    // time resolution should hit the deployed mycelium inventory fast path
    // and — since nothing in sibling-a's tree changed — resolve to the exact
    // same URI, converging with zero changes.
    let release = env.hypha_in_dir(&["release", "--domain", "test.local"], &sibling_dir);
    assert!(
        release.status.success(),
        "release failed: {}",
        combined_text(&release)
    );

    let output2 = sync(
        &env,
        &main_dir,
        "depends_on",
        spec,
        &["--domain", "test.local"],
    );
    assert!(output2.status.success());
    let json2 = parse_json_last_line(&combined_text(&output2));
    assert_eq!(
        json2["result"]["applied"].as_array().unwrap().len(),
        0,
        "deployed-inventory resolution should match the earlier dry-run URI exactly: {}",
        json2
    );
    let unchanged = json2["result"]["unchanged"].as_array().unwrap();
    assert_eq!(unchanged.len(), 1);
    assert_eq!(unchanged[0]["uri"].as_str().unwrap(), dry_run_uri);
}

#[test]
fn test_bond_sync_missing_domain_errors_when_resolution_needed() {
    let env = TestEnv::new();
    init_domain(&env, "test.local");
    let spore_dir = env.dir.join("spore-missing-domain");
    hatch_spore(&env, &spore_dir, "test.local", "spore-missing-domain");

    let spec = r#"[{"id": "some-dep"}]"#;
    let output = sync(&env, &spore_dir, "depends_on", spec, &[]);
    assert!(!output.status.success(), "should fail without --domain");
    let text = combined_text(&output);
    assert!(
        text.contains("missing_domain"),
        "should report missing_domain: {}",
        text
    );
}

#[test]
fn test_bond_sync_unresolvable_uri_errors_with_hint() {
    let env = TestEnv::new();
    init_domain(&env, "test.local");
    let spore_dir = env.dir.join("spore-unresolvable");
    hatch_spore(&env, &spore_dir, "test.local", "spore-unresolvable");

    // Not in the deployed inventory, and no sibling directory named "ghost".
    let spec = r#"[{"id": "ghost"}]"#;
    let output = sync(
        &env,
        &spore_dir,
        "depends_on",
        spec,
        &["--domain", "test.local"],
    );
    assert!(!output.status.success(), "should fail to resolve");
    let text = combined_text(&output);
    assert!(
        text.contains("bond_uri_unresolved"),
        "should report bond_uri_unresolved: {}",
        text
    );
}

#[test]
fn test_bond_sync_invalid_spec_json_errors() {
    let env = TestEnv::new();
    init_domain(&env, "test.local");
    let spore_dir = env.dir.join("spore-bad-spec");
    hatch_spore(&env, &spore_dir, "test.local", "spore-bad-spec");

    let output = sync(&env, &spore_dir, "follows", "{ not json", &[]);
    assert!(!output.status.success());
    let text = combined_text(&output);
    assert!(
        text.contains("bond_spec_invalid"),
        "should report bond_spec_invalid: {}",
        text
    );
}
