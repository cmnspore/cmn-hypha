use serde_json::json;
use std::process::ExitCode;

use super::inventory::resolve_public_file_path;
use crate::api::Output;
use crate::site::{self, SiteDir};

pub async fn handle_pulse(
    out: &Output,
    synapse_arg: Option<&str>,
    synapse_token_secret: Option<&str>,
    file_path: &str,
) -> ExitCode {
    let resolved = match crate::config::resolve_synapse(synapse_arg, synapse_token_secret) {
        Ok(r) => r,
        Err(e) => return out.error("synapse_error", &e),
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

    let base_url = resolved.url.trim_end_matches('/');

    let client = match substrate::client::http_client(30) {
        Ok(c) => c,
        Err(e) => return out.error("NETWORK_ERR", &format!("HTTP client error: {}", e)),
    };
    let opts = match &resolved.token_secret {
        Some(token) => substrate::client::FetchOptions::with_bearer_token(token),
        None => Default::default(),
    };

    match substrate::client::post_synapse_pulse(&client, base_url, &payload, opts).await {
        Ok(body) => out.ok(json!({
            "uri": uri,
            "synapse": base_url,
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
    use std::io::Read;
    use tiny_http::{Header, Response, Server};

    // Resolve site directory
    if site_path.is_none() {
        if let Some(d) = domain {
            if let Err(e) = site::validate_site_domain_path(d) {
                return out.error("invalid_domain", &e);
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
        .map(|endpoint| endpoint.url.clone())
        .collect();
    let data = json!({
        "status": "running",
        "domain": domain,
        "public_dir": public_dir.display().to_string(),
        "listen_addr": format!("127.0.0.1:{}", port),
        "base_url": base_url,
        "endpoints": {
            "cmn": format!("{}/.well-known/cmn.json", base_url),
            "mycelium": mycelium_url,
            "spore": spore_url,
            "archive": archive_urls,
        }
    });

    // Output startup info (both modes use out.ok for consistent formatting)
    // Note: ok() returns ExitCode but we continue serving, so ignore it
    let _ = out.ok(&data);

    // Serve requests
    for request in server.incoming_requests() {
        let request_url = request.url().to_string();
        let file_path = match resolve_public_file_path(&public_dir, &request_url) {
            Some(path) => path,
            None => {
                out.warn(
                    "HTTP_FORBIDDEN",
                    &format!("GET {} (invalid path)", request_url),
                );
                let response = Response::from_string("Forbidden").with_status_code(403);
                let _ = request.respond(response);
                continue;
            }
        };

        let url_path = request_url
            .split('?')
            .next()
            .unwrap_or_default()
            .trim_start_matches('/');

        if !file_path.starts_with(&public_dir) {
            out.warn(
                "HTTP_FORBIDDEN",
                &format!("GET {} (path escape)", request_url),
            );
            let response = Response::from_string("Forbidden").with_status_code(403);
            let _ = request.respond(response);
            continue;
        }

        // Try to serve the file
        if file_path.is_file() {
            match std::fs::File::open(&file_path) {
                Ok(mut file) => {
                    let mut content = Vec::new();
                    if file.read_to_end(&mut content).is_ok() {
                        // Determine content type
                        let content_type =
                            match file_path.extension().and_then(std::ffi::OsStr::to_str) {
                                Some("json") => "application/json",
                                Some("html") => "text/html",
                                Some("css") => "text/css",
                                Some("js") => "application/javascript",
                                Some("gz") => "application/gzip",
                                _ => "application/octet-stream",
                            };

                        let header =
                            Header::from_bytes(&b"Content-Type"[..], content_type.as_bytes());

                        let mut response = Response::from_data(content);
                        if let Ok(h) = header {
                            response = response.with_header(h);
                        }

                        // Add CORS header for debugging
                        if let Ok(cors) =
                            Header::from_bytes(&b"Access-Control-Allow-Origin"[..], &b"*"[..])
                        {
                            response = response.with_header(cors);
                        }

                        out.warn("HTTP_OK", &format!("GET /{}", url_path));
                        let _ = request.respond(response);
                    } else {
                        out.warn("HTTP_ERROR", &format!("GET /{} (read error)", url_path));
                        let response =
                            Response::from_string("Internal Server Error").with_status_code(500);
                        let _ = request.respond(response);
                    }
                }
                Err(_) => {
                    out.warn("HTTP_NOT_FOUND", &format!("GET /{}", url_path));
                    let response = Response::from_string("Not Found").with_status_code(404);
                    let _ = request.respond(response);
                }
            }
        } else {
            out.warn("HTTP_NOT_FOUND", &format!("GET /{}", url_path));
            let response = Response::from_string("Not Found").with_status_code(404);
            let _ = request.respond(response);
        }
    }

    ExitCode::SUCCESS
}
