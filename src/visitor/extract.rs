//! Archive extraction, file download, and delta decoding helpers.

use crate::cache::CacheDir;

use super::ExtractError;

/// Extract limits for archive decompression.
pub struct ExtractLimits {
    pub max_bytes: u64,
    pub max_files: u64,
    pub max_file_bytes: u64,
}

impl ExtractLimits {
    pub fn from_cache(cache: &CacheDir) -> Self {
        Self {
            max_bytes: cache.max_extract_bytes,
            max_files: cache.max_extract_files,
            max_file_bytes: cache.max_extract_file_bytes,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct DeltaByteBudget {
    pub max_download_bytes: u64,
    pub max_extract_bytes: u64,
}

impl DeltaByteBudget {
    pub fn new(max_download_bytes: u64, limits: &ExtractLimits) -> Self {
        Self {
            max_download_bytes,
            max_extract_bytes: limits.max_bytes,
        }
    }
}

/// Download a file from URL to local path
pub async fn download_file(
    url: &str,
    dest: &std::path::Path,
    max_download_bytes: u64,
) -> Result<(), ExtractError> {
    use std::io::Write;

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

    if let Some(cl) = response.content_length() {
        if cl > max_download_bytes {
            return Err(ExtractError::Malicious(format!(
                "Response too large: {} bytes exceeds max_download_bytes ({})",
                cl, max_download_bytes
            )));
        }
    }

    let bytes = response
        .bytes()
        .await
        .map_err(|e| format!("Failed to read response: {}", e))?;
    if bytes.len() as u64 > max_download_bytes {
        return Err(ExtractError::Malicious(format!(
            "Download exceeds max_download_bytes ({})",
            max_download_bytes
        )));
    }

    let dest = dest.to_path_buf();
    let write_result = tokio::task::spawn_blocking(move || {
        let mut out =
            std::fs::File::create(&dest).map_err(|e| format!("Failed to create file: {}", e))?;
        out.write_all(&bytes)
            .map_err(|e| format!("Failed to write file: {}", e))?;
        out.sync_all()
            .map_err(|e| format!("Failed to sync file: {}", e))?;
        Ok::<(), String>(())
    })
    .await
    .map_err(|e| format!("Download task failed: {}", e))?;
    write_result?;

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

    let cache = CacheDir::new();
    let temp_dir = tempfile::tempdir()
        .map_err(|e| ExtractError::Failed(format!("Failed to create temp directory: {}", e)))?;
    let archive_path = temp_dir.path().join("archive");
    download_file(url, &archive_path, cache.max_download_bytes).await?;

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
        budget.max_extract_bytes,
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
        budget.max_extract_bytes,
    )?;
    std::fs::write(raw_tar_path, &raw_tar)
        .map_err(|e| ExtractError::Failed(format!("Failed to write decoded delta file: {}", e)))?;
    Ok(())
}
