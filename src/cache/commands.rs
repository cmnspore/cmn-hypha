use std::process::ExitCode;

use serde_json::json;
use substrate::CmnUri;

use crate::api::Output;

use super::CacheDir;

/// Handle the `cache list` command
pub fn handle_list(out: &Output) -> ExitCode {
    let cache = match CacheDir::new() {
        Ok(cache) => cache,
        Err(e) => return out.error_hypha(&e),
    };
    let spores = cache.list_all();

    if spores.is_empty() {
        let data = json!({
            "spore_count": 0,
            "spores": [],
            "total_size_bytes": 0,
        });

        return out.ok(data);
    }

    let total_size_bytes: u64 = spores.iter().map(|s| s.size_bytes).sum();

    let spores_json: Vec<serde_json::Value> = spores
        .iter()
        .map(|s| {
            json!({
                "domain": s.domain,
                "hash": s.hash,
                "name": s.name,
                "synopsis": s.synopsis,
                "path": s.path.display().to_string(),
                "size_bytes": s.size_bytes,
                "verdict": s.verdict,
            })
        })
        .collect();

    let data = json!({
        "spore_count": spores.len(),
        "spores": spores_json,
        "total_size_bytes": total_size_bytes,
    });

    out.ok(data)
}

/// Handle the `cache clean` command
pub fn handle_clean(out: &Output, all: bool) -> ExitCode {
    let cache = match CacheDir::new() {
        Ok(cache) => cache,
        Err(e) => return out.error_hypha(&e),
    };

    if all {
        match cache.clean_all() {
            Ok(count) => {
                let data = json!({
                    "removed_count": count,
                });
                out.ok(data)
            }
            Err(e) => out.error_hypha(&e),
        }
    } else {
        out.error(
            "invalid_args",
            "Use --all to remove all cached items. Age-based cleanup not yet implemented.",
        )
    }
}

/// Handle the `cache path` command
pub fn handle_path(out: &Output, uri_str: &str) -> ExitCode {
    let uri = match CmnUri::parse(uri_str) {
        Ok(u) => u,
        Err(e) => return out.error("uri_error", &e),
    };

    let hash = match &uri.hash {
        Some(h) => h,
        None => return out.error("uri_error", "spore URI must include a hash"),
    };

    let cache = match CacheDir::new() {
        Ok(cache) => cache,
        Err(e) => return out.error_hypha(&e),
    };
    let path = cache.spore_path(&uri.domain, hash);

    if !path.exists() {
        return out.error_hint(
            "NOT_CACHED",
            "Spore not cached",
            Some(&format!("run: hypha taste {}", uri_str)),
        );
    }

    let content_path = path.join("content");
    let display_path = if content_path.exists() {
        content_path
    } else {
        path.clone()
    };

    let data = json!({
        "cmn_url": uri_str,
        "path": display_path.display().to_string(),
    });

    out.ok(data)
}
