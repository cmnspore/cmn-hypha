//! Archive extraction, file download, and delta decoding helpers.

use crate::cache::CacheDir;

use super::ExtractError;

/// Extract limits for archive decompression.
pub struct ExtractLimits {
    pub max_bytes: u64,
    pub max_files: u64,
    pub max_file_bytes: u64,
    pub reject_path_components: Vec<String>,
}

impl ExtractLimits {
    pub fn from_cache(cache: &CacheDir) -> Self {
        Self {
            max_bytes: cache.spore_max_extract_bytes,
            max_files: cache.spore_max_extract_files,
            max_file_bytes: cache.spore_max_extract_file_bytes,
            reject_path_components: cache.spore_reject_path_components.clone(),
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct DeltaByteBudget {
    pub spore_max_download_bytes: u64,
    pub spore_max_extract_bytes: u64,
}

impl DeltaByteBudget {
    pub fn new(spore_max_download_bytes: u64, limits: &ExtractLimits) -> Self {
        Self {
            spore_max_download_bytes,
            spore_max_extract_bytes: limits.max_bytes,
        }
    }
}

/// Return the first normalized path component rejected by local receive policy.
pub(crate) fn rejected_path_component(
    path: &std::path::Path,
    reject_path_components: &[String],
) -> Option<String> {
    use std::ffi::OsStr;
    use std::path::Component;

    if reject_path_components.is_empty() {
        return None;
    }

    let mut normalized = Vec::new();
    for component in path.components() {
        match component {
            Component::Normal(name) => normalized.push(name.to_os_string()),
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            Component::RootDir | Component::Prefix(_) => normalized.clear(),
        }
    }

    normalized.into_iter().find_map(|component| {
        reject_path_components
            .iter()
            .find(|rejected| component == OsStr::new(rejected.as_str()))
            .cloned()
    })
}

/// Reject a materialized tree if any relative path contains protected components.
pub(crate) fn ensure_no_rejected_path_components(
    root: &std::path::Path,
    reject_path_components: &[String],
) -> Result<(), ExtractError> {
    if reject_path_components.is_empty() {
        return Ok(());
    }

    for entry in walkdir::WalkDir::new(root).min_depth(1).follow_links(false) {
        let entry =
            entry.map_err(|e| ExtractError::Failed(format!("Failed to walk directory: {}", e)))?;
        let relative = entry
            .path()
            .strip_prefix(root)
            .map_err(|e| ExtractError::Failed(format!("Failed to get relative path: {}", e)))?;
        if let Some(component) = rejected_path_component(relative, reject_path_components) {
            return Err(ExtractError::PolicyRejected(format!(
                "received spore content contains protected path component '{}': {}",
                component,
                relative.display()
            )));
        }
    }

    Ok(())
}

/// A writer that fails once `limit` bytes have been written, recording that the
/// limit was hit. This lets callers classify an over-limit download
/// deterministically as malicious rather than parsing error message strings.
pub(super) struct LimitedWriter<W> {
    inner: W,
    limit: u64,
    written: u64,
    pub exceeded: bool,
}

impl<W: std::io::Write> LimitedWriter<W> {
    pub(super) fn new(inner: W, limit: u64) -> Self {
        Self {
            inner,
            limit,
            written: 0,
            exceeded: false,
        }
    }
}

impl<W: std::io::Write> std::io::Write for LimitedWriter<W> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.written = self.written.saturating_add(buf.len() as u64);
        if self.written > self.limit {
            self.exceeded = true;
            return Err(std::io::Error::other("download size limit exceeded"));
        }
        self.inner.write(buf)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.inner.flush()
    }
}

/// Download a file from URL to local path using streaming I/O.
///
/// Streams the response body to a temp file (then persists atomically), keeping
/// memory bounded. The download-size limit is enforced both by the
/// `Content-Length` header and a streaming [`LimitedWriter`], so an over-limit
/// payload is classified as [`ExtractError::Malicious`] deterministically.
pub async fn download_file(
    url: &str,
    dest: &std::path::Path,
    spore_max_download_bytes: u64,
) -> Result<(), ExtractError> {
    let client = substrate::client::http_client(300)
        .map_err(|e| format!("Failed to create HTTP client: {}", e))?;

    let response = client
        .get(url)
        .send()
        .await
        .map_err(|e| format!("Failed to download: {}", e))?;

    if !response.status().is_success() {
        return Err(ExtractError::Failed(format!("HTTP {}", response.status())));
    }

    if let Some(content_length) = response.content_length() {
        if content_length > spore_max_download_bytes {
            return Err(ExtractError::Malicious(format!(
                "Remote payload too large: {} bytes exceeds limit {}",
                content_length, spore_max_download_bytes
            )));
        }
    }

    let parent = dest.parent().ok_or_else(|| {
        ExtractError::Failed("Cannot determine destination directory".to_string())
    })?;
    let mut tmp = tempfile::NamedTempFile::new_in(parent)
        .map_err(|e| format!("Failed to create temp file: {}", e))?;

    let mut limited = LimitedWriter::new(tmp.as_file_mut(), spore_max_download_bytes);
    let result = substrate::client::download_response_to_writer(
        response,
        url,
        u64::MAX,
        &mut limited,
        |_, _| {},
    )
    .await;
    let exceeded = limited.exceeded;

    if let Err(e) = result {
        // NamedTempFile is removed on drop.
        if exceeded {
            return Err(ExtractError::Malicious(format!(
                "Download exceeded size limit of {} bytes",
                spore_max_download_bytes
            )));
        }
        return Err(ExtractError::Failed(e.to_string()));
    }

    tmp.as_file()
        .sync_all()
        .map_err(|e| format!("Failed to sync file: {}", e))?;
    tmp.persist(dest)
        .map_err(|e| ExtractError::Failed(format!("Failed to persist downloaded file: {}", e)))?;

    Ok(())
}

/// Download and extract tarball to a directory
pub async fn download_and_extract_to_dir(
    url: &str,
    dest: &std::path::Path,
    format_hint: Option<&str>,
) -> Result<(), ExtractError> {
    use super::spawn::extract_archive;

    std::fs::create_dir_all(dest)
        .map_err(|e| ExtractError::Failed(format!("Failed to create directory: {}", e)))?;

    let cache = CacheDir::new()
        .map_err(|e| ExtractError::Failed(format!("Failed to load config: {}", e)))?;
    let temp_dir = tempfile::tempdir()
        .map_err(|e| ExtractError::Failed(format!("Failed to create temp directory: {}", e)))?;
    let archive_path = temp_dir.path().join("archive");
    download_file(url, &archive_path, cache.spore_max_download_bytes).await?;

    let limits = ExtractLimits::from_cache(&cache);
    let archive_path_clone = archive_path.clone();
    let dest = dest.to_path_buf();
    let url = url.to_string();
    let format_hint = format_hint.map(|s| s.to_string());
    tokio::task::spawn_blocking(move || {
        extract_archive(
            &archive_path_clone,
            &dest,
            &url,
            format_hint.as_deref(),
            &limits,
        )
    })
    .await
    .map_err(|e| ExtractError::Failed(format!("Extract task failed: {}", e)))??;

    Ok(())
}

pub fn load_old_archive_dictionary(
    old_archive_path: &std::path::Path,
    budget: &DeltaByteBudget,
) -> Result<Vec<u8>, ExtractError> {
    let compressed = std::fs::read(old_archive_path)
        .map_err(|e| ExtractError::Failed(format!("Failed to read old archive: {}", e)))?;
    Ok(substrate::archive::decode_zstd(
        &compressed,
        budget.spore_max_extract_bytes,
    )?)
}

pub fn decode_delta_to_raw_tar_file(
    delta_archive_path: &std::path::Path,
    dict_bytes: &[u8],
    raw_tar_path: &std::path::Path,
    budget: &DeltaByteBudget,
) -> Result<(), ExtractError> {
    let compressed = std::fs::read(delta_archive_path).map_err(|e| {
        ExtractError::Failed(format!("Failed to read downloaded delta archive: {}", e))
    })?;
    let raw_tar = substrate::archive::decode_zstd_with_dict(
        &compressed,
        dict_bytes,
        budget.spore_max_extract_bytes,
    )?;
    std::fs::write(raw_tar_path, &raw_tar)
        .map_err(|e| ExtractError::Failed(format!("Failed to write decoded delta file: {}", e)))?;
    Ok(())
}
