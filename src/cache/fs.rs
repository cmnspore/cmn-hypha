use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

/// Process-wide counter ensuring temp file names are unique even within a
/// single millisecond / same PID.
static TMP_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Run `f` while holding an exclusive advisory lock on `parent/.lock`.
fn with_dir_lock<T>(
    parent: &Path,
    f: impl FnOnce() -> Result<T, crate::sink::HyphaError>,
) -> Result<T, crate::sink::HyphaError> {
    use crate::sink::HyphaError;
    use fs2::FileExt;

    std::fs::create_dir_all(parent).map_err(|e| {
        HyphaError::new(
            "cache_write_failed",
            format!("Failed to create directory: {}", e),
        )
    })?;

    let lock_path = parent.join(".lock");
    let lock_file = std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(&lock_path)
        .map_err(|e| {
            HyphaError::new(
                "cache_write_failed",
                format!("Failed to open lock file: {}", e),
            )
        })?;

    lock_file.lock_exclusive().map_err(|e| {
        HyphaError::new(
            "cache_write_failed",
            format!("Failed to acquire lock: {}", e),
        )
    })?;

    let result = f();

    let _ = lock_file.unlock();
    result
}

pub(super) fn locked_write_file(path: &Path, content: &str) -> Result<(), crate::sink::HyphaError> {
    use crate::sink::HyphaError;

    let parent = path.parent().ok_or_else(|| {
        HyphaError::new("cache_write_failed", "Cannot determine parent directory")
    })?;
    with_dir_lock(parent, || atomic_write_file(path, content))
}

/// Read-modify-write `path` atomically while holding the directory lock, so
/// concurrent updaters can't lose each other's changes. `update` receives the
/// current file contents (if any) and returns the new contents.
pub(super) fn locked_update_file(
    path: &Path,
    update: impl FnOnce(Option<String>) -> Result<String, crate::sink::HyphaError>,
) -> Result<(), crate::sink::HyphaError> {
    use crate::sink::HyphaError;

    let parent = path.parent().ok_or_else(|| {
        HyphaError::new("cache_write_failed", "Cannot determine parent directory")
    })?;
    with_dir_lock(parent, || {
        let existing = std::fs::read_to_string(path).ok();
        let content = update(existing)?;
        atomic_write_file(path, &content)
    })
}

/// Write content to a file atomically: write to a temp file in the same directory,
/// then rename to the final path. This prevents partial/corrupt reads on crash.
fn atomic_write_file(path: &Path, content: &str) -> Result<(), crate::sink::HyphaError> {
    use crate::sink::HyphaError;
    use std::io::Write;

    let parent = path.parent().ok_or_else(|| {
        HyphaError::new("cache_write_failed", "Cannot determine parent directory")
    })?;
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("tmp");

    // Unique per (pid, process-wide counter) so concurrent writers in the same
    // directory never collide; `create_new(true)` turns any residual collision
    // into an error instead of silently truncating another writer's temp file.
    let tmp_path = parent.join(format!(
        ".{}.tmp.{}.{}",
        file_name,
        std::process::id(),
        TMP_COUNTER.fetch_add(1, Ordering::Relaxed)
    ));

    let mut f = std::fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&tmp_path)
        .map_err(|e| {
            HyphaError::new(
                "cache_write_failed",
                format!("Failed to create temp file: {}", e),
            )
        })?;
    f.write_all(content.as_bytes()).map_err(|e| {
        let _ = std::fs::remove_file(&tmp_path);
        HyphaError::new(
            "cache_write_failed",
            format!("Failed to write temp file: {}", e),
        )
    })?;
    f.sync_all().map_err(|e| {
        let _ = std::fs::remove_file(&tmp_path);
        HyphaError::new(
            "cache_write_failed",
            format!("Failed to sync temp file: {}", e),
        )
    })?;
    drop(f);

    std::fs::rename(&tmp_path, path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp_path);
        HyphaError::new(
            "cache_write_failed",
            format!("Failed to rename temp file: {}", e),
        )
    })?;

    // fsync the directory so the rename itself survives a crash.
    sync_parent_dir(parent);
    Ok(())
}

/// Best-effort fsync of a directory so a just-completed rename is durable.
fn sync_parent_dir(dir: &Path) {
    if let Ok(handle) = std::fs::File::open(dir) {
        let _ = handle.sync_all();
    }
}

pub(super) fn read_spore_metadata(manifest_path: &Path) -> (String, String) {
    if manifest_path.exists() {
        if let Ok(content) = std::fs::read_to_string(manifest_path) {
            if let Ok(manifest) = serde_json::from_str::<substrate::Spore>(&content) {
                return (manifest.capsule.core.name, manifest.capsule.core.synopsis);
            }
        }
    }
    ("unknown".to_string(), String::new())
}

pub(super) fn dir_size(path: &Path) -> u64 {
    let mut size = 0;
    let mut stack = vec![path.to_path_buf()];
    while let Some(dir) = stack.pop() {
        if let Ok(entries) = std::fs::read_dir(&dir) {
            for entry in entries.filter_map(|e| e.ok()) {
                let path = entry.path();
                if path.is_file() {
                    size += entry.metadata().map(|m| m.len()).unwrap_or(0);
                } else if path.is_dir() {
                    stack.push(path);
                }
            }
        }
    }
    size
}
