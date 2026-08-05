use serde_json::json;
use std::process::ExitCode;

use super::inventory::resolve_public_file_path;
use crate::api::Output;
use crate::site::{self, SiteDir};

const JSON_FETCH_MAX_BYTES: usize = 8 * 1024 * 1024;

fn log_request(
    out: &Output,
    level: &str,
    event: &str,
    message: &str,
    base_url: &str,
    request_path: &str,
) {
    let request_url = format!(
        "{}{}{}",
        base_url,
        if request_path.starts_with('/') {
            ""
        } else {
            "/"
        },
        request_path
    );
    out.log_data(
        level,
        event,
        message,
        json!({
            "request_url": request_url,
        }),
    );
}

pub async fn handle_pulse(
    out: &Output,
    synapse_arg: Option<&str>,
    synapse_token_secret: Option<&str>,
    file_path: &str,
) -> ExitCode {
    let resolved = match crate::config::resolve_synapse(synapse_arg, synapse_token_secret) {
        Ok(r) => r,
        Err(e) => return out.error_hypha(&e),
    };

    let content = match std::fs::read_to_string(file_path) {
        Ok(c) => c,
        Err(e) => {
            return out.error(
                "read_error",
                &format!("Failed to read {}: {}", file_path, e),
            )
        }
    };

    let payload: serde_json::Value = match serde_json::from_str(&content) {
        Ok(p) => p,
        Err(e) => return out.error("parse_error", &format!("Invalid JSON: {}", e)),
    };

    // Validate payload against schema (catches missing fields, wrong types, etc.)
    if let Err(e) = substrate::validate_schema(&payload) {
        return out.error("schema_error", &format!("Schema validation failed: {}", e));
    }

    let uri = payload
        .pointer("/capsule/uri")
        .or_else(|| payload.pointer("/capsules/0/uri"))
        .and_then(|v| v.as_str())
        .unwrap_or(file_path)
        .to_string();

    let base_url = resolved.synapse_url.trim_end_matches('/');

    let client = match substrate::client::http_client(30) {
        Ok(c) => c,
        Err(e) => return out.error("NETWORK_ERR", &format!("HTTP client error: {}", e)),
    };
    let opts = match &resolved.token_secret {
        Some(token_secret) => substrate::client::FetchOptions::with_bearer_token(token_secret)
            .max_bytes(JSON_FETCH_MAX_BYTES),
        None => substrate::client::FetchOptions::with_max_bytes(JSON_FETCH_MAX_BYTES),
    };

    match substrate::client::post_synapse_pulse(&client, base_url, &payload, opts).await {
        Ok(body) => out.ok(json!({
            "cmn_url": uri,
            "synapse_url": base_url,
            "response": body,
        })),
        Err(e) => out.error("synapse_error", &e.to_string()),
    }
}

pub fn handle_serve(
    out: &Output,
    domain: Option<&str>,
    site_path: Option<&str>,
    port: u16,
) -> ExitCode {
    use tiny_http::{Header, Response, Server};

    // Resolve site directory
    if site_path.is_none() {
        if let Some(d) = domain {
            if let Err(e) = site::validate_site_domain_path(d) {
                return out.error_hypha(&e);
            }
        }
    }

    let (site, domain): (SiteDir, String) = if let Some(path) = site_path {
        let d = domain.unwrap_or("localhost").to_string();
        (SiteDir::with_path(std::path::PathBuf::from(path)), d)
    } else if let Some(d) = domain {
        (SiteDir::new(d), d.to_string())
    } else {
        // Try to find the first available site
        let domains = site::list_domains();
        if domains.is_empty() {
            return out.error_hint(
                "NO_SITE",
                "No site found",
                Some("run: hypha mycelium root --domain <DOMAIN>"),
            );
        }
        let d = domains[0].clone();
        (SiteDir::new(&d), d)
    };

    if !site.exists() {
        return out.error(
            "NO_SITE",
            &format!("Site not found at {}", site.root.display()),
        );
    }

    let public_dir = site.public.clone();
    if !public_dir.exists() {
        return out.error(
            "NO_PUBLIC",
            &format!("Public directory not found: {}", public_dir.display()),
        );
    }

    // Local debug server: bind loopback only.
    let addr = format!("127.0.0.1:{}", port);
    let server = match Server::http(&addr) {
        Ok(s) => s,
        Err(e) => return out.error("server_error", &format!("Failed to start server: {}", e)),
    };

    // Output server info (JSON mode outputs to stdout, then logs go to stderr)
    let base_url = format!("http://127.0.0.1:{}", port);
    let ep = SiteDir::endpoints(&base_url);
    let mycelium_url = ep
        .iter()
        .find(|endpoint| endpoint.kind == "mycelium")
        .map(|endpoint| endpoint.url.clone());
    let spore_url = ep
        .iter()
        .find(|endpoint| endpoint.kind == "spore")
        .map(|endpoint| endpoint.url.clone());
    let archive_urls: Vec<_> = ep
        .iter()
        .filter(|endpoint| endpoint.kind == "archive")
        .map(|endpoint| {
            json!({
                "archive_url": endpoint.url,
            })
        })
        .collect();
    let data = json!({
        "status": "running",
        "domain": domain,
        "public_dir": public_dir.display().to_string(),
        "listen_addr": format!("127.0.0.1:{}", port),
        "base_url": base_url,
        "endpoints": {
            "cmn_url": format!("{}/.well-known/cmn.json", base_url),
            "mycelium_url": mycelium_url,
            "spore_url": spore_url,
            "archives": archive_urls,
        }
    });

    // A server-start event is non-terminal; the terminal result is emitted only
    // if the request iterator later shuts down.
    out.log_data("info", "server_started", "Mycelium server started", data);

    // Canonical public root used to verify that every served file is actually
    // inside public/ (defeats symlink escapes that a lexical check would miss).
    let canonical_public = std::fs::canonicalize(&public_dir).unwrap_or(public_dir.clone());

    // Serve requests
    for request in server.incoming_requests() {
        let request_url = request.url().to_string();
        let file_path = match resolve_public_file_path(&public_dir, &request_url) {
            Some(path) => path,
            None => {
                log_request(
                    out,
                    "warn",
                    "http_forbidden",
                    "HTTP request path is invalid",
                    &base_url,
                    &request_url,
                );
                let response = Response::from_string("Forbidden").with_status_code(403);
                let _ = request.respond(response);
                continue;
            }
        };

        // Resolve symlinks before serving; a missing target → 404.
        let canonical = match std::fs::canonicalize(&file_path) {
            Ok(c) => c,
            Err(_) => {
                log_request(
                    out,
                    "warn",
                    "http_not_found",
                    "HTTP request target was not found",
                    &base_url,
                    &request_url,
                );
                let response = Response::from_string("Not Found").with_status_code(404);
                let _ = request.respond(response);
                continue;
            }
        };

        if !canonical.starts_with(&canonical_public) {
            log_request(
                out,
                "warn",
                "http_forbidden",
                "HTTP request attempted a path escape",
                &base_url,
                &request_url,
            );
            let response = Response::from_string("Forbidden").with_status_code(403);
            let _ = request.respond(response);
            continue;
        }

        if !canonical.is_file() {
            log_request(
                out,
                "warn",
                "http_not_found",
                "HTTP request target was not a file",
                &base_url,
                &request_url,
            );
            let response = Response::from_string("Not Found").with_status_code(404);
            let _ = request.respond(response);
            continue;
        }

        match std::fs::File::open(&canonical) {
            Ok(file) => {
                let content_type = match canonical.extension().and_then(std::ffi::OsStr::to_str) {
                    Some("json") => "application/json",
                    Some("html") => "text/html",
                    Some("css") => "text/css",
                    Some("js") => "application/javascript",
                    Some("gz") => "application/gzip",
                    _ => "application/octet-stream",
                };

                // Stream the file instead of buffering it entirely in memory.
                let mut response = Response::from_file(file);
                if let Ok(h) = Header::from_bytes(&b"Content-Type"[..], content_type.as_bytes()) {
                    response = response.with_header(h);
                }
                // Prevent browsers from MIME-sniffing the octet-stream fallback.
                if let Ok(h) = Header::from_bytes(&b"X-Content-Type-Options"[..], &b"nosniff"[..]) {
                    response = response.with_header(h);
                }

                log_request(
                    out,
                    "info",
                    "http_ok",
                    "HTTP request served",
                    &base_url,
                    &request_url,
                );
                let _ = request.respond(response);
            }
            Err(_) => {
                log_request(
                    out,
                    "warn",
                    "http_not_found",
                    "HTTP request target could not be opened",
                    &base_url,
                    &request_url,
                );
                let response = Response::from_string("Not Found").with_status_code(404);
                let _ = request.respond(response);
            }
        }
    }

    out.ok(json!({
        "status": "stopped",
        "base_url": base_url,
    }))
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn request_url_field_redacts_secret_query_values() {
        let event = agent_first_data::json_log(json!({
            "level": "info",
            "message": "request",
            "request_url": "http://127.0.0.1/spore?token_secret=canary",
        }))
        .build()
        .into_value();
        let rendered = agent_first_data::render(
            &event,
            agent_first_data::OutputFormat::Json,
            &agent_first_data::OutputOptions::default(),
        );
        assert!(!rendered.contains("canary"));
        assert!(rendered.contains("***"));
    }
}
