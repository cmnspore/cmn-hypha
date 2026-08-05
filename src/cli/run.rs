use std::process::ExitCode;

use crate::api::Output;
use crate::{cache, config, mycelium, skill_admin, spore, synapse, visitor};

use super::*;

fn emit_preflight_cli_error(message: &str, hint: &str) -> ExitCode {
    let mut emitter = agent_first_data::CliEmitter::finite(agent_first_data::OutputFormat::Json)
        .with_strict_protocol();
    ExitCode::from(emitter.finish(agent_first_data::build_cli_error(message, Some(hint)), 2))
}

fn build_runtime(out: &Output) -> Result<tokio::runtime::Runtime, ExitCode> {
    tokio::runtime::Runtime::new().map_err(|e| {
        out.error_hint(
            "runtime_init_failed",
            &format!("Failed to create async runtime: {e}"),
            Some("retry the command; if it repeats, check available system resources"),
        )
    })
}

fn emit_startup(out: &Output, cli: &Cli, log: &agent_first_data::LogFilters) {
    if log.enabled("startup") {
        let mut args = match serde_json::to_value(&cli.command) {
            Ok(args) => args,
            Err(err) => {
                out.warn(
                    "startup_args_serialize_error",
                    &format!("Failed to serialize parsed CLI arguments: {err}"),
                );
                serde_json::json!({
                    "serialization_error": err.to_string(),
                })
            }
        };
        if let Some(obj) = args.as_object_mut() {
            obj.insert(
                "output".to_string(),
                serde_json::Value::String(cli.output.clone()),
            );
            obj.insert(
                "output_to".to_string(),
                serde_json::Value::String(cli.output_to.clone()),
            );
            match serde_json::to_value(log.as_slice()) {
                Ok(log_value) => {
                    obj.insert("log".to_string(), log_value);
                }
                Err(err) => {
                    out.warn(
                        "startup_log_filter_serialize_error",
                        &format!("Failed to serialize CLI log filters: {err}"),
                    );
                }
            }
        }
        out.startup(args);
    }
}

pub fn execute(cli: Cli) -> ExitCode {
    let output_format = match agent_first_data::cli_parse_output(&cli.output) {
        Ok(format) => format,
        Err(message) => {
            return emit_preflight_cli_error(
                &message,
                "use --output json, --output yaml, or --output plain",
            )
        }
    };
    let output_to = match agent_first_data::OutputTo::parse(&cli.output_to) {
        Ok(output_to) => output_to,
        Err(message) => {
            return emit_preflight_cli_error(
                &message,
                "use --output-to split, --output-to stdout, or --output-to stderr",
            )
        }
    };

    let log = agent_first_data::cli_parse_log_filters(&cli.log);
    let out = Output::with_output_to(output_format, output_to);
    emit_startup(&out, &cli, &log);

    let rt = match build_runtime(&out) {
        Ok(rt) => rt,
        Err(code) => return code,
    };

    match cli.command {
        Commands::Sense { uri, id } => {
            rt.block_on(visitor::handle_sense(&out, &uri, id.as_deref()))
        }

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
            vcs.map(|v| v.as_str()),
            dist.map(|d| d.as_str()),
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
            dist.map(|d| d.as_str()),
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
                HatchBondCommands::Sync {
                    relation,
                    spec,
                    domain,
                    site_path,
                    check,
                } => spore::handle_bond_sync(&out, relation, &spec, domain, site_path, check),
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
            direction.map(|d| d.as_str()),
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
            MyceliumAction::Spore { command } => match command {
                SporeCommands::Yank {
                    id,
                    domain,
                    site_path,
                    purge,
                } => mycelium::handle_spore_yank(
                    &out,
                    domain.as_deref(),
                    &id,
                    site_path.as_deref(),
                    purge,
                ),
                SporeCommands::Unyank {
                    id,
                    domain,
                    site_path,
                } => mycelium::handle_spore_unyank(
                    &out,
                    domain.as_deref(),
                    &id,
                    site_path.as_deref(),
                ),
            },
            MyceliumAction::Status {
                domain,
                site_path,
                id,
            } => mycelium::handle_status(
                &out,
                domain.as_deref(),
                site_path.as_deref(),
                id.as_deref(),
            ),
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

        Commands::Skill { action } => skill_admin::handle_skill(&out, action),
    }
}
