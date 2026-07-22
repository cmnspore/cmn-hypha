use std::process::ExitCode;

use agent_first_data::skill::{
    run_skill_admin, SkillAction as AfSkillAction, SkillAgentSelection, SkillOptions, SkillScope,
    SkillSpec,
};

use crate::api::Output;
use crate::cli::{SkillAgentArg, SkillCommand, SkillOptionsArg, SkillScopeArg};

const HYPHA_SKILL: &str = include_str!("../../skills/cmn-hypha/SKILL.md");

fn spec() -> SkillSpec<'static> {
    SkillSpec {
        name: "cmn-hypha",
        source: HYPHA_SKILL,
        title: "CMN Hypha",
        marker_slug: "cmn-hypha",
        assets: &[],
    }
}

pub fn handle_skill(out: &Output, command: SkillCommand) -> ExitCode {
    let (action, options) = match command {
        SkillCommand::Status(options) => (AfSkillAction::Status, options),
        SkillCommand::Install(options) => (AfSkillAction::Install, options),
        SkillCommand::Uninstall(options) => (AfSkillAction::Uninstall, options),
    };

    let options = to_afdata_options(options);
    match run_skill_admin(&spec(), action, &options) {
        Ok(report) => out.ok(report),
        Err(err) => out.error_hint(
            "skill_error",
            &err.message,
            err.hint.as_deref().or(Some(
                "check hypha skill --help and retry with the suggested options",
            )),
        ),
    }
}

fn to_afdata_options(options: SkillOptionsArg) -> SkillOptions {
    SkillOptions {
        agent: match options.agent {
            SkillAgentArg::All => SkillAgentSelection::All,
            SkillAgentArg::Codex => SkillAgentSelection::Codex,
            SkillAgentArg::ClaudeCode => SkillAgentSelection::ClaudeCode,
            SkillAgentArg::Opencode => SkillAgentSelection::Opencode,
        },
        scope: match options.scope {
            SkillScopeArg::Personal => SkillScope::Personal,
            SkillScopeArg::Project => SkillScope::Workspace,
        },
        skills_dir: options.skills_dir,
        force: options.force,
    }
}
