use serde_json::json;
use std::process::ExitCode;

use crate::api::Output;
use crate::site::{self, SiteDir};

use super::{load_existing_mycelium, sign_and_save_mycelium};

pub fn handle_nutrient_add(
    out: &Output,
    domain: &str,
    method_type: &str,
    with_entries: Vec<String>,
    site_path: Option<&str>,
) -> ExitCode {
    let now_epoch_ms = crate::time::now_epoch_ms();

    if site_path.is_none() {
        if let Err(e) = site::validate_site_domain_path(domain) {
            return out.error_hypha(&e);
        }
    }
    let site = SiteDir::from_args(domain, site_path);

    let (mut mycelium, endpoints) = match load_existing_mycelium(&site) {
        Some(v) => v,
        None => {
            return out.error_hint(
                "no_mycelium",
                "No mycelium found",
                Some("run: hypha mycelium root --endpoints-base URL"),
            )
        }
    };

    let mut nutrient = substrate::Nutrient {
        kind: method_type.to_string(),
        address: None,
        recipient: None,
        url: None,
        label: None,
        chain_id: None,
        token: None,
        asset_id: None,
    };
    for entry in &with_entries {
        let Some((key, val_str)) = entry.split_once('=') else {
            return out.error_hint(
                "invalid_args",
                &format!("Invalid --with format: '{}'", entry),
                Some("expected format: KEY=VALUE"),
            );
        };
        let value: serde_json::Value = serde_json::from_str(val_str)
            .unwrap_or_else(|_| serde_json::Value::String(val_str.to_string()));
        match key {
            "address" => nutrient.address = value.as_str().map(|s| s.to_string()),
            "recipient" => nutrient.recipient = value.as_str().map(|s| s.to_string()),
            "url" => nutrient.url = value.as_str().map(|s| s.to_string()),
            "label" => nutrient.label = value.as_str().map(|s| s.to_string()),
            "chain_id" => nutrient.chain_id = value.as_u64(),
            "token" => nutrient.token = value.as_str().map(|s| s.to_string()),
            "asset_id" => nutrient.asset_id = value.as_str().map(|s| s.to_string()),
            _ => {
                return out.error(
                    "invalid_args",
                    &format!("Unknown nutrient field: '{}'. Valid: address, recipient, url, label, chain_id, token, asset_id", key),
                )
            }
        }
    }

    mycelium
        .capsule
        .core
        .nutrients
        .retain(|n| n.kind != method_type);
    mycelium.capsule.core.nutrients.push(nutrient);

    match sign_and_save_mycelium(&site, domain, &mut mycelium, endpoints, now_epoch_ms) {
        Ok(_) => out.ok(json!({
            "domain": domain,
            "action": "nutrient_add",
            "type": method_type,
        })),
        Err(e) => out.error(e.code(), &e.to_string()),
    }
}

pub fn handle_nutrient_remove(
    out: &Output,
    domain: &str,
    method_type: &str,
    site_path: Option<&str>,
) -> ExitCode {
    let now_epoch_ms = crate::time::now_epoch_ms();

    if site_path.is_none() {
        if let Err(e) = site::validate_site_domain_path(domain) {
            return out.error_hypha(&e);
        }
    }
    let site = SiteDir::from_args(domain, site_path);

    let (mut mycelium, endpoints) = match load_existing_mycelium(&site) {
        Some(v) => v,
        None => {
            return out.error_hint(
                "no_mycelium",
                "No mycelium found",
                Some("run: hypha mycelium root --endpoints-base URL"),
            )
        }
    };

    mycelium
        .capsule
        .core
        .nutrients
        .retain(|n| n.kind != method_type);

    match sign_and_save_mycelium(&site, domain, &mut mycelium, endpoints, now_epoch_ms) {
        Ok(_) => out.ok(json!({
            "domain": domain,
            "action": "nutrient_remove",
            "type": method_type,
        })),
        Err(e) => out.error(e.code(), &e.to_string()),
    }
}

pub fn handle_nutrient_clear(out: &Output, domain: &str, site_path: Option<&str>) -> ExitCode {
    let now_epoch_ms = crate::time::now_epoch_ms();

    if site_path.is_none() {
        if let Err(e) = site::validate_site_domain_path(domain) {
            return out.error_hypha(&e);
        }
    }
    let site = SiteDir::from_args(domain, site_path);

    let (mut mycelium, endpoints) = match load_existing_mycelium(&site) {
        Some(v) => v,
        None => {
            return out.error_hint(
                "no_mycelium",
                "No mycelium found",
                Some("run: hypha mycelium root --endpoints-base URL"),
            )
        }
    };

    mycelium.capsule.core.nutrients.clear();

    match sign_and_save_mycelium(&site, domain, &mut mycelium, endpoints, now_epoch_ms) {
        Ok(_) => out.ok(json!({
            "domain": domain,
            "action": "nutrient_clear",
        })),
        Err(e) => out.error(e.code(), &e.to_string()),
    }
}
