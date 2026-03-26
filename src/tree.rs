//! Filesystem traversal that produces `TreeEntry` lists for substrate tree hashing.
//!
//! Implements [`substrate::DirReader`] for real filesystem I/O, delegating
//! filtering decisions (exclude_names, follow_rules) to substrate.

use std::fs;
use std::path::Path;

use anyhow::Result;
use ignore::gitignore::{Gitignore, GitignoreBuilder};
use substrate::{DirReader, TreeEntry};

/// Real filesystem reader with gitignore-style follow_rules support.
pub struct FsReader {
    gitignore: Option<Gitignore>,
}

impl FsReader {
    pub fn new(root_path: &Path, follow_rules: &[String]) -> Self {
        Self {
            gitignore: build_follow_rules(root_path, follow_rules),
        }
    }
}

impl substrate::DirReader for FsReader {
    fn read_dir(&self, path: &Path) -> Result<Vec<substrate::DirEntry>> {
        let mut entries = Vec::new();
        for entry in fs::read_dir(path)? {
            let entry = entry?;
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().to_string();
            let file_type = fs::symlink_metadata(&path)?.file_type();

            // Symlinks and special files are skipped during tree walk.
            // Use check_no_symlinks() before walk/hash to reject symlinks
            // with a clear error (respecting exclude_names and follow_rules).
            if file_type.is_symlink() || (!file_type.is_file() && !file_type.is_dir()) {
                continue;
            }

            entries.push(substrate::DirEntry {
                name,
                is_dir: file_type.is_dir(),
                is_file: file_type.is_file(),
            });
        }
        Ok(entries)
    }

    fn read_file(&self, path: &Path) -> Result<Vec<u8>> {
        Ok(fs::read(path)?)
    }

    fn is_executable(&self, path: &Path) -> Result<bool> {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let metadata = fs::metadata(path)?;
            Ok(metadata.permissions().mode() & 0o111 != 0)
        }

        #[cfg(not(unix))]
        {
            let _ = path;
            Ok(false)
        }
    }

    fn is_ignored(&self, path: &Path, is_dir: bool) -> bool {
        match &self.gitignore {
            Some(gi) => gi.matched_path_or_any_parents(path, is_dir).is_ignore(),
            None => false,
        }
    }

    fn mtime_ms(&self, path: &Path) -> Result<Option<u64>> {
        let meta = fs::metadata(path)?;
        Ok(meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_millis() as u64))
    }
}

fn build_follow_rules(root_path: &Path, follow_rules: &[String]) -> Option<Gitignore> {
    if follow_rules.is_empty() {
        return None;
    }

    let mut builder = GitignoreBuilder::new(root_path);
    let mut found_any = false;

    for rule_file in follow_rules {
        let path = root_path.join(rule_file);
        if path.exists() && builder.add(&path).is_none() {
            found_any = true;
        }
    }

    if !found_any {
        return None;
    }

    builder.build().ok()
}

/// Walk a directory and produce in-memory `TreeEntry` values.
///
/// `exclude_names` — filenames to skip (exact match).
/// `follow_rules` — paths (relative to `dir_path`) of gitignore-style rule files.
pub fn walk_dir(
    dir_path: &Path,
    exclude_names: &[String],
    follow_rules: &[String],
) -> Result<Vec<TreeEntry>> {
    let reader = FsReader::new(dir_path, follow_rules);
    substrate::walk_dir(&reader, dir_path, exclude_names)
}

/// Convenience: walk + compute hash in one call.
pub fn compute_tree_hash(dir_path: &Path, tree: &substrate::SporeTree) -> Result<String> {
    let entries = walk_dir(dir_path, &tree.exclude_names, &tree.follow_rules)?;
    tree.compute_hash(&entries)
}

/// Check that a directory tree contains no symlinks (respecting exclude_names and follow_rules).
///
/// Returns an error listing the first symlink found, with instructions for the user.
/// Call this before `release` to catch symlinks early.
pub fn check_no_symlinks(
    dir_path: &Path,
    exclude_names: &[String],
    follow_rules: &[String],
) -> Result<()> {
    let reader = FsReader::new(dir_path, follow_rules);
    check_no_symlinks_inner(&reader, dir_path, dir_path, exclude_names)
}

fn check_no_symlinks_inner(
    reader: &FsReader,
    root: &Path,
    dir_path: &Path,
    exclude_names: &[String],
) -> Result<()> {
    for entry in fs::read_dir(dir_path)? {
        let entry = entry?;
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();

        if substrate::tree::should_exclude(&name, exclude_names) {
            continue;
        }
        if reader.is_ignored(&path, false) {
            continue;
        }

        let file_type = fs::symlink_metadata(&path)?.file_type();
        if file_type.is_symlink() {
            let target = fs::read_link(&path)
                .map(|t| t.to_string_lossy().into_owned())
                .unwrap_or_else(|_| "?".to_string());
            let relative = path.strip_prefix(root).unwrap_or(&path);
            anyhow::bail!(
                "symlink found: {} → {}\n\
                 Symlinks are not included in spore content.\n  \
                 To include the target content: cp -L \"{0}\" \"{0}.tmp\" && mv \"{0}.tmp\" \"{0}\"\n  \
                 To exclude it: add \"{}\" to exclude_names",
                relative.display(), target, name,
            );
        }
        if file_type.is_dir() {
            check_no_symlinks_inner(reader, root, &path, exclude_names)?;
        }
    }
    Ok(())
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn walk_dir_skips_symlink_entries() {
        use std::os::unix::fs::symlink;

        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        let target = root.join("target.txt");
        let regular = root.join("regular.txt");
        let symlink_path = root.join("linked.txt");

        std::fs::write(&target, "target").unwrap();
        std::fs::write(&regular, "regular").unwrap();
        symlink(&target, &symlink_path).unwrap();

        let entries = walk_dir(root, &[], &[]).unwrap();
        let flat = substrate::flatten_entries(&entries);
        let names: Vec<String> = flat.into_iter().map(|(path, _, _)| path).collect();

        assert!(names.contains(&"regular.txt".to_string()));
        assert!(names.contains(&"target.txt".to_string()));
        assert!(
            !names.contains(&"linked.txt".to_string()),
            "symlink entries must be skipped"
        );
    }

    #[cfg(unix)]
    #[test]
    fn check_no_symlinks_catches_symlink() {
        use std::os::unix::fs::symlink;

        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        std::fs::write(root.join("target.txt"), "target").unwrap();
        symlink("target.txt", root.join("linked.txt")).unwrap();

        let err = check_no_symlinks(root, &[], &[]).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("symlink found"),
            "error should mention symlink: {}",
            msg
        );
        assert!(
            msg.contains("linked.txt"),
            "error should name the file: {}",
            msg
        );
    }

    #[cfg(unix)]
    #[test]
    fn check_no_symlinks_respects_exclude_names() {
        use std::os::unix::fs::symlink;

        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        std::fs::write(root.join("regular.txt"), "data").unwrap();
        symlink("regular.txt", root.join("linked.txt")).unwrap();

        // Excluding the symlink by name should not error
        assert!(check_no_symlinks(root, &["linked.txt".to_string()], &[]).is_ok());
    }
}
