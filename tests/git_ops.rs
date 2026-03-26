#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

//! Tests for git operations using system git
//!
//! These tests verify that system git operations produce expected results
//! and that content hash is preserved across git clone roundtrips.

use std::fs;
use std::path::PathBuf;
use std::process::Command;
use tempfile::TempDir;

/// Test environment for git operations
struct GitTestEnv {
    _temp: TempDir,
    dir: PathBuf,
}

impl GitTestEnv {
    fn new() -> Self {
        let temp = TempDir::new().expect("Failed to create temp dir");
        let dir = temp.path().to_path_buf();
        Self { _temp: temp, dir }
    }

    /// Run git command and return output
    fn git(&self, args: &[&str]) -> std::process::Output {
        Command::new("git")
            .args(args)
            .current_dir(&self.dir)
            .env("GIT_AUTHOR_NAME", "Test")
            .env("GIT_AUTHOR_EMAIL", "test@test.com")
            .env("GIT_COMMITTER_NAME", "Test")
            .env("GIT_COMMITTER_EMAIL", "test@test.com")
            .output()
            .expect("Failed to run git")
    }

    /// Create test files in the directory
    fn create_test_files(&self) {
        fs::write(self.dir.join("README.md"), "# Test Project\n").unwrap();
        fs::write(
            self.dir.join("main.rs"),
            "fn main() { println!(\"hello\"); }\n",
        )
        .unwrap();

        let src_dir = self.dir.join("src");
        fs::create_dir_all(&src_dir).unwrap();
        fs::write(
            src_dir.join("lib.rs"),
            "pub fn add(a: i32, b: i32) -> i32 { a + b }\n",
        )
        .unwrap();
        fs::write(src_dir.join("util.rs"), "pub fn helper() {}\n").unwrap();
    }
}

#[test]
fn test_git_init_creates_valid_repo() {
    let env = GitTestEnv::new();
    env.create_test_files();

    // Use command-line git to initialize
    let output = env.git(&["init", "-b", "main"]);
    assert!(output.status.success(), "git init failed");

    // Check .git directory exists
    assert!(env.dir.join(".git").exists(), ".git directory should exist");
    assert!(
        env.dir.join(".git/objects").exists(),
        ".git/objects should exist"
    );
    assert!(env.dir.join(".git/refs").exists(), ".git/refs should exist");
}

#[test]
fn test_git_add_and_commit() {
    let env = GitTestEnv::new();
    env.create_test_files();

    // Initialize and commit with git
    env.git(&["init", "-b", "main"]);
    env.git(&["add", "."]);
    let output = env.git(&["commit", "-m", "Initial commit"]);
    assert!(output.status.success(), "git commit failed");

    // Verify commit exists
    let log_output = env.git(&["log", "--oneline"]);
    assert!(log_output.status.success());
    let log = String::from_utf8_lossy(&log_output.stdout);
    assert!(
        log.contains("Initial commit"),
        "commit message should be in log"
    );

    // Verify tree
    let tree_output = env.git(&["ls-tree", "-r", "HEAD"]);
    assert!(tree_output.status.success());
    let tree = String::from_utf8_lossy(&tree_output.stdout);
    assert!(tree.contains("README.md"), "README.md should be in tree");
    assert!(tree.contains("main.rs"), "main.rs should be in tree");
    assert!(tree.contains("src/lib.rs"), "src/lib.rs should be in tree");
    assert!(
        tree.contains("src/util.rs"),
        "src/util.rs should be in tree"
    );
}

#[test]
fn test_git_tree_structure() {
    let env = GitTestEnv::new();
    env.create_test_files();

    // Initialize and commit
    env.git(&["init", "-b", "main"]);
    env.git(&["add", "."]);
    env.git(&["commit", "-m", "Test commit"]);

    // Get tree hash
    let tree_output = env.git(&["rev-parse", "HEAD^{tree}"]);
    assert!(tree_output.status.success());
    let tree_hash = String::from_utf8_lossy(&tree_output.stdout)
        .trim()
        .to_string();

    // Verify tree hash is valid (40 hex chars for SHA-1)
    assert_eq!(tree_hash.len(), 40, "tree hash should be 40 hex chars");
    assert!(
        tree_hash.chars().all(|c| c.is_ascii_hexdigit()),
        "tree hash should be hex"
    );

    // Get tree contents
    let ls_tree = env.git(&["ls-tree", &tree_hash]);
    assert!(ls_tree.status.success());
    let tree_content = String::from_utf8_lossy(&ls_tree.stdout);

    // Verify entries
    assert!(
        tree_content.contains("100644 blob"),
        "should have blob entries"
    );
    assert!(
        tree_content.contains("040000 tree"),
        "should have tree entry for src/"
    );
}

#[test]
fn test_git_working_dir_clean_detection() {
    let env = GitTestEnv::new();
    env.create_test_files();

    // Initialize and commit
    env.git(&["init", "-b", "main"]);
    env.git(&["add", "."]);
    env.git(&["commit", "-m", "Initial"]);

    // Check clean status
    let status = env.git(&["status", "--porcelain"]);
    assert!(status.status.success());
    let status_out = String::from_utf8_lossy(&status.stdout);
    assert!(status_out.is_empty(), "working dir should be clean");

    // Modify a file
    fs::write(env.dir.join("README.md"), "# Modified\n").unwrap();

    // Check dirty status
    let status = env.git(&["status", "--porcelain"]);
    assert!(status.status.success());
    let status_out = String::from_utf8_lossy(&status.stdout);
    assert!(
        status_out.contains("README.md"),
        "should show modified file"
    );
}

#[test]
fn test_git_commit_with_parent() {
    let env = GitTestEnv::new();

    // Create initial file and commit
    fs::write(env.dir.join("file1.txt"), "content1").unwrap();
    env.git(&["init", "-b", "main"]);
    env.git(&["add", "."]);
    env.git(&["commit", "-m", "First commit"]);

    // Get first commit hash
    let first_hash = env.git(&["rev-parse", "HEAD"]);
    let first_hash = String::from_utf8_lossy(&first_hash.stdout)
        .trim()
        .to_string();

    // Create second file and commit
    fs::write(env.dir.join("file2.txt"), "content2").unwrap();
    env.git(&["add", "."]);
    env.git(&["commit", "-m", "Second commit"]);

    // Get second commit hash
    let second_hash = env.git(&["rev-parse", "HEAD"]);
    let second_hash = String::from_utf8_lossy(&second_hash.stdout)
        .trim()
        .to_string();

    // Verify parent relationship
    let parent_hash = env.git(&["rev-parse", "HEAD^"]);
    let parent_hash = String::from_utf8_lossy(&parent_hash.stdout)
        .trim()
        .to_string();

    assert_eq!(
        first_hash, parent_hash,
        "first commit should be parent of second"
    );
    assert_ne!(first_hash, second_hash, "commits should be different");
}

#[test]
fn test_git_root_commit_detection() {
    let env = GitTestEnv::new();

    // Create two commits
    fs::write(env.dir.join("file1.txt"), "content1").unwrap();
    env.git(&["init", "-b", "main"]);
    env.git(&["add", "."]);
    env.git(&["commit", "-m", "Root commit"]);

    let root_hash = env.git(&["rev-parse", "HEAD"]);
    let root_hash = String::from_utf8_lossy(&root_hash.stdout)
        .trim()
        .to_string();

    fs::write(env.dir.join("file2.txt"), "content2").unwrap();
    env.git(&["add", "."]);
    env.git(&["commit", "-m", "Second commit"]);

    // Find root commit by walking back
    let log_output = env.git(&["rev-list", "--max-parents=0", "HEAD"]);
    let found_root = String::from_utf8_lossy(&log_output.stdout)
        .trim()
        .to_string();

    assert_eq!(root_hash, found_root, "should find the root commit");
}

/// Round-trip test: hash → git commit → clone → hash must match
///
/// This tests the critical invariant: content hashed during `hypha release`
/// must produce the same hash after `git clone` + checkout.
#[test]
fn test_hash_roundtrip_git_clone() {
    let source = GitTestEnv::new();

    // Create source files
    fs::write(source.dir.join("README.md"), "# Test\n").unwrap();
    fs::write(source.dir.join("lib.rs"), "pub fn hello() {}\n").unwrap();
    let sub = source.dir.join("src");
    fs::create_dir_all(&sub).unwrap();
    fs::write(sub.join("main.rs"), "fn main() {}\n").unwrap();

    // Compute hash (same params as spore.core.json: exclude_names=[".git"], follow_rules=[".gitignore"])
    let hash_before = hypha::tree::compute_tree_hash(
        &source.dir,
        &substrate::SporeTree {
            algorithm: "blob_tree_blake3_nfc".to_string(),
            exclude_names: vec![".git".to_string()],
            follow_rules: vec![".gitignore".to_string()],
        },
    )
    .expect("hash before");

    // Git init + add + commit
    source.git(&["init", "-b", "main"]);
    source.git(&["add", "."]);
    source.git(&["commit", "-m", "initial"]);

    // Clone to a new directory
    let clone_dir = TempDir::new().expect("clone dir");
    let output = Command::new("git")
        .args([
            "clone",
            &source.dir.display().to_string(),
            &clone_dir.path().display().to_string(),
        ])
        .output()
        .expect("git clone");
    assert!(output.status.success(), "git clone failed");

    // Compute hash on cloned content (same params)
    let hash_after = hypha::tree::compute_tree_hash(
        clone_dir.path(),
        &substrate::SporeTree {
            algorithm: "blob_tree_blake3_nfc".to_string(),
            exclude_names: vec![".git".to_string()],
            follow_rules: vec![".gitignore".to_string()],
        },
    )
    .expect("hash after");

    assert_eq!(
        hash_before, hash_after,
        "hash must match after git clone roundtrip"
    );
}

/// Round-trip test with .gitignore: gitignored files must be handled consistently
///
/// Verifies that follow_rules filenames (e.g., ".gitignore") are used consistently
/// in both hash computation and file collection.
#[test]
fn test_hash_roundtrip_with_gitignore() {
    let source = GitTestEnv::new();

    // Create .gitignore
    fs::write(source.dir.join(".gitignore"), "*.log\nbuild/\n").unwrap();

    // Create normal files
    fs::write(source.dir.join("README.md"), "# Test\n").unwrap();
    fs::write(source.dir.join("lib.rs"), "pub fn hello() {}\n").unwrap();

    // Create gitignored files
    fs::write(source.dir.join("debug.log"), "some log data\n").unwrap();
    let build = source.dir.join("build");
    fs::create_dir_all(&build).unwrap();
    fs::write(build.join("output.bin"), "binary data\n").unwrap();

    // Compute hash with follow_rules=[".gitignore"] (as spore.core.json uses)
    let hash_before = hypha::tree::compute_tree_hash(
        &source.dir,
        &substrate::SporeTree {
            algorithm: "blob_tree_blake3_nfc".to_string(),
            exclude_names: vec![".git".to_string()],
            follow_rules: vec![".gitignore".to_string()],
        },
    )
    .expect("hash before");

    // Git init + add + commit (git add respects .gitignore)
    source.git(&["init", "-b", "main"]);
    source.git(&["add", "."]);
    source.git(&["commit", "-m", "initial"]);

    // Clone to a new directory
    let clone_dir = TempDir::new().expect("clone dir");
    let output = Command::new("git")
        .args([
            "clone",
            &source.dir.display().to_string(),
            &clone_dir.path().display().to_string(),
        ])
        .output()
        .expect("git clone");
    assert!(output.status.success(), "git clone failed");

    // Compute hash on cloned content (same params)
    let hash_after = hypha::tree::compute_tree_hash(
        clone_dir.path(),
        &substrate::SporeTree {
            algorithm: "blob_tree_blake3_nfc".to_string(),
            exclude_names: vec![".git".to_string()],
            follow_rules: vec![".gitignore".to_string()],
        },
    )
    .expect("hash after");

    assert_eq!(
        hash_before, hash_after,
        "hash must match after git clone roundtrip (with .gitignore)"
    );
}

/// Test that executable files preserve their mode through git roundtrip
#[cfg(unix)]
#[test]
fn test_hash_roundtrip_executable_files() {
    use std::os::unix::fs::PermissionsExt;

    let source = GitTestEnv::new();

    // Create a regular file and an executable file
    fs::write(source.dir.join("README.md"), "# Test\n").unwrap();
    fs::write(source.dir.join("run.sh"), "#!/bin/sh\necho hi\n").unwrap();
    fs::set_permissions(source.dir.join("run.sh"), fs::Permissions::from_mode(0o755)).unwrap();

    // Compute hash
    let hash_before = hypha::tree::compute_tree_hash(
        &source.dir,
        &substrate::SporeTree {
            algorithm: "blob_tree_blake3_nfc".to_string(),
            exclude_names: vec![".git".to_string()],
            follow_rules: vec![".gitignore".to_string()],
        },
    )
    .expect("hash before");

    // Git init + add + commit
    source.git(&["init", "-b", "main"]);
    source.git(&["add", "."]);
    source.git(&["commit", "-m", "initial"]);

    // Clone
    let clone_dir = TempDir::new().expect("clone dir");
    let output = Command::new("git")
        .args([
            "clone",
            &source.dir.display().to_string(),
            &clone_dir.path().display().to_string(),
        ])
        .output()
        .expect("git clone");
    assert!(output.status.success(), "git clone failed");

    // Verify executable bit preserved
    let cloned_perms = fs::metadata(clone_dir.path().join("run.sh"))
        .unwrap()
        .permissions();
    assert!(
        cloned_perms.mode() & 0o111 != 0,
        "executable bit should be preserved after clone"
    );

    // Compute hash
    let hash_after = hypha::tree::compute_tree_hash(
        clone_dir.path(),
        &substrate::SporeTree {
            algorithm: "blob_tree_blake3_nfc".to_string(),
            exclude_names: vec![".git".to_string()],
            follow_rules: vec![".gitignore".to_string()],
        },
    )
    .expect("hash after");

    assert_eq!(
        hash_before, hash_after,
        "hash must match after git clone roundtrip (executable files)"
    );
}
