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
    crate::git::last_commit_epoch_ms(path)
}
