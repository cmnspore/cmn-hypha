use std::path::Path;

/// Run `f` while holding an exclusive advisory lock on `parent/.lock`.
pub(crate) fn with_dir_lock<T>(
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
    with_dir_lock(parent, || {
        atomic_write_file(path, content.as_bytes(), parent, None)
            .map_err(|e| HyphaError::new("cache_write_failed", e.to_string()))
    })
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
        atomic_write_file(path, content.as_bytes(), parent, None)
            .map_err(|e| HyphaError::new("cache_write_failed", e.to_string()))
    })
}

/// Write through a temporary file on the target filesystem, preserving permissions.
/// Source-tree callers put temporary files under `.cmn` so interrupted writes
/// cannot add files to a later release tree. Symbolic-link targets are rejected.
pub(crate) fn atomic_write_file(
    path: &Path,
    content: &[u8],
    temp_dir: &Path,
    create_mode: Option<u32>,
) -> std::io::Result<()> {
    use std::io::Write;
    atomic_write_with(path, temp_dir, create_mode, |file| file.write_all(content))
}

fn atomic_write_with(
    path: &Path,
    temp_dir: &Path,
    create_mode: Option<u32>,
    write: impl FnOnce(&mut std::fs::File) -> std::io::Result<()>,
) -> std::io::Result<()> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let permissions = match std::fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("Refusing to replace symbolic link: {}", path.display()),
            ));
        }
        Ok(metadata) if metadata.is_file() => Some(metadata.permissions()),
        Ok(_) => {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("Target is not a regular file: {}", path.display()),
            ));
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => return Err(e),
    };
    std::fs::create_dir_all(parent)?;
    std::fs::create_dir_all(temp_dir)?;
    let mut temporary = tempfile::Builder::new()
        .prefix(".hypha-write-")
        .tempfile_in(temp_dir)?;
    if let Some(permissions) = permissions {
        temporary.as_file().set_permissions(permissions)?;
    } else {
        #[cfg(unix)]
        if let Some(mode) = create_mode {
            use std::os::unix::fs::PermissionsExt;
            temporary
                .as_file()
                .set_permissions(std::fs::Permissions::from_mode(mode))?;
        }
        #[cfg(not(unix))]
        let _ = create_mode;
    }
    write(temporary.as_file_mut())?;
    temporary.as_file().sync_all()?;
    temporary.persist(path).map_err(|error| error.error)?;
    sync_parent_dir(parent);
    if temp_dir != parent {
        sync_parent_dir(temp_dir);
    }
    Ok(())
}

/// Best-effort fsync of a directory so a just-completed rename is durable.
fn sync_parent_dir(dir: &Path) {
    if let Ok(handle) = std::fs::File::open(dir) {
        let _ = handle.sync_all();
    }
}

pub(super) fn read_spore_metadata(manifest_path: &Path) -> (String, String) {
    if manifest_path.exists()
        && let Ok(content) = std::fs::read_to_string(manifest_path)
        && let Ok(manifest) = serde_json::from_str::<substrate::Spore>(&content)
    {
        return (manifest.capsule.core.name, manifest.capsule.core.synopsis);
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

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn partial_write_keeps_original_and_removes_temporary_file() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("draft.json");
        let staging = root.path().join(".cmn");
        std::fs::write(&path, "original").unwrap();

        let error = atomic_write_with(&path, &staging, None, |file| {
            file.write_all(b"partial")?;
            Err(std::io::Error::other("injected write failure"))
        })
        .unwrap_err();

        assert!(error.to_string().contains("injected"));
        assert_eq!(std::fs::read(&path).unwrap(), b"original");
        assert_eq!(std::fs::read_dir(staging).unwrap().count(), 0);
    }

    #[test]
    fn failed_private_key_write_can_be_retried_by_init() {
        let root = tempfile::tempdir().unwrap();
        let site = crate::site::SiteDir::with_path(root.path().to_path_buf());
        let result = atomic_write_with(
            &site.private_key_path(),
            &site.keys.join(".cmn"),
            Some(0o600),
            |file| {
                file.write_all(b"partial PEM")?;
                Err(std::io::Error::other("injected private key write failure"))
            },
        );

        assert!(result.is_err());
        assert!(!site.private_key_path().exists());
        let identity = crate::auth::init_identity_with_site("example.com", &site).unwrap();
        assert!(identity.newly_created);
        assert_eq!(
            crate::auth::get_identity_with_site("example.com", &site)
                .unwrap()
                .public_key,
            identity.public_key
        );
    }

    #[cfg(unix)]
    #[test]
    fn atomic_write_preserves_existing_mode_and_rejects_symlink() {
        use std::os::unix::fs::{PermissionsExt, symlink};

        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("draft.json");
        let staging = root.path().join(".cmn");
        std::fs::write(&path, "original").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o640)).unwrap();
        atomic_write_file(&path, b"complete", &staging, Some(0o644)).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"complete");
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o640
        );

        let link = root.path().join("linked.json");
        symlink(&path, &link).unwrap();
        assert!(atomic_write_file(&link, b"replacement", &staging, None).is_err());
        assert!(
            std::fs::symlink_metadata(&link)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert_eq!(std::fs::read(&path).unwrap(), b"complete");
    }
}
