//! Filesystem traversal that produces `TreeEntry` lists for substrate tree hashing.
//!
//! Implements [`substrate::DirReader`] for real filesystem I/O, delegating
//! filtering decisions (exclude_names, follow_rules) to substrate.

use std::fs;
use std::path::Path;

use anyhow::Result;
use ignore::gitignore::{Gitignore, GitignoreBuilder};
use ignore::Match;
use substrate::{DirReader, TreeEntry};

/// Real filesystem reader with hierarchical gitignore-style follow_rules support.
///
/// `follow_rules` names ignore files (e.g. `.gitignore`) that are honored at
/// EVERY directory level, matching git: a `.gitignore` in a subdirectory applies
/// to that subtree, and a deeper file overrides a shallower one. The matchers are
/// discovered by walking the tree once here, skipping `exclude_names` directories,
/// symlinks, and any subtree already ignored by a shallower rule (so large
/// build-artifact trees like `node_modules` are never descended into).
pub struct FsReader {
    /// One matcher per directory that holds a follow-rule file, ordered
    /// shallowest-first so deeper rules take precedence.
    gitignores: Vec<Gitignore>,
}

impl FsReader {
    pub fn new(root_path: &Path, exclude_names: &[String], follow_rules: &[String]) -> Self {
        let mut gitignores = Vec::new();
        if !follow_rules.is_empty() {
            discover_follow_rules(root_path, exclude_names, follow_rules, &mut gitignores);
        }
        Self { gitignores }
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
        path_is_ignored(&self.gitignores, path, is_dir)
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

/// Evaluate `path` against every ancestor directory's matcher, shallowest-first,
/// so a deeper `.gitignore` overrides a shallower one (git semantics: the last
/// matching rule, from the closest file, decides).
fn path_is_ignored(gitignores: &[Gitignore], path: &Path, is_dir: bool) -> bool {
    let mut ignored = false;
    for gi in gitignores {
        if path.starts_with(gi.path()) {
            match gi.matched_path_or_any_parents(path, is_dir) {
                Match::Ignore(_) => ignored = true,
                Match::Whitelist(_) => ignored = false,
                Match::None => {}
            }
        }
    }
    ignored
}

/// Walk `dir` (pre-order) collecting one gitignore matcher per directory that
/// holds a follow-rule file, so the rules are honored hierarchically like git.
/// Skips `exclude_names` directories, symlinks, and subtrees already ignored by a
/// shallower rule — the latter means a build-artifact tree (e.g. `node_modules`,
/// once its parent `.gitignore` is seen) is never descended into.
fn discover_follow_rules(
    dir: &Path,
    exclude_names: &[String],
    follow_rules: &[String],
    acc: &mut Vec<Gitignore>,
) {
    let mut builder = GitignoreBuilder::new(dir);
    let mut found = false;
    for rule_file in follow_rules {
        let path = dir.join(rule_file);
        if path.is_file() && builder.add(&path).is_none() {
            found = true;
        }
    }
    if found {
        if let Ok(gi) = builder.build() {
            acc.push(gi);
        }
    }

    let read = match fs::read_dir(dir) {
        Ok(read) => read,
        Err(_) => return,
    };
    for entry in read.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if substrate::tree::should_exclude(&name, exclude_names) {
            continue;
        }
        let path = entry.path();
        let Ok(file_type) = fs::symlink_metadata(&path).map(|m| m.file_type()) else {
            continue;
        };
        if !file_type.is_dir() || file_type.is_symlink() {
            continue;
        }
        if path_is_ignored(acc, &path, true) {
            continue;
        }
        discover_follow_rules(&path, exclude_names, follow_rules, acc);
    }
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
    let reader = FsReader::new(dir_path, exclude_names, follow_rules);
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
    let reader = FsReader::new(dir_path, exclude_names, follow_rules);
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

    #[cfg(unix)]
    #[test]
    fn check_no_symlinks_honors_nested_gitignore() {
        use std::os::unix::fs::symlink;

        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        // A per-language subdir whose own .gitignore excludes node_modules — the
        // exact shape that broke afdata packaging (typescript/.gitignore holds
        // `node_modules/`, and node_modules/.bin/tsx is a symlink).
        let sub = root.join("typescript");
        std::fs::create_dir(&sub).unwrap();
        std::fs::write(sub.join(".gitignore"), "node_modules/\n").unwrap();
        let bin = sub.join("node_modules").join(".bin");
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::write(bin.join("target"), "x").unwrap();
        symlink("target", bin.join("tsx")).unwrap();

        // follow_rules=[".gitignore"] must honor the nested file, so the ignored
        // node_modules is never descended and its symlink never flagged.
        assert!(check_no_symlinks(root, &[], &[".gitignore".to_string()]).is_ok());
    }

    #[test]
    fn walk_dir_honors_nested_gitignore() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        let sub = root.join("pkg");
        std::fs::create_dir(&sub).unwrap();
        std::fs::write(sub.join(".gitignore"), "build/\n").unwrap();
        std::fs::write(sub.join("keep.txt"), "keep").unwrap();
        std::fs::create_dir(sub.join("build")).unwrap();
        std::fs::write(sub.join("build").join("out.o"), "obj").unwrap();

        let entries = walk_dir(root, &[], &[".gitignore".to_string()]).unwrap();
        let names: Vec<String> = substrate::flatten_entries(&entries)
            .into_iter()
            .map(|(path, _, _)| path)
            .collect();

        assert!(
            names.iter().any(|n| n.ends_with("keep.txt")),
            "tracked sibling must survive: {names:?}"
        );
        assert!(
            !names.iter().any(|n| n.contains("build")),
            "nested-gitignored dir must be excluded: {names:?}"
        );
    }

    #[test]
    fn deeper_gitignore_overrides_shallower() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        // Root ignores *.log; a subdirectory whitelists them back — a deeper
        // rule must override the shallower one, like git.
        std::fs::write(root.join(".gitignore"), "*.log\n").unwrap();
        let sub = root.join("keep");
        std::fs::create_dir(&sub).unwrap();
        std::fs::write(sub.join(".gitignore"), "!*.log\n").unwrap();
        std::fs::write(root.join("root.log"), "r").unwrap();
        std::fs::write(sub.join("kept.log"), "k").unwrap();

        let entries = walk_dir(root, &[], &[".gitignore".to_string()]).unwrap();
        let names: Vec<String> = substrate::flatten_entries(&entries)
            .into_iter()
            .map(|(path, _, _)| path)
            .collect();

        assert!(
            !names.iter().any(|n| n.ends_with("root.log")),
            "root-level *.log stays ignored: {names:?}"
        );
        assert!(
            names.iter().any(|n| n.ends_with("kept.log")),
            "deeper !*.log re-includes it: {names:?}"
        );
    }
}
