//! Resolve the closed registry into Hypha's typed command model.

use std::io::Write;
use std::str::FromStr;

use agent_first_data::{
    cli_error_event, cli_help_event, cli_parse_output, cli_version_event, render_cli_reference,
    BoundOutcome, BuiltCliSpec, CliEmitter, CliValue, OutputFormat, OutputPlan, OutputTo,
    ResolvedInvocation,
};

use super::spec;
use super::types::*;

type BuildResult = Result<Cli, String>;
type ActionHandler = fn(&ResolvedInvocation) -> BuildResult;

pub fn parse_or_exit() -> (
    Cli,
    Option<agent_first_data::stream_redirect::InstalledStreamRedirect>,
) {
    let cli = match spec::cli_spec() {
        Ok(cli) => cli,
        Err(error) => emit_startup_error_or_exit("cli_spec_invalid", &error.to_string()),
    };
    let app = match cli.bind_actions(action_handlers(&cli)) {
        Ok(app) => app,
        Err(error) => emit_startup_error_or_exit("cli_actions_invalid", &error.to_string()),
    };
    let outcome = match app.resolve_from(std::env::args_os()) {
        Ok(outcome) => outcome,
        Err(error) => emit_event_or_exit(
            cli_error_event(&error),
            OutputFormat::Json,
            OutputTo::Stderr,
            error.exit_code(),
        ),
    };

    match outcome {
        BoundOutcome::Run(invocation) => {
            let redirect = install_redirect_or_exit(invocation.output_plan());
            match invocation.run() {
                Ok(command) => (command, redirect),
                Err(message) => emit_invalid_invocation_or_exit(&message),
            }
        }
        BoundOutcome::Docs(docs) => {
            let _redirect = install_redirect_or_exit(docs.output_plan());
            write_text_or_exit(&render_cli_reference(&cli), OutputTo::Stdout)
        }
        BoundOutcome::Help(help) => {
            let _redirect = install_redirect_or_exit(help.output_plan());
            let format = format_of(help.output_plan());
            if format == OutputFormat::Plain {
                write_text_or_exit(&help.plain(), raw_destination(help.output_plan()));
            }
            emit_event_or_exit(
                cli_help_event(&help),
                format,
                destination_of(help.output_plan()),
                0,
            )
        }
        BoundOutcome::Version(version) => {
            let _redirect = install_redirect_or_exit(version.output_plan());
            emit_event_or_exit(
                cli_version_event(&version),
                format_of(version.output_plan()),
                destination_of(version.output_plan()),
                0,
            )
        }
    }
}

fn action_handlers(cli: &BuiltCliSpec) -> Vec<(String, ActionHandler)> {
    let mut actions: Vec<String> = cli
        .spec()
        .commands
        .iter()
        .flat_map(|command| command.combinations.iter())
        .map(|combination| combination.action_id.clone())
        .collect();
    actions.sort();
    actions.dedup();
    actions
        .into_iter()
        .map(|action| (action, build_cli as ActionHandler))
        .collect()
}

fn build_cli(invocation: &ResolvedInvocation) -> BuildResult {
    let command = match invocation.action_id() {
        "sense" => Commands::Sense {
            uri: required_string(invocation, "uri")?,
            id: optional_string(invocation, "id"),
        },
        "taste" => Commands::Taste {
            uri: required_string(invocation, "uri")?,
            verdict: optional_verdict(invocation)?,
            notes: optional_string(invocation, "notes"),
            synapse: optional_string(invocation, "synapse"),
            synapse_token_secret: optional_string(invocation, "synapse_token_secret"),
            domain: optional_string(invocation, "domain"),
        },
        "spawn" => Commands::Spawn {
            uri: required_string(invocation, "uri")?,
            directory: optional_string(invocation, "directory"),
            vcs: optional_vcs(invocation)?,
            dist: optional_dist(invocation)?,
            bond: flag(invocation, "bond"),
        },
        "grow" => Commands::Grow {
            dist: optional_dist(invocation)?,
            synapse: optional_string(invocation, "synapse"),
            synapse_token_secret: optional_string(invocation, "synapse_token_secret"),
            bond: flag(invocation, "bond"),
        },
        "absorb" => Commands::Absorb {
            uris: repeated_strings(invocation, "uris"),
            discover: flag(invocation, "discover"),
            synapse: optional_string(invocation, "synapse"),
            synapse_token_secret: optional_string(invocation, "synapse_token_secret"),
            max_depth: u32_value(invocation, "max_depth", 10)?,
        },
        "bond" => Commands::Bond {
            clean: flag(invocation, "clean"),
            status: flag(invocation, "status"),
        },
        "replicate" => Commands::Replicate {
            uris: repeated_strings(invocation, "uris"),
            refs: flag(invocation, "refs"),
            domain: required_string(invocation, "domain")?,
            site_path: optional_string(invocation, "site_path"),
        },
        "hatch" => Commands::Hatch {
            id: optional_string(invocation, "id"),
            version: optional_string(invocation, "version"),
            name: optional_string(invocation, "name"),
            domain: optional_string(invocation, "domain"),
            synopsis: optional_string(invocation, "synopsis"),
            intent: repeated_strings(invocation, "intent"),
            mutations: repeated_strings(invocation, "mutations"),
            license: optional_string(invocation, "license"),
            command: None,
        },
        "hatch_bond_set" => hatch_subcommand(HatchCommands::Bond {
            command: HatchBondCommands::Set {
                uri: required_string(invocation, "uri")?,
                relation: optional_relation(invocation)?,
                id: optional_string(invocation, "id"),
                reason: optional_string(invocation, "reason"),
                with_entries: repeated_strings(invocation, "with"),
            },
        }),
        "hatch_bond_remove" => hatch_subcommand(HatchCommands::Bond {
            command: HatchBondCommands::Remove {
                uri: optional_string(invocation, "uri"),
                relation: optional_relation(invocation)?,
            },
        }),
        "hatch_bond_clear" => hatch_subcommand(HatchCommands::Bond {
            command: HatchBondCommands::Clear,
        }),
        "hatch_bond_sync" => hatch_subcommand(HatchCommands::Bond {
            command: HatchBondCommands::Sync {
                relation: required_relation(invocation, "relation")?,
                spec: required_string(invocation, "spec")?,
                domain: optional_string(invocation, "domain"),
                site_path: optional_string(invocation, "site_path"),
                check: flag(invocation, "check"),
            },
        }),
        "hatch_tree_set" => hatch_subcommand(HatchCommands::Tree {
            command: HatchTreeCommands::Set {
                algorithm: optional_string(invocation, "algorithm"),
                exclude_names: optional_strings(invocation, "exclude_names"),
                follow_rules: optional_strings(invocation, "follow_rules"),
            },
        }),
        "hatch_tree_show" => hatch_subcommand(HatchCommands::Tree {
            command: HatchTreeCommands::Show,
        }),
        "release" => Commands::Release {
            domain: required_string(invocation, "domain")?,
            source: optional_string(invocation, "source"),
            site_path: optional_string(invocation, "site_path"),
            dist_git: optional_string(invocation, "dist_git"),
            dist_ref: optional_string(invocation, "dist_ref"),
            archive: optional_string(invocation, "archive").unwrap_or_else(|| "zstd".to_string()),
            dry_run: flag(invocation, "dry_run"),
        },
        "lineage" => Commands::Lineage {
            uri: required_string(invocation, "uri")?,
            direction: optional_direction(invocation)?,
            synapse: optional_string(invocation, "synapse"),
            synapse_token_secret: optional_string(invocation, "synapse_token_secret"),
            max_depth: u32_value(invocation, "max_depth", 10)?,
        },
        "search" => Commands::Search {
            query: required_string(invocation, "query")?,
            synapse: optional_string(invocation, "synapse"),
            synapse_token_secret: optional_string(invocation, "synapse_token_secret"),
            domain: optional_string(invocation, "domain"),
            license: optional_string(invocation, "license"),
            bonds: optional_string(invocation, "bonds"),
            limit: u32_value(invocation, "limit", 20)?,
        },
        "mycelium_root" => Commands::Mycelium {
            action: MyceliumAction::Root {
                domain: optional_string(invocation, "domain"),
                hub: optional_string(invocation, "hub"),
                site_path: optional_string(invocation, "site_path"),
                name: optional_string(invocation, "name"),
                synopsis: optional_string(invocation, "synopsis"),
                bio: optional_string(invocation, "bio"),
                endpoints_base: optional_string(invocation, "endpoints_base"),
            },
        },
        "mycelium_status" => Commands::Mycelium {
            action: MyceliumAction::Status {
                domain: optional_string(invocation, "domain"),
                site_path: optional_string(invocation, "site_path"),
                id: optional_string(invocation, "id"),
            },
        },
        "mycelium_serve" => Commands::Mycelium {
            action: MyceliumAction::Serve {
                domain: optional_string(invocation, "domain"),
                site_path: optional_string(invocation, "site_path"),
                port: u16_value(invocation, "port", 8080)?,
            },
        },
        "mycelium_nutrient_add" => Commands::Mycelium {
            action: MyceliumAction::Nutrient {
                command: NutrientCommands::Add {
                    domain: required_string(invocation, "domain")?,
                    method_type: required_string(invocation, "type")?,
                    with_entries: repeated_strings(invocation, "with"),
                    site_path: optional_string(invocation, "site_path"),
                },
            },
        },
        "mycelium_nutrient_remove" => Commands::Mycelium {
            action: MyceliumAction::Nutrient {
                command: NutrientCommands::Remove {
                    domain: required_string(invocation, "domain")?,
                    method_type: required_string(invocation, "type")?,
                    site_path: optional_string(invocation, "site_path"),
                },
            },
        },
        "mycelium_nutrient_clear" => Commands::Mycelium {
            action: MyceliumAction::Nutrient {
                command: NutrientCommands::Clear {
                    domain: required_string(invocation, "domain")?,
                    site_path: optional_string(invocation, "site_path"),
                },
            },
        },
        "mycelium_spore_yank" => Commands::Mycelium {
            action: MyceliumAction::Spore {
                command: SporeCommands::Yank {
                    id: required_string(invocation, "id")?,
                    domain: optional_string(invocation, "domain"),
                    site_path: optional_string(invocation, "site_path"),
                    purge: flag(invocation, "purge"),
                },
            },
        },
        "mycelium_spore_unyank" => Commands::Mycelium {
            action: MyceliumAction::Spore {
                command: SporeCommands::Unyank {
                    id: required_string(invocation, "id")?,
                    domain: optional_string(invocation, "domain"),
                    site_path: optional_string(invocation, "site_path"),
                },
            },
        },
        "mycelium_pulse" => Commands::Mycelium {
            action: MyceliumAction::Pulse {
                synapse: optional_string(invocation, "synapse"),
                synapse_token_secret: optional_string(invocation, "synapse_token_secret"),
                file: required_string(invocation, "file")?,
            },
        },
        "synapse_discover" => Commands::Synapse {
            action: SynapseAction::Discover {
                synapse: optional_string(invocation, "synapse"),
                synapse_token_secret: optional_string(invocation, "synapse_token_secret"),
            },
        },
        "synapse_list" => Commands::Synapse {
            action: SynapseAction::List,
        },
        "synapse_health" => Commands::Synapse {
            action: SynapseAction::Health {
                synapse: optional_string(invocation, "synapse"),
                synapse_token_secret: optional_string(invocation, "synapse_token_secret"),
            },
        },
        "synapse_add" => Commands::Synapse {
            action: SynapseAction::Add {
                url: required_string(invocation, "url")?,
            },
        },
        "synapse_remove" => Commands::Synapse {
            action: SynapseAction::Remove {
                domain: required_string(invocation, "domain")?,
            },
        },
        "synapse_use" => Commands::Synapse {
            action: SynapseAction::Use {
                domain: required_string(invocation, "domain")?,
            },
        },
        "synapse_config" => Commands::Synapse {
            action: SynapseAction::Config {
                domain: required_string(invocation, "domain")?,
                token_secret: optional_string(invocation, "token_secret"),
            },
        },
        "cache_list" => Commands::Cache {
            action: CacheAction::List,
        },
        "cache_clean" => Commands::Cache {
            action: CacheAction::Clean {
                all: flag(invocation, "all"),
            },
        },
        "cache_path" => Commands::Cache {
            action: CacheAction::Path {
                uri: required_string(invocation, "uri")?,
            },
        },
        "config_list" => Commands::Config {
            action: ConfigAction::List,
        },
        "config_set" => Commands::Config {
            action: ConfigAction::Set {
                key: required_string(invocation, "key")?,
                value: required_string(invocation, "value")?,
            },
        },
        "skill_status" => Commands::Skill {
            action: SkillCommand::Status(skill_options(invocation)?),
        },
        "skill_install" => Commands::Skill {
            action: SkillCommand::Install(skill_options(invocation)?),
        },
        "skill_uninstall" => Commands::Skill {
            action: SkillCommand::Uninstall(skill_options(invocation)?),
        },
        action => {
            return Err(format!(
                "registry action `{action}` has no typed command builder"
            ))
        }
    };

    Ok(Cli {
        output: invocation
            .output_plan()
            .format()
            .unwrap_or("json")
            .to_string(),
        output_to: invocation
            .output_plan()
            .destination()
            .unwrap_or("split")
            .to_string(),
        log: log_filters(invocation),
        command,
    })
}

fn hatch_subcommand(command: HatchCommands) -> Commands {
    Commands::Hatch {
        id: None,
        version: None,
        name: None,
        domain: None,
        synopsis: None,
        intent: Vec::new(),
        mutations: Vec::new(),
        license: None,
        command: Some(command),
    }
}

fn skill_options(invocation: &ResolvedInvocation) -> Result<SkillOptionsArg, String> {
    let agent = match optional_string(invocation, "agent").as_deref() {
        None | Some("all") => SkillAgentArg::All,
        Some("codex") => SkillAgentArg::Codex,
        Some("claude-code") => SkillAgentArg::ClaudeCode,
        Some("opencode") => SkillAgentArg::Opencode,
        Some(_) => return Err("unsupported --agent value".to_string()),
    };
    let scope = match optional_string(invocation, "scope").as_deref() {
        None | Some("personal") => SkillScopeArg::Personal,
        Some("project") => SkillScopeArg::Project,
        Some(_) => return Err("unsupported --scope value".to_string()),
    };
    Ok(SkillOptionsArg {
        agent,
        scope,
        skills_dir: optional_string(invocation, "skills_dir"),
        force: flag(invocation, "force"),
    })
}

fn required_string(invocation: &ResolvedInvocation, id: &str) -> Result<String, String> {
    invocation
        .required(id)
        .as_str()
        .map(str::to_string)
        .ok_or_else(|| format!("registered argument `{id}` did not resolve to a string"))
}

fn optional_string(invocation: &ResolvedInvocation, id: &str) -> Option<String> {
    invocation
        .optional(id)
        .and_then(CliValue::as_str)
        .map(str::to_string)
}

fn repeated_strings(invocation: &ResolvedInvocation, id: &str) -> Vec<String> {
    invocation
        .repeated(id)
        .iter()
        .filter_map(CliValue::as_str)
        .map(str::to_string)
        .collect()
}

fn optional_strings(invocation: &ResolvedInvocation, id: &str) -> Option<Vec<String>> {
    let values = repeated_strings(invocation, id);
    (!values.is_empty()).then_some(values)
}

fn log_filters(invocation: &ResolvedInvocation) -> Vec<String> {
    repeated_strings(invocation, "log")
        .into_iter()
        .flat_map(|entry| {
            entry
                .split(',')
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .collect()
}

fn flag(invocation: &ResolvedInvocation, id: &str) -> bool {
    invocation
        .optional(id)
        .and_then(CliValue::as_bool)
        .unwrap_or(false)
}

fn i64_value(invocation: &ResolvedInvocation, id: &str, default: i64) -> i64 {
    invocation
        .optional(id)
        .and_then(CliValue::as_i64)
        .unwrap_or(default)
}

fn u32_value(invocation: &ResolvedInvocation, id: &str, default: u32) -> Result<u32, String> {
    u32::try_from(i64_value(invocation, id, i64::from(default))).map_err(|_| {
        format!(
            "`--{}` must be between 0 and {}",
            id.replace('_', "-"),
            u32::MAX
        )
    })
}

fn u16_value(invocation: &ResolvedInvocation, id: &str, default: u16) -> Result<u16, String> {
    u16::try_from(i64_value(invocation, id, i64::from(default))).map_err(|_| {
        format!(
            "`--{}` must be between 0 and {}",
            id.replace('_', "-"),
            u16::MAX
        )
    })
}

fn optional_verdict(
    invocation: &ResolvedInvocation,
) -> Result<Option<substrate::TasteVerdict>, String> {
    optional_string(invocation, "verdict")
        .map(|value| substrate::TasteVerdict::from_str(&value).map_err(|error| error.to_string()))
        .transpose()
}

fn optional_relation(
    invocation: &ResolvedInvocation,
) -> Result<Option<substrate::BondRelation>, String> {
    optional_string(invocation, "relation")
        .map(|value| substrate::BondRelation::from_str(&value).map_err(|error| error.to_string()))
        .transpose()
}

fn required_relation(
    invocation: &ResolvedInvocation,
    id: &str,
) -> Result<substrate::BondRelation, String> {
    let value = required_string(invocation, id)?;
    substrate::BondRelation::from_str(&value).map_err(|error| error.to_string())
}

fn optional_dist(invocation: &ResolvedInvocation) -> Result<Option<DistArg>, String> {
    match optional_string(invocation, "dist").as_deref() {
        None => Ok(None),
        Some("archive") => Ok(Some(DistArg::Archive)),
        Some("git") => Ok(Some(DistArg::Git)),
        Some(_) => Err("unsupported --dist value".to_string()),
    }
}

fn optional_vcs(invocation: &ResolvedInvocation) -> Result<Option<VcsArg>, String> {
    match optional_string(invocation, "vcs").as_deref() {
        None => Ok(None),
        Some("git") => Ok(Some(VcsArg::Git)),
        Some("none") => Ok(Some(VcsArg::None)),
        Some(_) => Err("unsupported --vcs value".to_string()),
    }
}

fn optional_direction(invocation: &ResolvedInvocation) -> Result<Option<DirectionArg>, String> {
    match optional_string(invocation, "direction").as_deref() {
        None => Ok(None),
        Some("in") => Ok(Some(DirectionArg::In)),
        Some("out") => Ok(Some(DirectionArg::Out)),
        Some(_) => Err("unsupported --direction value".to_string()),
    }
}

fn install_redirect_or_exit(
    plan: &OutputPlan,
) -> Option<agent_first_data::stream_redirect::InstalledStreamRedirect> {
    let config = match agent_first_data::stream_redirect::StreamRedirectConfig::new(
        plan.stdout_file().map(std::path::Path::to_path_buf),
        plan.stderr_file().map(std::path::Path::to_path_buf),
    ) {
        Ok(config) => config,
        Err(error) => emit_startup_error_or_exit("output_setup_failed", &error.to_string()),
    };
    match config
        .as_ref()
        .map(agent_first_data::stream_redirect::install)
        .transpose()
    {
        Ok(redirect) => redirect,
        Err(error) => emit_startup_error_or_exit("output_setup_failed", &error.to_string()),
    }
}

fn format_of(plan: &OutputPlan) -> OutputFormat {
    plan.format()
        .and_then(|format| cli_parse_output(format).ok())
        .unwrap_or(OutputFormat::Json)
}

fn destination_of(plan: &OutputPlan) -> OutputTo {
    plan.destination()
        .and_then(|destination| OutputTo::parse(destination).ok())
        .unwrap_or(OutputTo::Split)
}

fn raw_destination(plan: &OutputPlan) -> OutputTo {
    if plan.destination() == Some("stderr") {
        OutputTo::Stderr
    } else {
        OutputTo::Stdout
    }
}

fn emit_invalid_invocation_or_exit(message: &str) -> ! {
    let event = match agent_first_data::json_error("cli_invalid_argument_value", message)
        .hint("run `hypha --help` and choose one registered combination")
        .build()
    {
        Ok(event) => event,
        Err(_) => std::process::exit(4),
    };
    emit_event_or_exit(event, OutputFormat::Json, OutputTo::Stderr, 2)
}

fn emit_event_or_exit(
    event: agent_first_data::Event,
    format: OutputFormat,
    destination: OutputTo,
    exit_code: u8,
) -> ! {
    let mut emitter = CliEmitter::from_output_to(destination, format).with_strict_protocol();
    let code = match emitter.emit(event) {
        Ok(()) => exit_code,
        Err(_) => 4,
    };
    std::process::exit(i32::from(code))
}

fn emit_startup_error_or_exit(code: &str, message: &str) -> ! {
    let event = match agent_first_data::json_error(code, message).build() {
        Ok(event) => event,
        Err(_) => std::process::exit(4),
    };
    emit_event_or_exit(event, OutputFormat::Json, OutputTo::Stderr, 1)
}

#[allow(clippy::disallowed_methods)]
fn write_text_or_exit(text: &str, destination: OutputTo) -> ! {
    let result = match destination {
        OutputTo::Stderr => std::io::stderr().lock().write_all(text.as_bytes()),
        OutputTo::Split | OutputTo::Stdout => std::io::stdout().lock().write_all(text.as_bytes()),
    };
    std::process::exit(if result.is_ok() { 0 } else { 4 })
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic)]
mod tests {
    use super::*;
    use agent_first_data::CliOutcome;

    #[test]
    fn every_registered_shape_resolves_and_builds_a_typed_command() {
        let cli = spec::cli_spec().unwrap();
        let app = cli.bind_actions(action_handlers(&cli)).unwrap();
        // Panics naming any combination whose handler reads an id it does not
        // declare; the returned results carry the other half, that every
        // combination `build_cli` accepted actually built.
        for (combination, built) in app.call_every_combination() {
            if let Err(error) = built {
                panic!("{combination} did not build: {error}");
            }
        }
    }

    #[test]
    fn parser_known_relationships_are_closed_combinations() {
        let cli = spec::cli_spec().unwrap();
        for argv in [
            vec!["hypha", "absorb"],
            vec!["hypha", "replicate", "--domain", "example.com"],
            vec![
                "hypha",
                "release",
                "--domain",
                "example.com",
                "--dist-git",
                "https://example.com/repo",
            ],
            vec!["hypha", "mycelium", "status", "--id", "tool"],
            vec![
                "hypha",
                "mycelium",
                "root",
                "example.com",
                "--hub",
                "hub.example",
            ],
        ] {
            let error = cli.resolve_from(argv).unwrap_err();
            assert_eq!(
                error.rule,
                agent_first_data::CliErrorRule::UnregisteredCombination
            );
        }
    }

    #[test]
    fn secret_arguments_are_derived_from_their_names() {
        let cli = spec::cli_spec().unwrap();
        let secrets: Vec<&agent_first_data::ArgSpec> = cli
            .spec()
            .commands
            .iter()
            .flat_map(|command| command.arguments.iter())
            .filter(|argument| argument.argument_id.ends_with("_secret"))
            .collect();
        assert!(!secrets.is_empty());
        assert!(secrets.iter().all(|argument| argument.sensitive));
    }

    #[test]
    fn help_and_docs_are_generated_from_the_same_registry() {
        let cli = spec::cli_spec().unwrap();
        let CliOutcome::Help(help) = cli.resolve_from(["hypha", "release", "--help"]).unwrap()
        else {
            panic!("expected help");
        };
        assert_eq!(help.model().schema, "cli-help-v2");
        assert_eq!(help.model().shapes.len(), 2);
        assert!(help
            .model()
            .shapes
            .iter()
            .any(|shape| shape.usage.contains("--dist-git")));
        let docs = render_cli_reference(&cli);
        assert!(docs.contains("closed `cli-spec-v1` registry"));
        assert!(docs.contains("hypha mycelium serve"));
    }
}
