use anyhow::Result;
use std::path::Path;

pub fn compute_updated_at_ms(
    root_path: &Path,
    exclude_names: &[String],
    follow_rules: &[String],
) -> Result<u64> {
    if let Some(ms) = git_last_commit_ms(root_path) {
        return Ok(ms);
    }

    let reader = crate::tree::FsReader::new(root_path, follow_rules);
    substrate::max_mtime(&reader, root_path, exclude_names)
}

fn git_last_commit_ms(path: &Path) -> Option<u64> {
    let output = std::process::Command::new("git")
        .args(["log", "-1", "--format=%ct", "--", "."])
        .current_dir(path)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let s = std::str::from_utf8(&output.stdout).ok()?.trim();
    let epoch_s: u64 = s.parse().ok()?;
    Some(epoch_s * 1000)
}
