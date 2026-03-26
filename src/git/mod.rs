//! Git operations using system `git` command.
//!
//! All functions shell out to `git` via `std::process::Command`.
//! This eliminates the heavy `gix` dependency and works with any
//! git transport (including dumb HTTP).

use std::path::Path;

/// Error type for git operations.
#[derive(Debug, thiserror::Error)]
pub enum GitError {
    /// Failed to spawn or execute the git process.
    #[error("failed to run git: {0}")]
    Exec(#[from] std::io::Error),
    /// Git command exited with non-zero status (stderr captured).
    #[error("{0}")]
    Command(String),
    /// URL rejected by security validation.
    #[error("rejected git URL: {0}")]
    InvalidUrl(String),
}

/// Validate that a git URL is safe for remote operations.
///
/// Delegates to `substrate::normalize_and_validate_url()` for SSRF protection (loopback,
/// private IPs, link-local, CGNAT, userinfo), then additionally rejects
/// non-HTTPS schemes that substrate allows for .onion/.i2p (git must be HTTPS-only).
fn validate_remote_url(url: &str) -> Result<(), GitError> {
    // substrate::normalize_and_validate_url covers: SSRF
    // (private/reserved IPs, localhost, link-local, CGNAT), userinfo, bare
    // hostnames, scheme validation, and trailing-slash normalization.
    let normalized = substrate::normalize_and_validate_url(url)
        .map_err(|e| GitError::InvalidUrl(e.to_string()))?;

    // substrate allows HTTP for .onion/.i2p — git requires strict HTTPS
    let parsed = reqwest::Url::parse(&normalized)
        .map_err(|e| GitError::InvalidUrl(format!("invalid URL syntax ({})", e)))?;
    if parsed.scheme() != "https" {
        return Err(GitError::InvalidUrl(format!(
            "only https:// URLs are allowed (got: {})",
            url
        )));
    }
    Ok(())
}

/// Run a git command and return Ok(()) on success, or the stderr message on failure.
fn run_git(args: &[&str]) -> Result<(), GitError> {
    let output = std::process::Command::new("git").args(args).output()?;
    if !output.status.success() {
        return Err(GitError::Command(
            String::from_utf8_lossy(&output.stderr).trim().to_string(),
        ));
    }
    Ok(())
}

/// Run a git command in a specific directory.
fn run_git_in(dir: &Path, args: &[&str]) -> Result<(), GitError> {
    let output = std::process::Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()?;
    if !output.status.success() {
        return Err(GitError::Command(
            String::from_utf8_lossy(&output.stderr).trim().to_string(),
        ));
    }
    Ok(())
}

/// Run a git command in a specific directory and return stdout.
fn run_git_output(dir: &Path, args: &[&str]) -> Result<String, GitError> {
    let output = std::process::Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()?;
    if !output.status.success() {
        return Err(GitError::Command(
            String::from_utf8_lossy(&output.stderr).trim().to_string(),
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// Check if system git is available.
pub fn is_available() -> bool {
    std::process::Command::new("git")
        .arg("--version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Clone a git repository to the specified destination.
///
/// - `url`: Git repository URL
/// - `dest`: Destination directory (must not exist)
/// - `shallow`: If true, performs a shallow clone (depth 1)
pub fn clone_repo(url: &str, dest: &Path, shallow: bool) -> Result<(), GitError> {
    validate_remote_url(url)?;
    let dest_str = dest.display().to_string();
    if shallow {
        run_git(&["clone", "--depth", "1", url, &dest_str])
    } else {
        run_git(&["clone", url, &dest_str])
    }
}

/// Clone a git repository as a bare repository.
pub fn clone_bare_repo(url: &str, dest: &Path) -> Result<(), GitError> {
    validate_remote_url(url)?;
    let dest_str = dest.display().to_string();
    run_git(&["clone", "--bare", url, &dest_str])
}

/// Clone from a local bare repository to a working directory.
pub fn clone_from_local(local_bare_path: &Path, dest: &Path) -> Result<(), GitError> {
    let src = format!("file://{}", local_bare_path.display());
    let dest_str = dest.display().to_string();
    run_git(&["clone", &src, &dest_str])
}

/// Checkout a specific ref (commit SHA, tag, or branch).
pub fn checkout_ref(repo_path: &Path, ref_spec: &str) -> Result<(), GitError> {
    run_git_in(repo_path, &["checkout", ref_spec])
}

/// Initialize a new git repository at the given path.
pub fn init_repo(path: &Path) -> Result<(), GitError> {
    run_git_in(path, &["init"])
}

/// Add all files and create a commit. Returns the commit SHA.
pub fn add_all_and_commit(repo_path: &Path, message: &str) -> Result<String, GitError> {
    run_git_in(repo_path, &["add", "."])?;
    run_git_in(
        repo_path,
        &[
            "-c",
            "user.name=CMN Hypha",
            "-c",
            "user.email=hypha@cmn.dev",
            "commit",
            "-m",
            message,
        ],
    )?;
    run_git_output(repo_path, &["rev-parse", "HEAD"])
}

/// Get the current HEAD commit ID as a string.
pub fn get_head_commit(repo_path: &Path) -> Result<String, GitError> {
    run_git_output(repo_path, &["rev-parse", "HEAD"])
}

/// Check if a commit exists in the repository.
pub fn commit_exists(repo_path: &Path, commit_sha: &str) -> Result<bool, GitError> {
    let output = std::process::Command::new("git")
        .args(["cat-file", "-t", commit_sha])
        .current_dir(repo_path)
        .output()?;
    Ok(output.status.success())
}

/// Fetch from a remote URL into a bare repository.
pub fn fetch_to_bare(bare_repo_path: &Path, remote_url: &str) -> Result<(), GitError> {
    validate_remote_url(remote_url)?;
    run_git_in(
        bare_repo_path,
        &["fetch", remote_url, "+refs/heads/*:refs/heads/*", "--force"],
    )
}

/// Fetch from a named remote in the repository.
pub fn fetch_from_remote(repo_path: &Path, remote_name: &str) -> Result<(), GitError> {
    run_git_in(repo_path, &["fetch", remote_name])
}

/// Add a remote to the repository.
pub fn add_remote(repo_path: &Path, remote_name: &str, remote_url: &str) -> Result<(), GitError> {
    run_git_in(repo_path, &["remote", "add", remote_name, remote_url])
}

/// Set the URL for an existing remote.
pub fn set_remote_url(repo_path: &Path, remote_name: &str, new_url: &str) -> Result<(), GitError> {
    run_git_in(repo_path, &["remote", "set-url", remote_name, new_url])
}

/// Check if the working directory has uncommitted changes.
///
/// Returns true if clean (no changes), false if dirty.
pub fn is_working_dir_clean(repo_path: &Path) -> Result<bool, GitError> {
    let output = run_git_output(repo_path, &["status", "--porcelain"])?;
    Ok(output.is_empty())
}

/// Get the root commit from a bare repository.
pub fn get_root_commit_bare(bare_repo_path: &Path) -> Result<String, GitError> {
    run_git_output(bare_repo_path, &["rev-list", "--max-parents=0", "HEAD"])
}

/// Get the root commit SHA (first commit in history) from a working directory.
pub fn get_root_commit(repo_path: &Path) -> Result<String, GitError> {
    run_git_output(repo_path, &["rev-list", "--max-parents=0", "HEAD"])
}

/// Get the URL of a named remote, or None if the remote doesn't exist.
pub fn get_remote_url(repo_path: &Path, remote: &str) -> Result<Option<String>, GitError> {
    match run_git_output(repo_path, &["remote", "get-url", remote]) {
        Ok(url) if url.is_empty() => Ok(None),
        Ok(url) => Ok(Some(url)),
        Err(_) => Ok(None),
    }
}
