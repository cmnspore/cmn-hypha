use super::*;

/// Absorb source info for tracking
#[derive(Serialize)]
struct AbsorbSource {
    uri: String,
    hash: String,
    name: String,
    path: String,
}

/// Prepare spores for AI-assisted merge — library level.
pub async fn absorb(
    uris: Vec<String>,
    use_discover: bool,
    synapse_arg: Option<&str>,
    synapse_token_secret: Option<&str>,
    max_depth: u32,
    sink: &dyn crate::EventSink,
) -> Result<crate::output::AbsorbOutput, crate::HyphaError> {
    // If discover flag is set, auto-discover from Synapse
    let uris_to_absorb = if use_discover {
        let resolved =
            crate::config::resolve_synapse(synapse_arg, synapse_token_secret).map_err(|e| {
                crate::HyphaError::new(
                    "absorb_error",
                    format!("Synapse required with --discover: {}", e),
                )
            })?;

        let spore_core_path = std::path::Path::new("spore.core.json");
        if !spore_core_path.exists() {
            return Err(crate::HyphaError::new(
                "absorb_error",
                "spore.core.json not found. Run from spore directory.",
            ));
        }

        let spore_core = std::fs::read_to_string(spore_core_path).map_err(|e| {
            crate::HyphaError::new(
                "absorb_error",
                format!("Failed to read spore.core.json: {}", e),
            )
        })?;

        let _core: serde_json::Value = serde_json::from_str(&spore_core).map_err(|e| {
            crate::HyphaError::new(
                "absorb_error",
                format!("Failed to parse spore.core.json: {}", e),
            )
        })?;

        // Read spawned_from from .cmn/spawned-from/spore.json
        let spawned_from_path = std::path::Path::new(".cmn/spawned-from/spore.json");
        let parent: substrate::Spore =
            serde_json::from_str(&std::fs::read_to_string(spawned_from_path).map_err(|_| {
                crate::HyphaError::with_hint(
                    "absorb_error",
                    "Not a spawned spore — .cmn/spawned-from/spore.json not found",
                    "run: hypha spawn <URI>",
                )
            })?)
            .map_err(|e| {
                crate::HyphaError::new(
                    "absorb_error",
                    format!("Failed to parse .cmn/spawned-from/spore.json: {}", e),
                )
            })?;
        let spawned_uri = parent.uri().to_string();

        let spawned_parsed = CmnUri::parse(&spawned_uri).map_err(|e| {
            crate::HyphaError::new("absorb_error", format!("Invalid spawned_from URI: {}", e))
        })?;

        let spawned_hash = spawned_parsed.hash.as_deref().ok_or_else(|| {
            crate::HyphaError::new("absorb_error", "spawned_from URI must include a hash")
        })?;

        let bonds = fetch_bonds(
            &resolved.url,
            spawned_hash,
            "inbound",
            max_depth,
            resolved.token_secret.as_deref(),
        )
        .await?;

        let mut discovered: Vec<String> = bonds
            .result
            .bonds
            .iter()
            .map(|s| s.uri.clone())
            .filter(|uri| *uri != spawned_uri)
            .collect();

        for uri in uris {
            if !discovered.contains(&uri) {
                discovered.push(uri);
            }
        }

        if discovered.is_empty() {
            return Err(crate::HyphaError::new(
                "absorb_error",
                "No descendants found to absorb. Use explicit URIs or check Synapse index.",
            ));
        }

        sink.emit(crate::HyphaEvent::Warn {
            message: format!(
                "Discovered {} potential sources from lineage",
                discovered.len()
            ),
        });
        discovered
    } else {
        if uris.is_empty() {
            return Err(crate::HyphaError::new(
                "absorb_error",
                "No URIs provided. Specify URIs or use --discover flag.",
            ));
        }
        uris
    };

    // Validate URIs
    let mut parsed_uris: Vec<(CmnUri, String)> = Vec::new();
    for uri_str in &uris_to_absorb {
        let uri = CmnUri::parse(uri_str).map_err(|e| {
            crate::HyphaError::new("invalid_uri", format!("Invalid URI {}: {}", uri_str, e))
        })?;

        let hash = uri.hash.clone().ok_or_else(|| {
            crate::HyphaError::new("invalid_uri", format!("{} must include a hash", uri_str))
        })?;

        parsed_uris.push((uri, hash));
    }

    let absorb_dir = std::path::Path::new(".cmn/absorb");
    std::fs::create_dir_all(absorb_dir).map_err(|e| {
        crate::HyphaError::new(
            "absorb_error",
            format!("Failed to create .cmn/absorb/: {}", e),
        )
    })?;

    let cache = CacheDir::new();
    let mut sources: Vec<AbsorbSource> = Vec::new();

    for (uri, hash) in &parsed_uris {
        let uri_str_current = format!("cmn://{}/{}", uri.domain, hash);

        check_taste(sink, &cache, &uri_str_current, &uri.domain, hash)?;

        sink.emit(crate::HyphaEvent::Warn {
            message: format!("Fetching {}...", uris_to_absorb[sources.len()]),
        });

        let domain_cache = cache.domain(&uri.domain);

        let entry = get_cmn_entry(sink, &domain_cache, cache.cmn_ttl_ms).await?;

        let capsule = primary_capsule(&entry)?;
        let public_key = capsule.key.clone();
        let ep = &capsule.endpoints;

        let manifest = fetch_spore_manifest(capsule, hash).await.map_err(|e| {
            crate::HyphaError::new(
                "manifest_failed",
                format!("Failed to fetch spore {}: {}", hash, e),
            )
        })?;
        let spore = decode_spore_manifest(&manifest)?;

        let author_key = embedded_spore_author_key(&manifest);
        let ak = author_key.as_deref().unwrap_or(&public_key);

        verify_manifest_two_key_signatures(&manifest, &public_key, ak).map_err(|e| {
            crate::HyphaError::new(
                "sig_failed",
                format!("Signature verification failed for {}: {}", hash, e),
            )
        })?;

        sink.emit(crate::HyphaEvent::Warn {
            message: "Signature verified".to_string(),
        });

        let name = spore.capsule.core.name.as_str();

        let source_dir = absorb_dir.join(hash);
        std::fs::create_dir_all(&source_dir).map_err(|e| {
            crate::HyphaError::new(
                "absorb_error",
                format!("Failed to create {}: {}", source_dir.display(), e),
            )
        })?;

        let manifest_path = source_dir.join("spore.json");
        std::fs::write(
            &manifest_path,
            serde_json::to_string_pretty(&spore).unwrap_or_default(),
        )
        .map_err(|e| {
            crate::HyphaError::new("absorb_error", format!("Failed to write spore.json: {}", e))
        })?;

        let dist_array = spore.distributions();
        if dist_array.is_empty() {
            return Err(crate::HyphaError::new(
                "manifest_failed",
                format!("No distribution options for {}", hash),
            ));
        }

        let content_dir = source_dir.join("content");
        std::fs::create_dir_all(&content_dir).map_err(|e| {
            crate::HyphaError::new(
                "absorb_error",
                format!("Failed to create content dir: {}", e),
            )
        })?;

        let archive_endpoints = ep
            .iter()
            .filter(|endpoint| endpoint.kind == "archive")
            .collect::<Vec<_>>();
        let mut downloaded = false;
        for dist_entry in dist_array {
            if dist_has_type(dist_entry, "archive") {
                for archive_ep in &archive_endpoints {
                    let archive_url = build_archive_url_from_endpoint(archive_ep, hash)?;

                    if content_dir.exists() {
                        std::fs::remove_dir_all(&content_dir).map_err(|e| {
                            crate::HyphaError::new(
                                "absorb_error",
                                format!("Failed to reset content dir: {}", e),
                            )
                        })?;
                    }
                    std::fs::create_dir_all(&content_dir).map_err(|e| {
                        crate::HyphaError::new(
                            "absorb_error",
                            format!("Failed to recreate content dir: {}", e),
                        )
                    })?;

                    match download_and_extract_to_dir(
                        &archive_url,
                        &content_dir,
                        archive_ep.format.as_deref(),
                    )
                    .await
                    {
                        Ok(_) => {
                            downloaded = true;
                            break;
                        }
                        Err(e) if e.is_malicious() => {
                            let msg = e.to_string();
                            mark_toxic(&domain_cache, hash, &msg);
                            return Err(crate::HyphaError::new("TOXIC", msg));
                        }
                        Err(e) => {
                            sink.emit(crate::HyphaEvent::Warn {
                                message: format!("Failed to download from {}: {}", archive_url, e),
                            });
                        }
                    }
                }
                if downloaded {
                    break;
                }
            } else if let Some(git_url) = dist_git_url(dist_entry) {
                let git_ref = dist_git_ref(dist_entry);
                if content_dir.exists() {
                    std::fs::remove_dir_all(&content_dir).map_err(|e| {
                        crate::HyphaError::new(
                            "absorb_error",
                            format!("Failed to reset content dir: {}", e),
                        )
                    })?;
                }
                std::fs::create_dir_all(&content_dir).map_err(|e| {
                    crate::HyphaError::new(
                        "absorb_error",
                        format!("Failed to recreate content dir: {}", e),
                    )
                })?;

                match clone_git_to_dir(git_url, git_ref, &content_dir).await {
                    Ok(_) => {
                        downloaded = true;
                        break;
                    }
                    Err(e) => {
                        sink.emit(crate::HyphaEvent::Warn {
                            message: format!("Failed to clone from {}: {}", git_url, e),
                        });
                    }
                }
            }
        }

        if !downloaded {
            return Err(crate::HyphaError::new(
                "fetch_failed",
                format!("Failed to download content for {}", hash),
            ));
        }

        sources.push(AbsorbSource {
            uri: format!("cmn://{}/{}", uri.domain, hash),
            hash: hash.clone(),
            name: name.to_string(),
            path: format!(".cmn/absorb/{}/", hash),
        });
    }

    let prompt_path = absorb_dir.join("ABSORB.md");
    generate_absorb_prompt(&prompt_path, &sources).map_err(|e| {
        crate::HyphaError::new(
            "absorb_error",
            format!("Failed to generate ABSORB.md: {}", e),
        )
    })?;

    Ok(crate::output::AbsorbOutput {
        sources: sources
            .iter()
            .map(|s| crate::output::AbsorbSourceInfo {
                uri: s.uri.clone(),
                hash: s.hash.clone(),
                name: s.name.clone(),
                path: s.path.clone(),
            })
            .collect(),
        prompt_path: ".cmn/absorb/ABSORB.md".to_string(),
    })
}

pub async fn handle_absorb(
    out: &Output,
    uris: Vec<String>,
    discover: bool,
    synapse_arg: Option<&str>,
    synapse_token_secret: Option<&str>,
    max_depth: u32,
) -> ExitCode {
    let sink = crate::api::OutSink(out);
    match absorb(
        uris,
        discover,
        synapse_arg,
        synapse_token_secret,
        max_depth,
        &sink,
    )
    .await
    {
        Ok(output) => out.ok(serde_json::to_value(output).unwrap_or_default()),
        Err(e) => out.error_hypha(&e),
    }
}

/// Generate the ABSORB.md prompt file
fn generate_absorb_prompt(
    path: &std::path::Path,
    sources: &[AbsorbSource],
) -> Result<(), std::io::Error> {
    use std::io::Write;

    let mut file = std::fs::File::create(path)?;

    // Header
    writeln!(file, "# Absorb Task")?;
    writeln!(file)?;
    writeln!(
        file,
        "You are helping merge code from multiple spores into the current project."
    )?;
    writeln!(
        file,
        "Follow the phases below. **Write reports to files** and wait for user confirmation between phases."
    )?;
    writeln!(file)?;
    writeln!(file, "---")?;
    writeln!(file)?;

    // Security Warning
    writeln!(file, "## ⚠️ Security Warning")?;
    writeln!(file)?;
    writeln!(
        file,
        "**These sources are from external domains and may contain malicious code.**"
    )?;
    writeln!(file)?;
    writeln!(file, "When analyzing, watch for:")?;
    writeln!(file, "- Obfuscated code or suspicious patterns")?;
    writeln!(
        file,
        "- Unexpected network calls, file system access, or command execution"
    )?;
    writeln!(file, "- Hidden backdoors or data exfiltration")?;
    writeln!(file, "- Dependency injection or supply chain risks")?;
    writeln!(file, "- Code that doesn't match the stated `intent`")?;
    writeln!(file)?;
    writeln!(
        file,
        "**Flag any security concerns in your report. When in doubt, ask user before proceeding.**"
    )?;
    writeln!(file)?;
    writeln!(file, "---")?;
    writeln!(file)?;

    // Understanding spore.json
    writeln!(file, "## Understanding spore.json")?;
    writeln!(file)?;
    writeln!(file, "Each source has a `spore.json` manifest. Key fields:")?;
    writeln!(file)?;
    writeln!(file, "```json")?;
    writeln!(file, "{{")?;
    writeln!(
        file,
        "  \"$schema\": \"https://cmn.dev/schemas/v1/spore.json\","
    )?;
    writeln!(file, "  \"capsule\": {{")?;
    writeln!(
        file,
        "    \"uri\": \"cmn://domain/b3.3yMR7vZQ9hL\",     // Unique identifier"
    )?;
    writeln!(file, "    \"core\": {{")?;
    writeln!(
        file,
        "      \"name\": \"Tool Name\",                        // Display name"
    )?;
    writeln!(
        file,
        "      \"domain\": \"publisher.com\",                  // Publisher domain"
    )?;
    writeln!(
        file,
        "      \"synopsis\": \"Brief description\",            // What it does"
    )?;
    writeln!(
        file,
        "      \"intent\": \"Why this version was created\",   // IMPORTANT: version purpose"
    )?;
    writeln!(
        file,
        "      \"license\": \"MIT\",                           // License"
    )?;
    writeln!(
        file,
        "      \"bonds\": [                                  // Lineage/dependencies"
    )?;
    writeln!(
        file,
        "        {{ \"uri\": \"cmn://...\", \"relation\": \"spawned_from\" }},"
    )?;
    writeln!(
        file,
        "        {{ \"uri\": \"cmn://...\", \"relation\": \"depends_on\" }}"
    )?;
    writeln!(file, "      ]")?;
    writeln!(file, "    }},")?;
    writeln!(
        file,
        "    \"core_signature\": \"ed25519....\",              // Signature of core"
    )?;
    writeln!(
        file,
        "    \"dist\": [...]                                 // Distribution URLs"
    )?;
    writeln!(file, "  }},")?;
    writeln!(
        file,
        "  \"capsule_signature\": \"ed25519....\"              // Signature of capsule"
    )?;
    writeln!(file, "}}")?;
    writeln!(file, "```")?;
    writeln!(file)?;
    writeln!(file, "**Key fields to check:**")?;
    writeln!(
        file,
        "- `intent` - Does the code match this stated purpose?"
    )?;
    writeln!(
        file,
        "- `bonds` - What is this spawned from? What dependencies?"
    )?;
    writeln!(file, "- `domain` - Who published this? Is it trustworthy?")?;
    writeln!(file)?;
    writeln!(file, "---")?;
    writeln!(file)?;

    // Phase 1: Analyze Each Source
    writeln!(file, "## Phase 1: Analyze Each Source")?;
    writeln!(file)?;
    writeln!(file, "For each source, create a detailed report file.")?;
    writeln!(file)?;

    for (i, source) in sources.iter().enumerate() {
        writeln!(file, "### Source {}: {}", i + 1, source.name)?;
        writeln!(file, "| Field | Value |")?;
        writeln!(file, "|-------|-------|")?;
        writeln!(file, "| URI | `{}` |", source.uri)?;
        writeln!(file, "| Name | {} |", source.name)?;
        writeln!(file, "| Manifest | `{}spore.json` |", source.path)?;
        writeln!(file, "| Code | `{}content/` |", source.path)?;
        writeln!(file)?;

        writeln!(
            file,
            "**Task:** Analyze and write report to `.cmn/absorb/{}.report.md`:",
            source.hash
        )?;
        writeln!(file)?;
        writeln!(file, "```markdown")?;
        writeln!(file, "# Source Report: {}", source.name)?;
        writeln!(file)?;
        writeln!(file, "## Summary")?;
        writeln!(file, "[Brief description of what this source adds]")?;
        writeln!(file)?;
        writeln!(file, "## File Analysis")?;
        writeln!(file)?;
        writeln!(file, "| File | Status | Description |")?;
        writeln!(file, "|------|--------|-------------|")?;
        writeln!(file, "| src/file.rs | New/Modified | Description |")?;
        writeln!(file)?;
        writeln!(file, "## Detailed Changes")?;
        writeln!(file, "[Describe key changes in each file]")?;
        writeln!(file)?;
        writeln!(file, "## Potential Conflicts")?;
        writeln!(
            file,
            "[List files that may conflict with current project or other sources]"
        )?;
        writeln!(file)?;
        writeln!(file, "## Dependencies")?;
        writeln!(file, "[Any new dependencies added]")?;
        writeln!(file)?;
        writeln!(file, "## Security Assessment")?;
        writeln!(file, "| Check | Status | Notes |")?;
        writeln!(file, "|-------|--------|-------|")?;
        writeln!(file, "| Code matches intent? | ✅/⚠️/❌ | |")?;
        writeln!(file, "| No obfuscated code? | ✅/⚠️/❌ | |")?;
        writeln!(file, "| No suspicious network calls? | ✅/⚠️/❌ | |")?;
        writeln!(file, "| No unexpected file/system access? | ✅/⚠️/❌ | |")?;
        writeln!(file, "| Dependencies look safe? | ✅/⚠️/❌ | |")?;
        writeln!(file)?;
        writeln!(file, "## Recommendation")?;
        writeln!(
            file,
            "[Should we absorb? Fully or partially? Any security caveats?]"
        )?;
        writeln!(file, "```")?;
        writeln!(file)?;
    }

    writeln!(
        file,
        "**After completing all reports, inform user and wait for confirmation to proceed.**"
    )?;
    writeln!(file)?;
    writeln!(file, "---")?;
    writeln!(file)?;

    // Phase 2: Cross-Source Analysis
    writeln!(file, "## Phase 2: Cross-Source Analysis")?;
    writeln!(file)?;
    writeln!(
        file,
        "After user confirms Phase 1, analyze relationships between sources:"
    )?;
    writeln!(file)?;
    writeln!(
        file,
        "1. **Overlap Detection**: Do sources modify the same files/functions?"
    )?;
    writeln!(
        file,
        "2. **Compatibility**: Are the changes compatible or conflicting?"
    )?;
    writeln!(
        file,
        "3. **Dependencies**: Does one source's changes depend on another?"
    )?;
    writeln!(
        file,
        "4. **Priority**: If conflicts exist, which source's approach is better?"
    )?;
    writeln!(file)?;
    writeln!(file, "Output a summary table:")?;
    writeln!(file)?;

    // Generate table headers based on sources
    let mut header = "| File |".to_string();
    for source in sources {
        header.push_str(&format!(" {} |", source.name));
    }
    header.push_str(" Conflict? | Recommendation |");
    writeln!(file, "{}", header)?;

    let mut separator = "|------|".to_string();
    for _ in sources {
        separator.push_str("---------|");
    }
    separator.push_str("-----------|----------------|");
    writeln!(file, "{}", separator)?;

    writeln!(
        file,
        "| src/example.rs | Modified | - | Yes/No | [Recommendation] |"
    )?;
    writeln!(file)?;
    writeln!(file, "---")?;
    writeln!(file)?;

    // Phase 3: Merge Plan
    writeln!(file, "## Phase 3: Merge Plan")?;
    writeln!(file)?;
    writeln!(file, "Propose a concrete merge plan:")?;
    writeln!(file)?;
    writeln!(file, "```")?;
    writeln!(file, "Recommended Merge Order:")?;
    for (i, source) in sources.iter().enumerate() {
        writeln!(file, "{}. [ ] {} (reason: ...)", i + 1, source.name)?;
    }
    writeln!(file)?;
    writeln!(file, "File-by-file plan:")?;
    writeln!(file, "- src/file.rs: Take from [source], because...")?;
    writeln!(file)?;
    writeln!(file, "Potential issues:")?;
    writeln!(file, "- [list any concerns]")?;
    writeln!(file)?;
    writeln!(file, "Alternatives:")?;
    writeln!(file, "- Option A: [description]")?;
    writeln!(file, "- Option B: [description]")?;
    writeln!(file, "```")?;
    writeln!(file)?;
    writeln!(file, "**Wait for user decision before proceeding.**")?;
    writeln!(file)?;
    writeln!(file, "---")?;
    writeln!(file)?;

    // Phase 4: Execute Merge
    writeln!(file, "## Phase 4: Execute Merge")?;
    writeln!(file)?;
    writeln!(file, "After user approves the plan:")?;
    writeln!(file)?;
    writeln!(file, "1. Apply changes according to approved plan")?;
    writeln!(file, "2. Resolve any conflicts as discussed")?;
    writeln!(file, "3. Ensure code compiles/works")?;
    writeln!(file, "4. Run tests if available")?;
    writeln!(file)?;
    writeln!(file, "---")?;
    writeln!(file)?;

    // Phase 5: Update spore.core.json
    writeln!(file, "## Phase 5: Update spore.core.json")?;
    writeln!(file)?;
    writeln!(file, "After merge is complete, update bonds:")?;
    writeln!(file)?;
    writeln!(file, "```json")?;
    writeln!(file, "{{")?;
    writeln!(file, "  \"bonds\": [")?;
    writeln!(file, "    // existing bonds...")?;
    for source in sources {
        writeln!(
            file,
            "    {{ \"uri\": \"{}\", \"relation\": \"absorbed_from\" }},",
            source.uri
        )?;
    }
    writeln!(file, "  ]")?;
    writeln!(file, "}}")?;
    writeln!(file, "```")?;
    writeln!(file)?;
    writeln!(file, "---")?;
    writeln!(file)?;

    // Phase 6: Cleanup
    writeln!(file, "## Phase 6: Cleanup")?;
    writeln!(file)?;
    writeln!(file, "```bash")?;
    writeln!(file, "# After successful merge, clean up absorb directory")?;
    writeln!(file, "rm -rf .cmn/absorb/")?;
    writeln!(file)?;
    writeln!(file, "# Commit changes")?;
    writeln!(file, "git add .")?;
    writeln!(file, "git commit -m \"absorb: [describe what was merged]\"")?;
    writeln!(file, "```")?;
    writeln!(file)?;
    writeln!(file, "---")?;
    writeln!(file)?;

    // Guidelines
    writeln!(file, "## Guidelines")?;
    writeln!(file)?;
    writeln!(file, "- **Preserve current project's coding style**")?;
    writeln!(file, "- **Don't blindly replace** - intelligently merge")?;
    writeln!(file, "- **Ask user before major structural changes**")?;
    writeln!(file, "- **Document what was absorbed and why**")?;

    Ok(())
}
