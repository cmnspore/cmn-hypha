use std::io::Write;
use std::process::ExitCode;

use hypha::api::Output;
use hypha::cli::*;
use hypha::{cache, config, mycelium, spore, synapse, visitor};

fn emit_cli_json(message: &str) {
    let mut stdout = std::io::stdout();
    let _ = stdout.write_all(message.as_bytes());
    let _ = stdout.write_all(b"\n");
}

fn main() -> ExitCode {
    let cli = hypha::cli::parse_or_exit();

    let output_format = agent_first_data::cli_parse_output(&cli.output).unwrap_or_else(|e| {
        emit_cli_json(&agent_first_data::output_json(
            &agent_first_data::build_cli_error(&e, None),
        ));
        std::process::exit(2);
    });

    let log = agent_first_data::cli_parse_log_filters(&cli.log);
    let out = Output::new(output_format);

    // Emit Agent-First Data startup message if --log startup (or all/*) is set
    if log
        .iter()
        .any(|f| matches!(f.as_str(), "startup" | "all" | "*"))
    {
        let mut args = serde_json::to_value(&cli.command)
            .unwrap_or_else(|_| serde_json::Value::Object(Default::default()));
        if let Some(obj) = args.as_object_mut() {
            obj.insert(
                "output".to_string(),
                serde_json::Value::String(cli.output.clone()),
            );
            if let Ok(log_value) = serde_json::to_value(&log) {
                obj.insert("log".to_string(), log_value);
            }
        }
        out.startup(args);
    }

    // Create tokio runtime for async operations
    let rt = match tokio::runtime::Runtime::new() {
        Ok(rt) => rt,
        Err(e) => {
            emit_cli_json(&agent_first_data::output_json(
                &agent_first_data::build_cli_error(
                    &format!("Failed to create async runtime: {}", e),
                    None,
                ),
            ));
            return ExitCode::FAILURE;
        }
    };

    match cli.command {
        // ═══════════════════════════════════════════
        // New top-level lifecycle commands
        // ═══════════════════════════════════════════
        Commands::Sense { uri } => rt.block_on(visitor::handle_sense(&out, &uri)),

        Commands::Taste {
            uri,
            verdict,
            notes,
            synapse,
            synapse_token_secret,
            domain,
        } => rt.block_on(visitor::handle_taste(
            &out,
            &uri,
            verdict,
            notes.as_deref(),
            synapse.as_deref(),
            synapse_token_secret.as_deref(),
            domain.as_deref(),
        )),

        Commands::Spawn {
            uri,
            directory,
            vcs,
            dist,
            bond,
        } => rt.block_on(visitor::handle_spawn(
            &out,
            &uri,
            directory.as_deref(),
            vcs.as_deref(),
            dist.as_deref(),
            bond,
        )),

        Commands::Grow {
            dist,
            synapse,
            synapse_token_secret,
            bond,
        } => rt.block_on(visitor::handle_grow(
            &out,
            None,
            dist.as_deref(),
            bond,
            synapse.as_deref(),
            synapse_token_secret.as_deref(),
        )),

        Commands::Absorb {
            uris,
            discover,
            synapse,
            synapse_token_secret,
            max_depth,
        } => rt.block_on(visitor::handle_absorb(
            &out,
            uris,
            discover,
            synapse.as_deref(),
            synapse_token_secret.as_deref(),
            max_depth,
        )),

        Commands::Bond { clean, status } => {
            rt.block_on(visitor::handle_bond_fetch(&out, clean, status))
        }

        Commands::Replicate {
            uris,
            refs,
            domain,
            site_path,
        } => rt.block_on(spore::handle_replicate(
            &out,
            uris,
            refs,
            &domain,
            site_path.as_deref(),
        )),

        Commands::Hatch {
            id,
            version,
            name,
            domain,
            synopsis,
            intent,
            mutations,
            license,
            command,
        } => match command {
            Some(HatchCommands::Bond { command }) => match command {
                HatchBondCommands::Set {
                    uri,
                    relation,
                    id,
                    reason,
                    with_entries,
                } => spore::handle_bond_set(&out, &uri, relation, id, reason, with_entries),
                HatchBondCommands::Remove { uri, relation } => {
                    spore::handle_bond_remove(&out, uri, relation)
                }
                HatchBondCommands::Clear => spore::handle_bond_clear(&out),
            },
            Some(HatchCommands::Tree { command }) => match command {
                HatchTreeCommands::Set {
                    algorithm,
                    exclude_names,
                    follow_rules,
                } => spore::handle_tree_set(&out, algorithm, exclude_names, follow_rules),
                HatchTreeCommands::Show => spore::handle_tree_show(&out),
            },
            None => spore::handle_hatch(
                &out,
                spore::HatchArgs {
                    id,
                    version,
                    name,
                    domain,
                    synopsis,
                    intent,
                    mutations,
                    license,
                },
            ),
        },

        Commands::Release {
            domain,
            source,
            site_path,
            dist_git,
            dist_ref,
            archive,
            dry_run,
        } => spore::handle_release(
            &out,
            spore::ReleaseArgs {
                domain: &domain,
                source,
                site_path: site_path.as_deref(),
                dist_git,
                dist_ref,
                archive: &archive,
                dry_run,
            },
        ),

        Commands::Lineage {
            uri,
            direction,
            synapse,
            synapse_token_secret,
            max_depth,
        } => rt.block_on(visitor::handle_lineage(
            &out,
            &uri,
            direction.as_deref(),
            synapse.as_deref(),
            synapse_token_secret.as_deref(),
            max_depth,
        )),

        Commands::Search {
            query,
            synapse,
            synapse_token_secret,
            domain,
            license,
            bonds,
            limit,
        } => rt.block_on(visitor::handle_search(
            &out,
            &query,
            synapse.as_deref(),
            synapse_token_secret.as_deref(),
            domain.as_deref(),
            license.as_deref(),
            bonds.as_deref(),
            limit,
        )),

        // ═══════════════════════════════════════════
        // Infrastructure commands
        // ═══════════════════════════════════════════
        Commands::Mycelium { action } => match action {
            MyceliumAction::Root {
                domain,
                hub,
                site_path,
                name,
                synopsis,
                bio,
                endpoints_base,
            } => mycelium::handle_init(
                &out,
                mycelium::InitArgs {
                    domain: domain.as_deref(),
                    hub: hub.as_deref(),
                    site_path: site_path.as_deref(),
                    name: name.as_deref(),
                    synopsis: synopsis.as_deref(),
                    bio: bio.as_deref(),
                    endpoints_base: endpoints_base.as_deref(),
                },
            ),
            MyceliumAction::Nutrient { command } => match command {
                NutrientCommands::Add {
                    domain,
                    method_type,
                    with_entries,
                    site_path,
                } => mycelium::handle_nutrient_add(
                    &out,
                    &domain,
                    &method_type,
                    with_entries,
                    site_path.as_deref(),
                ),
                NutrientCommands::Remove {
                    domain,
                    method_type,
                    site_path,
                } => mycelium::handle_nutrient_remove(
                    &out,
                    &domain,
                    &method_type,
                    site_path.as_deref(),
                ),
                NutrientCommands::Clear { domain, site_path } => {
                    mycelium::handle_nutrient_clear(&out, &domain, site_path.as_deref())
                }
            },
            MyceliumAction::Status { domain, site_path } => {
                mycelium::handle_status(&out, domain.as_deref(), site_path.as_deref())
            }
            MyceliumAction::Pulse {
                synapse,
                synapse_token_secret,
                file,
            } => rt.block_on(mycelium::handle_pulse(
                &out,
                synapse.as_deref(),
                synapse_token_secret.as_deref(),
                &file,
            )),
            MyceliumAction::Serve {
                domain,
                site_path,
                port,
            } => mycelium::handle_serve(&out, domain.as_deref(), site_path.as_deref(), port),
        },

        Commands::Synapse { action } => match action {
            SynapseAction::Discover {
                synapse,
                synapse_token_secret,
            } => rt.block_on(synapse::handle_discover(
                &out,
                synapse.as_deref(),
                synapse_token_secret.as_deref(),
            )),
            SynapseAction::List => synapse::handle_list(&out),
            SynapseAction::Health {
                synapse,
                synapse_token_secret,
            } => rt.block_on(synapse::handle_info(
                &out,
                synapse.as_deref(),
                synapse_token_secret.as_deref(),
            )),
            SynapseAction::Add { url } => synapse::handle_add(&out, &url),
            SynapseAction::Remove { domain } => synapse::handle_remove(&out, &domain),
            SynapseAction::Use { domain } => synapse::handle_use(&out, &domain),
            SynapseAction::Config {
                domain,
                token_secret,
            } => synapse::handle_config(&out, &domain, token_secret.as_deref()),
        },

        Commands::Cache { action } => match action {
            CacheAction::List => cache::handle_list(&out),
            CacheAction::Clean { all } => cache::handle_clean(&out, all),
            CacheAction::Path { uri } => cache::handle_path(&out, &uri),
        },

        Commands::Config { action } => match action {
            ConfigAction::List => config::handle_list(&out),
            ConfigAction::Set { key, value } => config::handle_set(&out, &key, &value),
        },
    }
}
