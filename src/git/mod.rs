//! Git operations using system `git` command.
//!
//! All functions shell out to `git` via `std::process::Command`.
//! This eliminates the heavy `gix` dependency and works with any
//! git transport (including dumb HTTP).

use std::ffi::OsStr;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

/// Default wall-clock limit for every system git invocation.
pub const GIT_COMMAND_TIMEOUT: Duration = Duration::from_secs(300);

const GIT_POLL_INTERVAL: Duration = Duration::from_millis(50);
const BLOBLESS_FILTER: &str = "blob:none";
pub const CMN_PROMISOR_REMOTE: &str = "cmn-promisor";

/// Error type for git operations.
#[derive(Debug, thiserror::Error)]
pub enum GitError {
    /// Failed to spawn or execute the git process.
    #[error("failed to run git: {0}")]
    Exec(#[from] std::io::Error),
    /// Git command exited with non-zero status (stderr captured).
    #[error("{0}")]
    Command(String),
    /// Git command exceeded the wall-clock timeout.
    #[error("git command timed out after {timeout_secs}s: {command}")]
    Timeout { command: String, timeout_secs: u64 },
    /// Git content exceeded the configured local size budget.
    #[error("git size budget exceeded: {0}")]
    SizeLimit(String),
    /// URL rejected by security validation.
    #[error("rejected git URL: {0}")]
    InvalidUrl(String),
    /// Argument rejected by the option-injection guard.
    #[error("rejected git argument: {0}")]
    InvalidArg(String),
}

/// Disk budget for cloned/fetched git repositories.
#[derive(Debug, Clone, Copy)]
pub struct GitSizeLimits {
    pub max_bytes: u64,
    pub max_files: u64,
}

impl GitSizeLimits {
    pub fn new(max_bytes: u64, max_files: u64) -> Self {
        Self {
            max_bytes,
            max_files,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GitSizeStats {
    pub bytes: u64,
    pub files: u64,
}

/// Reject a value that could be misinterpreted by git as an option flag.
///
/// Refs, remote names, and URLs are passed positionally; a leading `-` would
/// otherwise be parsed as a flag (e.g. a ref named `--upload-pack=...`).
fn reject_option_like(value: &str, what: &str) -> Result<(), GitError> {
    if value.starts_with('-') {
        return Err(GitError::InvalidArg(format!(
            "{} must not start with '-': {}",
            what, value
        )));
    }
    Ok(())
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
    let normalized = substrate::normalize_and_validate_url(url).map_err(|e| {
        let safe_url = agent_first_data::redact_url_secrets(url);
        GitError::InvalidUrl(e.to_string().replace(url, &safe_url))
    })?;

    // substrate allows HTTP for .onion/.i2p — git requires strict HTTPS
    let parsed = reqwest::Url::parse(&normalized)
        .map_err(|e| GitError::InvalidUrl(format!("invalid URL syntax ({})", e)))?;
    if parsed.scheme() != "https" {
        let safe_url = agent_first_data::redact_url_secrets(url);
        return Err(GitError::InvalidUrl(format!(
            "only https:// URLs are allowed (got: {})",
            safe_url
        )));
    }
    Ok(())
}

/// Run a git command (optionally in `dir`), erroring with captured stderr on
/// non-zero exit. Returns the raw `Output` so callers can read stdout.
fn display_command(program: &str, args: &[impl AsRef<OsStr>]) -> String {
    let mut parts = vec![program.to_string()];
    parts.extend(
        args.iter()
            .map(|arg| arg.as_ref().to_string_lossy().into_owned()),
    );
    parts.join(" ")
}

fn run_program_raw<S: AsRef<OsStr>>(
    program: &str,
    dir: Option<&Path>,
    args: &[S],
    timeout: Duration,
) -> Result<Output, GitError> {
    let command_display = display_command(program, args);
    let mut stdout_file = tempfile::tempfile()?;
    let mut stderr_file = tempfile::tempfile()?;

    let mut cmd = Command::new(program);
    cmd.args(args.iter().map(|arg| arg.as_ref()));
    if let Some(d) = dir {
        cmd.current_dir(d);
    }
    cmd.stdout(Stdio::from(stdout_file.try_clone()?));
    cmd.stderr(Stdio::from(stderr_file.try_clone()?));

    let mut child = cmd.spawn()?;
    let started = Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if started.elapsed() >= timeout {
            let _ = child.kill();
            let _ = child.wait();
            return Err(GitError::Timeout {
                command: command_display,
                timeout_secs: timeout.as_secs(),
            });
        }
        std::thread::sleep(GIT_POLL_INTERVAL.min(timeout.saturating_sub(started.elapsed())));
    };

    let mut stdout = Vec::new();
    stdout_file.seek(SeekFrom::Start(0))?;
    stdout_file.read_to_end(&mut stdout)?;
    let mut stderr = Vec::new();
    stderr_file.seek(SeekFrom::Start(0))?;
    stderr_file.read_to_end(&mut stderr)?;

    Ok(Output {
        status,
        stdout,
        stderr,
    })
}

/// Run a git command (optionally in `dir`) and return the raw output without
/// treating non-zero exit status as an error.
fn run_git_raw_unchecked<S: AsRef<OsStr>>(
    dir: Option<&Path>,
    args: &[S],
) -> Result<Output, GitError> {
    run_program_raw("git", dir, args, GIT_COMMAND_TIMEOUT)
}

/// Run a git command (optionally in `dir`), erroring with captured stderr on
/// non-zero exit. Returns the raw `Output` so callers can read stdout.
fn run_git_raw<S: AsRef<OsStr>>(dir: Option<&Path>, args: &[S]) -> Result<Output, GitError> {
    let output = run_git_raw_unchecked(dir, args)?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let message = if stderr.is_empty() {
            format!(
                "{} exited with {}",
                display_command("git", args),
                output.status
            )
        } else {
            stderr
        };
        return Err(GitError::Command(message));
    }
    Ok(output)
}

/// Run a git command and return Ok(()) on success, or the stderr message on failure.
fn run_git<S: AsRef<OsStr>>(args: &[S]) -> Result<(), GitError> {
    run_git_raw(None, args).map(|_| ())
}

/// Run a git command in a specific directory.
fn run_git_in<S: AsRef<OsStr>>(dir: &Path, args: &[S]) -> Result<(), GitError> {
    run_git_raw(Some(dir), args).map(|_| ())
}

/// Run a git command in a specific directory and return stdout.
fn run_git_output<S: AsRef<OsStr>>(dir: &Path, args: &[S]) -> Result<String, GitError> {
    let output = run_git_raw(Some(dir), args)?;
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// Check if system git is available.
pub fn is_available() -> bool {
    run_git_raw_unchecked(None, &["--version"])
        .map(|output| output.status.success())
        .unwrap_or(false)
}

fn clone_repo_args(url: &str, dest: &str, shallow: bool) -> Vec<String> {
    let mut args = vec![
        "clone".to_string(),
        "--filter".to_string(),
        BLOBLESS_FILTER.to_string(),
    ];
    if shallow {
        args.extend(["--depth".to_string(), "1".to_string()]);
    }
    args.extend(["--".to_string(), url.to_string(), dest.to_string()]);
    args
}

fn clone_bare_repo_args(url: &str, dest: &str) -> Vec<String> {
    vec![
        "clone".to_string(),
        "--bare".to_string(),
        "--filter".to_string(),
        BLOBLESS_FILTER.to_string(),
        "--".to_string(),
        url.to_string(),
        dest.to_string(),
    ]
}

fn clone_from_local_args(local_bare_path: &Path, dest: &Path, no_checkout: bool) -> Vec<String> {
    let src = format!("file://{}", local_bare_path.display());
    let dest_str = dest.display().to_string();
    let mut args = vec![
        "clone".to_string(),
        "--filter".to_string(),
        BLOBLESS_FILTER.to_string(),
    ];
    if no_checkout {
        args.push("--no-checkout".to_string());
    }
    args.extend(["--".to_string(), src, dest_str]);
    args
}

fn fetch_to_bare_args(remote_url: &str) -> Vec<String> {
    vec![
        "fetch".to_string(),
        "--filter".to_string(),
        BLOBLESS_FILTER.to_string(),
        "--force".to_string(),
        remote_url.to_string(),
        "+refs/heads/*:refs/heads/*".to_string(),
    ]
}

/// Clone a git repository to the specified destination.
///
/// - `url`: Git repository URL
/// - `dest`: Destination directory (must not exist)
/// - `shallow`: If true, performs a shallow clone (depth 1)
pub fn clone_repo(url: &str, dest: &Path, shallow: bool) -> Result<(), GitError> {
    validate_remote_url(url)?;
    let dest_str = dest.display().to_string();
    run_git(&clone_repo_args(url, &dest_str, shallow))
}

/// Clone a git repository as a bare repository.
pub fn clone_bare_repo(url: &str, dest: &Path) -> Result<(), GitError> {
    validate_remote_url(url)?;
    let dest_str = dest.display().to_string();
    run_git(&clone_bare_repo_args(url, &dest_str))
}

/// Clone from a local bare repository to a working directory.
pub fn clone_from_local(local_bare_path: &Path, dest: &Path) -> Result<(), GitError> {
    run_git(&clone_from_local_args(local_bare_path, dest, false))
}

/// Clone from a local bare repository without checking out files.
pub fn clone_from_local_no_checkout(local_bare_path: &Path, dest: &Path) -> Result<(), GitError> {
    run_git(&clone_from_local_args(local_bare_path, dest, true))
}

/// Checkout a specific ref (commit SHA, tag, or branch).
pub fn checkout_ref(repo_path: &Path, ref_spec: &str) -> Result<(), GitError> {
    // `ref_spec` is untrusted (from spore manifests). Reject option-like values
    // and pin it as a tree-ish with a trailing `--` so it can never be parsed
    // as a flag or a pathspec.
    reject_option_like(ref_spec, "git ref")?;
    run_git_in(repo_path, &["checkout", ref_spec, "--"])
}

/// Initialize a new git repository at the given path.
pub fn init_repo(path: &Path) -> Result<(), GitError> {
    run_git_in(path, &["init"])
}

/// Configure a blobless promisor remote for lazy blob fetching.
pub fn configure_blobless_promisor_remote(
    repo_path: &Path,
    remote_name: &str,
    remote_url: &str,
) -> Result<(), GitError> {
    reject_option_like(remote_name, "remote name")?;
    validate_remote_url(remote_url)?;
    if get_remote_url(repo_path, remote_name)?.is_some() {
        run_git_in(repo_path, &["remote", "set-url", remote_name, remote_url])?;
    } else {
        run_git_in(repo_path, &["remote", "add", remote_name, remote_url])?;
    }
    let promisor_key = format!("remote.{remote_name}.promisor");
    run_git_in(repo_path, &["config", promisor_key.as_str(), "true"])?;
    let filter_key = format!("remote.{remote_name}.partialclonefilter");
    run_git_in(repo_path, &["config", filter_key.as_str(), BLOBLESS_FILTER])?;
    run_git_in(
        repo_path,
        &["config", "extensions.partialClone", remote_name],
    )
}

/// Configure origin as a blobless promisor remote for lazy blob fetching.
pub fn configure_blobless_origin(repo_path: &Path, remote_url: &str) -> Result<(), GitError> {
    configure_blobless_promisor_remote(repo_path, "origin", remote_url)
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
    reject_option_like(commit_sha, "commit sha")?;
    let output = run_git_raw_unchecked(Some(repo_path), &["cat-file", "-t", commit_sha])?;
    Ok(output.status.success())
}

/// Fetch from a remote URL into a bare repository.
pub fn fetch_to_bare(bare_repo_path: &Path, remote_url: &str) -> Result<(), GitError> {
    validate_remote_url(remote_url)?;
    run_git_in(bare_repo_path, &fetch_to_bare_args(remote_url))
}

/// Fetch from a named remote in the repository.
pub fn fetch_from_remote(repo_path: &Path, remote_name: &str) -> Result<(), GitError> {
    reject_option_like(remote_name, "remote name")?;
    run_git_in(
        repo_path,
        &["fetch", "--filter", BLOBLESS_FILTER, remote_name],
    )
}

/// Add a remote to the repository.
pub fn add_remote(repo_path: &Path, remote_name: &str, remote_url: &str) -> Result<(), GitError> {
    reject_option_like(remote_name, "remote name")?;
    reject_option_like(remote_url, "remote url")?;
    run_git_in(repo_path, &["remote", "add", remote_name, remote_url])
}

/// Set the URL for an existing remote.
pub fn set_remote_url(repo_path: &Path, remote_name: &str, new_url: &str) -> Result<(), GitError> {
    reject_option_like(remote_name, "remote name")?;
    reject_option_like(new_url, "remote url")?;
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

/// Get the latest git commit timestamp for a path, if the path is in a repo.
pub fn last_commit_epoch_ms(repo_path: &Path) -> Option<u64> {
    let output = run_git_output(repo_path, &["log", "-1", "--format=%ct", "--", "."]).ok()?;
    let epoch_s: u64 = output.parse().ok()?;
    Some(epoch_s * 1000)
}

/// Walk a git checkout/cache path without following symlinks and enforce disk limits.
pub fn enforce_size_budget(path: &Path, limits: GitSizeLimits) -> Result<GitSizeStats, GitError> {
    let mut stats = GitSizeStats { bytes: 0, files: 0 };
    let mut stack = vec![path.to_path_buf()];

    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir)? {
            let entry = entry?;
            let path = entry.path();
            let meta = std::fs::symlink_metadata(&path)?;

            stats.files = stats.files.saturating_add(1);
            stats.bytes = stats.bytes.saturating_add(meta.len());
            if stats.files > limits.max_files {
                return Err(GitError::SizeLimit(format!(
                    "{} contains more than {} entries",
                    path.display(),
                    limits.max_files
                )));
            }
            if stats.bytes > limits.max_bytes {
                return Err(GitError::SizeLimit(format!(
                    "{} exceeds {} bytes",
                    path.display(),
                    limits.max_bytes
                )));
            }

            if meta.is_dir() {
                stack.push(path);
            }
        }
    }

    Ok(stats)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn clone_args_use_blobless_filter_for_shallow_and_full() {
        assert_eq!(
            clone_repo_args("https://example.com/repo.git", "/tmp/repo", true),
            [
                "clone",
                "--filter",
                "blob:none",
                "--depth",
                "1",
                "--",
                "https://example.com/repo.git",
                "/tmp/repo",
            ]
            .map(String::from)
            .to_vec()
        );
        assert_eq!(
            clone_repo_args("https://example.com/repo.git", "/tmp/repo", false),
            [
                "clone",
                "--filter",
                "blob:none",
                "--",
                "https://example.com/repo.git",
                "/tmp/repo",
            ]
            .map(String::from)
            .to_vec()
        );
    }

    #[test]
    fn bare_clone_and_fetch_args_use_blobless_filter() {
        assert_eq!(
            clone_bare_repo_args("https://example.com/repo.git", "/tmp/repo.git"),
            [
                "clone",
                "--bare",
                "--filter",
                "blob:none",
                "--",
                "https://example.com/repo.git",
                "/tmp/repo.git",
            ]
            .map(String::from)
            .to_vec()
        );
        assert_eq!(
            fetch_to_bare_args("https://example.com/repo.git"),
            [
                "fetch",
                "--filter",
                "blob:none",
                "--force",
                "https://example.com/repo.git",
                "+refs/heads/*:refs/heads/*",
            ]
            .map(String::from)
            .to_vec()
        );
    }

    #[test]
    fn enforce_size_budget_rejects_too_many_bytes() {
        let dir = tempfile::tempdir().expect("tempdir");
        fs::write(dir.path().join("large.bin"), [0u8; 16]).expect("write");

        let err = enforce_size_budget(dir.path(), GitSizeLimits::new(8, 10)).unwrap_err();
        assert!(matches!(err, GitError::SizeLimit(_)));
    }

    #[test]
    fn enforce_size_budget_rejects_too_many_entries() {
        let dir = tempfile::tempdir().expect("tempdir");
        fs::write(dir.path().join("one.txt"), b"1").expect("write one");
        fs::write(dir.path().join("two.txt"), b"2").expect("write two");

        let err = enforce_size_budget(dir.path(), GitSizeLimits::new(1024, 1)).unwrap_err();
        assert!(matches!(err, GitError::SizeLimit(_)));
    }

    #[test]
    fn remote_url_validation_redacts_userinfo_password() {
        let password = "git-password-canary";
        let err = validate_remote_url(&format!("https://agent:{password}@example.com/repo.git"))
            .unwrap_err()
            .to_string();
        assert!(!err.contains(password));
        assert!(err.contains("***"));
    }

    #[cfg(unix)]
    #[test]
    fn run_program_raw_times_out() {
        let err =
            run_program_raw("sh", None, &["-c", "sleep 2"], Duration::from_millis(20)).unwrap_err();
        assert!(matches!(err, GitError::Timeout { .. }));
    }
}
