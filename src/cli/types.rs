//! Typed commands produced by the closed-world CLI registry.

use serde::Serialize;

#[derive(Serialize)]
pub struct Cli {
    pub output: String,
    pub output_to: String,
    pub log: Vec<String>,
    pub command: Commands,
}

#[derive(Serialize)]
#[serde(tag = "command", rename_all = "snake_case")]
pub enum Commands {
    Sense {
        #[serde(rename = "cmn_url")]
        uri: String,
        id: Option<String>,
    },
    Taste {
        #[serde(rename = "cmn_url")]
        uri: String,
        verdict: Option<substrate::TasteVerdict>,
        notes: Option<String>,
        #[serde(rename = "synapse_selector")]
        synapse: Option<String>,
        synapse_token_secret: Option<String>,
        domain: Option<String>,
    },
    Spawn {
        #[serde(rename = "cmn_url")]
        uri: String,
        directory: Option<String>,
        vcs: Option<VcsArg>,
        dist: Option<DistArg>,
        bond: bool,
    },
    Grow {
        dist: Option<DistArg>,
        #[serde(rename = "synapse_selector")]
        synapse: Option<String>,
        synapse_token_secret: Option<String>,
        bond: bool,
    },
    Absorb {
        uris: Vec<String>,
        discover: bool,
        #[serde(rename = "synapse_selector")]
        synapse: Option<String>,
        synapse_token_secret: Option<String>,
        max_depth: u32,
    },
    Bond {
        clean: bool,
        status: bool,
    },
    Replicate {
        uris: Vec<String>,
        refs: bool,
        domain: String,
        site_path: Option<String>,
    },
    Hatch {
        id: Option<String>,
        version: Option<String>,
        name: Option<String>,
        domain: Option<String>,
        synopsis: Option<String>,
        intent: Vec<String>,
        mutations: Vec<String>,
        license: Option<String>,
        #[serde(skip)]
        command: Option<HatchCommands>,
    },
    Release {
        domain: String,
        source: Option<String>,
        site_path: Option<String>,
        #[serde(rename = "dist_git_url")]
        dist_git: Option<String>,
        dist_ref: Option<String>,
        archive: String,
        dry_run: bool,
    },
    Lineage {
        #[serde(rename = "cmn_url")]
        uri: String,
        direction: Option<DirectionArg>,
        #[serde(rename = "synapse_selector")]
        synapse: Option<String>,
        synapse_token_secret: Option<String>,
        max_depth: u32,
    },
    Search {
        query: String,
        #[serde(rename = "synapse_selector")]
        synapse: Option<String>,
        synapse_token_secret: Option<String>,
        domain: Option<String>,
        license: Option<String>,
        bonds: Option<String>,
        limit: u32,
    },
    Mycelium {
        #[serde(flatten)]
        action: MyceliumAction,
    },
    Synapse {
        #[serde(flatten)]
        action: SynapseAction,
    },
    Cache {
        #[serde(flatten)]
        action: CacheAction,
    },
    Config {
        #[serde(flatten)]
        action: ConfigAction,
    },
    Skill {
        #[serde(flatten)]
        action: SkillCommand,
    },
}

#[derive(Serialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum SkillCommand {
    Status(SkillOptionsArg),
    Install(SkillOptionsArg),
    Uninstall(SkillOptionsArg),
}

#[derive(Serialize)]
pub struct SkillOptionsArg {
    pub agent: SkillAgentArg,
    pub scope: SkillScopeArg,
    pub skills_dir: Option<String>,
    pub force: bool,
}

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum SkillAgentArg {
    All,
    Codex,
    ClaudeCode,
    Opencode,
}

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum SkillScopeArg {
    Personal,
    Project,
}

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum DistArg {
    Archive,
    Git,
}

impl DistArg {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Archive => "archive",
            Self::Git => "git",
        }
    }
}

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum VcsArg {
    Git,
    None,
}

impl VcsArg {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Git => "git",
            Self::None => "none",
        }
    }
}

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum DirectionArg {
    In,
    Out,
}

impl DirectionArg {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::In => "in",
            Self::Out => "out",
        }
    }
}

#[derive(Serialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum HatchCommands {
    Bond {
        #[serde(flatten)]
        command: HatchBondCommands,
    },
    Tree {
        #[serde(flatten)]
        command: HatchTreeCommands,
    },
}

#[derive(Serialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum HatchBondCommands {
    Set {
        #[serde(rename = "cmn_url")]
        uri: String,
        relation: Option<substrate::BondRelation>,
        id: Option<String>,
        reason: Option<String>,
        with_entries: Vec<String>,
    },
    Remove {
        #[serde(rename = "cmn_url")]
        uri: Option<String>,
        relation: Option<substrate::BondRelation>,
    },
    Clear,
    Sync {
        relation: substrate::BondRelation,
        spec: String,
        domain: Option<String>,
        site_path: Option<String>,
        check: bool,
    },
}

#[derive(Serialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum HatchTreeCommands {
    Set {
        algorithm: Option<String>,
        exclude_names: Option<Vec<String>>,
        follow_rules: Option<Vec<String>>,
    },
    Show,
}

#[derive(Serialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum MyceliumAction {
    Root {
        domain: Option<String>,
        hub: Option<String>,
        site_path: Option<String>,
        name: Option<String>,
        synopsis: Option<String>,
        bio: Option<String>,
        #[serde(rename = "endpoints_base_url")]
        endpoints_base: Option<String>,
    },
    Status {
        domain: Option<String>,
        site_path: Option<String>,
        id: Option<String>,
    },
    Serve {
        domain: Option<String>,
        site_path: Option<String>,
        port: u16,
    },
    Nutrient {
        #[serde(flatten)]
        command: NutrientCommands,
    },
    Spore {
        #[serde(flatten)]
        command: SporeCommands,
    },
    Pulse {
        #[serde(rename = "synapse_selector")]
        synapse: Option<String>,
        synapse_token_secret: Option<String>,
        file: String,
    },
}

#[derive(Serialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum NutrientCommands {
    Add {
        domain: String,
        method_type: String,
        with_entries: Vec<String>,
        site_path: Option<String>,
    },
    Remove {
        domain: String,
        method_type: String,
        site_path: Option<String>,
    },
    Clear {
        domain: String,
        site_path: Option<String>,
    },
}

#[derive(Serialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum SporeCommands {
    Yank {
        id: String,
        domain: Option<String>,
        site_path: Option<String>,
        purge: bool,
    },
    Unyank {
        id: String,
        domain: Option<String>,
        site_path: Option<String>,
    },
}

#[derive(Serialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum SynapseAction {
    Discover {
        #[serde(rename = "synapse_selector")]
        synapse: Option<String>,
        synapse_token_secret: Option<String>,
    },
    List,
    Health {
        #[serde(rename = "synapse_selector")]
        synapse: Option<String>,
        synapse_token_secret: Option<String>,
    },
    Add {
        #[serde(rename = "synapse_url")]
        url: String,
    },
    Remove {
        domain: String,
    },
    Use {
        domain: String,
    },
    Config {
        domain: String,
        token_secret: Option<String>,
    },
}

#[derive(Serialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum CacheAction {
    List,
    Clean {
        all: bool,
    },
    Path {
        #[serde(rename = "cmn_url")]
        uri: String,
    },
}

#[derive(Serialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum ConfigAction {
    List,
    Set { key: String, value: String },
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::{Commands, MyceliumAction};

    #[test]
    fn startup_diagnostic_fields_name_known_urls_canonically() {
        let release = serde_json::to_value(Commands::Release {
            domain: "example.com".to_string(),
            source: None,
            site_path: None,
            dist_git: Some(
                "https://user:password@example.com/repo?token_secret=canary".to_string(),
            ),
            dist_ref: None,
            archive: "zstd".to_string(),
            dry_run: true,
        })
        .unwrap();
        assert!(release.get("dist_git_url").is_some());
        assert!(release.get("dist_git").is_none());
        let release = agent_first_data::redacted_value(&release);
        let rendered = serde_json::to_string(&release).unwrap();
        assert!(!rendered.contains("password"));
        assert!(!rendered.contains("canary"));

        let mycelium = serde_json::to_value(MyceliumAction::Root {
            domain: None,
            hub: None,
            site_path: None,
            name: None,
            synopsis: None,
            bio: None,
            endpoints_base: Some(
                "https://user:password@example.com?token_secret=canary".to_string(),
            ),
        })
        .unwrap();
        assert!(mycelium.get("endpoints_base_url").is_some());
        assert!(mycelium.get("endpoints_base").is_none());
        let mycelium = agent_first_data::redacted_value(&mycelium);
        let rendered = serde_json::to_string(&mycelium).unwrap();
        assert!(!rendered.contains("password"));
        assert!(!rendered.contains("canary"));
    }
}
