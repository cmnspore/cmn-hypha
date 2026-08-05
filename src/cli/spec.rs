//! Hypha's closed `cli-spec-v1` registry.
//!
//! This is the single source for argv parsing, typed values, legal argument
//! combinations, output contracts, help-v2, and the generated CLI reference.

use agent_first_data::{
    build_afdata_cli, ArgSpec, BuiltCliSpec, CliSpec, CliSpecError, Combination, CommandSpec,
    OutputSpec,
};

const VERDICTS: [&str; 5] = ["sweet", "fresh", "safe", "rotten", "toxic"];
const DISTRIBUTIONS: [&str; 2] = ["archive", "git"];
const VERSION_CONTROLS: [&str; 2] = ["git", "none"];
const DIRECTIONS: [&str; 2] = ["in", "out"];
const SKILL_AGENTS: [&str; 4] = ["all", "codex", "claude-code", "opencode"];
const SINGLE_SKILL_AGENTS: [&str; 3] = ["codex", "claude-code", "opencode"];
const SKILL_SCOPES: [&str; 2] = ["personal", "project"];

fn finite() -> OutputSpec {
    OutputSpec::protocol_finite(
        ["json", "yaml", "plain"],
        ["split", "stdout", "stderr"],
        "json",
        "split",
    )
    .file_sinks(["stdout", "stderr"])
}

fn streaming() -> OutputSpec {
    OutputSpec::protocol_stream(
        ["json", "yaml", "plain"],
        ["stdout", "stderr"],
        "json",
        "stdout",
    )
    .file_sinks(["stdout", "stderr"])
}

fn lifecycle() -> OutputSpec {
    finite()
}

struct Leaf {
    command: CommandSpec,
    required: Vec<String>,
    optional: Vec<String>,
    output: OutputSpec,
}

impl Leaf {
    fn new<I, S>(path: I, about: &str) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Self {
            command: CommandSpec::new(path).about(about).arg(
                ArgSpec::option("--log", "CATEGORY")
                    .repeatable()
                    .about("Diagnostic category to emit; repeat or use comma-separated values"),
            ),
            required: Vec::new(),
            optional: vec!["log".to_string()],
            output: finite(),
        }
    }

    fn req(mut self, argument: ArgSpec) -> Self {
        self.required.push(argument.argument_id.clone());
        self.command = self.command.arg(argument);
        self
    }

    fn opt(mut self, argument: ArgSpec) -> Self {
        self.optional.push(argument.argument_id.clone());
        self.command = self.command.arg(argument);
        self
    }

    fn stream(mut self) -> Self {
        self.output = streaming();
        self
    }

    fn single(self, _id: &str, action: &str) -> CommandSpec {
        self.command.combination(
            Combination::new(action)
                .action(action)
                .required(self.required)
                .optional(self.optional)
                .output(self.output),
        )
    }

    fn parts(self) -> (CommandSpec, Vec<String>, Vec<String>, OutputSpec) {
        (self.command, self.required, self.optional, self.output)
    }
}

fn synapse() -> ArgSpec {
    ArgSpec::option("--synapse", "DOMAIN_OR_URL").about("Synapse domain or URL")
}

fn synapse_token() -> ArgSpec {
    ArgSpec::option("--synapse-token-secret", "TOKEN")
        .about("Authentication token overriding the configured Synapse token")
}

fn site_path() -> ArgSpec {
    ArgSpec::option("--site-path", "PATH").about("Custom mycelium site directory")
}

pub fn cli_spec() -> Result<BuiltCliSpec, CliSpecError> {
    let mut spec = CliSpec::new("hypha", env!("CARGO_PKG_VERSION"))
        .display_name(env!("DISPLAY_NAME"))
        .about(env!("CARGO_PKG_DESCRIPTION"))
        .lifecycle_output(lifecycle())
        .command(CommandSpec::root())
        .command(sense())
        .command(taste())
        .command(spawn())
        .command(grow())
        .command(absorb())
        .command(bond())
        .command(replicate())
        .command(hatch())
        .command(CommandSpec::new(["hatch", "bond"]).about("Manage bonds in spore.core.json"))
        .command(hatch_bond_set())
        .command(hatch_bond_remove())
        .command(hatch_bond_clear())
        .command(hatch_bond_sync())
        .command(CommandSpec::new(["hatch", "tree"]).about("Manage tree hashing configuration"))
        .command(hatch_tree_set())
        .command(hatch_tree_show())
        .command(release())
        .command(lineage())
        .command(search())
        .command(CommandSpec::new(["mycelium"]).about("Manage a local mycelium site"))
        .command(mycelium_root())
        .command(mycelium_status())
        .command(mycelium_serve())
        .command(
            CommandSpec::new(["mycelium", "nutrient"]).about("Manage nutrient payment methods"),
        )
        .command(mycelium_nutrient_add())
        .command(mycelium_nutrient_remove())
        .command(mycelium_nutrient_clear())
        .command(CommandSpec::new(["mycelium", "spore"]).about("Manage published spore inventory"))
        .command(mycelium_spore_yank())
        .command(mycelium_spore_unyank())
        .command(mycelium_pulse())
        .command(CommandSpec::new(["synapse"]).about("Manage Synapse node connections"))
        .command(synapse_discover())
        .command(synapse_list())
        .command(synapse_health())
        .command(synapse_add())
        .command(synapse_remove())
        .command(synapse_use())
        .command(synapse_config())
        .command(CommandSpec::new(["cache"]).about("Manage the local spore cache"))
        .command(cache_list())
        .command(cache_clean())
        .command(cache_path())
        .command(CommandSpec::new(["config"]).about("View or modify Hypha configuration"))
        .command(config_list())
        .command(config_set())
        .command(CommandSpec::new(["skill"]).about("Manage the bundled Hypha agent skill"))
        .command(skill(
            "status",
            "Inspect whether the bundled skill is current",
        ))
        .command(skill("install", "Install or refresh the bundled skill"))
        .command(skill("uninstall", "Remove the bundled skill"));
    if let Some(build) = Some(env!("GIT_SHA")).filter(|sha| *sha != "unknown") {
        spec = spec.build_id(build);
    }
    build_afdata_cli(spec)
}

fn sense() -> CommandSpec {
    Leaf::new(["sense"], "Resolve a CMN URI and show its metadata")
        .req(ArgSpec::positional("uri", 0, "CMN_URL").about("CMN URI to resolve"))
        .opt(ArgSpec::option("--id", "SPORE_ID").about("Resolve the latest spore with this id"))
        .single("sense", "sense")
}

fn taste() -> CommandSpec {
    Leaf::new(["taste"], "Download a spore for review or record a verdict")
        .req(ArgSpec::positional("uri", 0, "CMN_URL").about("CMN URI to evaluate"))
        .opt(
            ArgSpec::option_enum("--verdict", VERDICTS)
                .value_name("VERDICT")
                .about("Verdict to record"),
        )
        .opt(ArgSpec::option("--notes", "TEXT").about("Notes accompanying the verdict"))
        .opt(synapse())
        .opt(synapse_token())
        .opt(ArgSpec::option("--domain", "DOMAIN").about("Domain used to sign a taste report"))
        .single("taste", "taste")
}

fn spawn() -> CommandSpec {
    Leaf::new(["spawn"], "Create a working copy of a spore")
        .req(ArgSpec::positional("uri", 0, "CMN_URL").about("Spore URI to copy"))
        .opt(ArgSpec::positional("directory", 1, "DIRECTORY").about("Target directory"))
        .opt(
            ArgSpec::option_enum("--vcs", VERSION_CONTROLS)
                .value_name("TYPE")
                .about("Version-control initialization"),
        )
        .opt(
            ArgSpec::option_enum("--dist", DISTRIBUTIONS)
                .value_name("SOURCE")
                .about("Preferred distribution source"),
        )
        .opt(ArgSpec::flag("--bond").about("Fetch the spore's bonds after spawning"))
        .single("spawn", "spawn")
}

fn grow() -> CommandSpec {
    Leaf::new(
        ["grow"],
        "Update a spawned working copy through Synapse lineage",
    )
    .opt(
        ArgSpec::option_enum("--dist", DISTRIBUTIONS)
            .value_name("SOURCE")
            .about("Override the distribution source"),
    )
    .opt(synapse())
    .opt(synapse_token())
    .opt(ArgSpec::flag("--bond").about("Update and fetch bonds too"))
    .single("grow", "grow")
}

fn absorb() -> CommandSpec {
    let leaf = Leaf::new(["absorb"], "Prepare spores for AI-assisted merge")
        .opt(
            ArgSpec::positional("uris", 0, "CMN_URL")
                .repeatable()
                .about("One or more spore URIs"),
        )
        .opt(ArgSpec::flag("--discover").about("Discover descendants through Synapse"))
        .opt(synapse())
        .opt(synapse_token())
        .opt(
            ArgSpec::option_i64("--max-depth", "COUNT")
                .default_i64(10)
                .about("Maximum lineage depth"),
        );
    let (command, _, optional, output) = leaf.parts();
    let common: Vec<String> = optional
        .iter()
        .filter(|id| *id != "uris" && *id != "discover")
        .cloned()
        .collect();
    command
        .combination(
            Combination::new("absorb_explicit")
                .action("absorb")
                .about("Absorb the listed URIs")
                .required(["uris"])
                .optional(common.clone())
                .output(output.clone()),
        )
        .combination(
            Combination::new("absorb_discover")
                .action("absorb")
                .about("Discover descendants and absorb them")
                .required(["discover"])
                .optional(common)
                .output(output),
        )
}

fn bond() -> CommandSpec {
    Leaf::new(["bond"], "Fetch bonds from spore.core.json")
        .opt(ArgSpec::flag("--clean").about("Remove cached bonds no longer declared"))
        .opt(ArgSpec::flag("--status").about("Show status without fetching"))
        .single("bond", "bond")
}

fn replicate() -> CommandSpec {
    let leaf = Leaf::new(
        ["replicate"],
        "Copy spores to another domain without changing hashes",
    )
    .opt(
        ArgSpec::positional("uris", 0, "CMN_URL")
            .repeatable()
            .about("One or more spore URIs"),
    )
    .opt(ArgSpec::flag("--refs").about("Replicate every non-self bond"))
    .req(ArgSpec::option("--domain", "DOMAIN").about("Target publisher domain"))
    .opt(site_path());
    let (command, required, optional, output) = leaf.parts();
    let domain = required;
    let common: Vec<String> = optional
        .iter()
        .filter(|id| *id != "uris" && *id != "refs")
        .cloned()
        .collect();
    command
        .combination(
            Combination::new("replicate_explicit")
                .action("replicate")
                .about("Replicate the listed URIs")
                .required(
                    domain
                        .iter()
                        .cloned()
                        .chain(std::iter::once("uris".to_string())),
                )
                .optional(common.clone())
                .output(output.clone()),
        )
        .combination(
            Combination::new("replicate_refs")
                .action("replicate")
                .about("Replicate every declared non-self bond")
                .required(
                    domain
                        .into_iter()
                        .chain(std::iter::once("refs".to_string())),
                )
                .optional(common)
                .output(output),
        )
}

fn hatch() -> CommandSpec {
    Leaf::new(["hatch"], "Create or update spore.core.json")
        .opt(ArgSpec::option("--id", "ID").about("Opaque spore identifier"))
        .opt(ArgSpec::option("--version", "VERSION").about("Spore version"))
        .opt(ArgSpec::option("--name", "NAME").about("Display name"))
        .opt(ArgSpec::option("--domain", "DOMAIN").about("Publisher domain"))
        .opt(ArgSpec::option("--synopsis", "TEXT").about("Short description"))
        .opt(
            ArgSpec::option("--intent", "TEXT")
                .repeatable()
                .about("Permanent intent; repeat for more entries"),
        )
        .opt(
            ArgSpec::option("--mutations", "TEXT")
                .repeatable()
                .about("Changes from the parent; repeat for more entries"),
        )
        .opt(ArgSpec::option("--license", "SPDX").about("SPDX license identifier"))
        .single("hatch", "hatch")
}

fn hatch_bond_set() -> CommandSpec {
    Leaf::new(["hatch", "bond", "set"], "Add or update one bond by URI")
        .req(ArgSpec::option("--uri", "CMN_URL").about("Bond URI and match key"))
        .opt(ArgSpec::option("--relation", "RELATION").about("Bond relation"))
        .opt(ArgSpec::option("--id", "ID").about("Bond id"))
        .opt(ArgSpec::option("--reason", "TEXT").about("Why this bond exists"))
        .opt(
            ArgSpec::option("--with", "KEY=VALUE")
                .repeatable()
                .about("Bond parameter with a JSON value"),
        )
        .single("set", "hatch_bond_set")
}

fn hatch_bond_remove() -> CommandSpec {
    let leaf = Leaf::new(
        ["hatch", "bond", "remove"],
        "Remove bonds matching a URI, relation, or both",
    )
    .opt(ArgSpec::option("--uri", "CMN_URL").about("URI to match"))
    .opt(ArgSpec::option("--relation", "RELATION").about("Relation to match"));
    let (command, _, optional, output) = leaf.parts();
    let log = optional
        .iter()
        .filter(|id| *id == "log")
        .cloned()
        .collect::<Vec<_>>();
    command
        .combination(
            Combination::new("hatch_bond_remove_uri")
                .action("hatch_bond_remove")
                .about("Match by URI, optionally narrowed by relation")
                .required(["uri"])
                .optional(
                    log.iter()
                        .cloned()
                        .chain(std::iter::once("relation".to_string())),
                )
                .output(output.clone()),
        )
        .combination(
            Combination::new("hatch_bond_remove_relation")
                .action("hatch_bond_remove")
                .about("Match every bond with one relation")
                .required(["relation"])
                .optional(log)
                .output(output),
        )
}

fn hatch_bond_clear() -> CommandSpec {
    Leaf::new(["hatch", "bond", "clear"], "Remove every bond").single("clear", "hatch_bond_clear")
}

fn hatch_bond_sync() -> CommandSpec {
    Leaf::new(
        ["hatch", "bond", "sync"],
        "Reconcile one relation to a declarative JSON spec",
    )
    .req(ArgSpec::option("--relation", "RELATION").about("Relation to reconcile"))
    .req(ArgSpec::option("--spec", "PATH_OR_JSON").about("JSON spec file, inline JSON, or stdin"))
    .opt(ArgSpec::option("--domain", "DOMAIN").about("Domain used to resolve omitted URIs"))
    .opt(site_path())
    .opt(ArgSpec::flag("--check").about("Report drift without writing"))
    .single("sync", "hatch_bond_sync")
}

fn hatch_tree_set() -> CommandSpec {
    Leaf::new(["hatch", "tree", "set"], "Set tree hashing configuration")
        .opt(ArgSpec::option("--algorithm", "ALGORITHM").about("Tree hash algorithm"))
        .opt(
            ArgSpec::option("--exclude-names", "NAME")
                .repeatable()
                .about("File or directory name to exclude"),
        )
        .opt(
            ArgSpec::option("--follow-rules", "FILE")
                .repeatable()
                .about("Ignore-rule file to follow"),
        )
        .single("set", "hatch_tree_set")
}

fn hatch_tree_show() -> CommandSpec {
    Leaf::new(["hatch", "tree", "show"], "Show tree hashing configuration")
        .single("show", "hatch_tree_show")
}

fn release() -> CommandSpec {
    let leaf = Leaf::new(["release"], "Sign and publish a spore")
        .req(ArgSpec::option("--domain", "DOMAIN").about("Target publisher domain"))
        .opt(ArgSpec::option("--source", "PATH").about("Spore source directory"))
        .opt(site_path())
        .opt(ArgSpec::option("--dist-git", "URL").about("External git repository"))
        .opt(ArgSpec::option("--dist-ref", "REF").about("Tag, branch, or commit"))
        .opt(
            ArgSpec::option_enum("--archive", ["zstd"])
                .value_name("FORMAT")
                .default("zstd")
                .about("Generated archive format"),
        )
        .opt(ArgSpec::flag("--dry-run").about("Compute the URI without writing"));
    let (command, required, optional, output) = leaf.parts();
    let common: Vec<String> = optional
        .iter()
        .filter(|id| *id != "dist_git" && *id != "dist_ref")
        .cloned()
        .collect();
    command
        .combination(
            Combination::new("release_archive")
                .action("release")
                .about("Release from local source and archive")
                .required(required.clone())
                .optional(common.clone())
                .output(output.clone()),
        )
        .combination(
            Combination::new("release_git")
                .action("release")
                .about("Release with an external git distribution")
                .required(
                    required
                        .into_iter()
                        .chain(["dist_git".to_string(), "dist_ref".to_string()]),
                )
                .optional(common)
                .output(output),
        )
}

fn lineage() -> CommandSpec {
    Leaf::new(["lineage"], "Trace descendants or ancestors of a spore")
        .req(ArgSpec::positional("uri", 0, "CMN_URL").about("Spore URI"))
        .opt(
            ArgSpec::option_enum("--direction", DIRECTIONS)
                .value_name("DIRECTION")
                .about("in for descendants, out for ancestors"),
        )
        .opt(synapse())
        .opt(synapse_token())
        .opt(
            ArgSpec::option_i64("--max-depth", "COUNT")
                .default_i64(10)
                .about("Maximum traversal depth"),
        )
        .single("lineage", "lineage")
}

fn search() -> CommandSpec {
    Leaf::new(["search"], "Search spores through a Synapse")
        .req(ArgSpec::positional("query", 0, "QUERY").about("Search text"))
        .opt(synapse())
        .opt(synapse_token())
        .opt(ArgSpec::option("--domain", "DOMAIN").about("Filter by publisher domain"))
        .opt(ArgSpec::option("--license", "SPDX").about("Filter by license"))
        .opt(ArgSpec::option("--bonds", "RELATION:URI").about("Comma-separated bond filters"))
        .opt(
            ArgSpec::option_i64("--limit", "COUNT")
                .default_i64(20)
                .about("Maximum number of results"),
        )
        .single("search", "search")
}

fn mycelium_root() -> CommandSpec {
    let leaf = Leaf::new(["mycelium", "root"], "Establish or update a domain site")
        .opt(ArgSpec::positional("domain", 0, "DOMAIN").about("Domain name"))
        .opt(ArgSpec::option("--hub", "DOMAIN").about("Hosted taste hub"))
        .opt(site_path())
        .opt(ArgSpec::option("--name", "NAME").about("Site or author name"))
        .opt(ArgSpec::option("--synopsis", "TEXT").about("Brief site description"))
        .opt(ArgSpec::option("--bio", "MARKDOWN").about("Site or author bio"))
        .opt(ArgSpec::option("--endpoints-base", "URL").about("Base URL for endpoints"));
    let (command, _, optional, output) = leaf.parts();
    let common: Vec<String> = optional
        .iter()
        .filter(|id| *id != "domain" && *id != "hub" && *id != "endpoints_base")
        .cloned()
        .collect();
    command
        .combination(
            Combination::new("mycelium_root_domain")
                .action("mycelium_root")
                .about("Create or update an explicitly named domain")
                .required(["domain"])
                .optional(
                    common
                        .iter()
                        .cloned()
                        .chain(std::iter::once("endpoints_base".to_string())),
                )
                .output(output.clone()),
        )
        .combination(
            Combination::new("mycelium_root_hub")
                .action("mycelium_root")
                .about("Create a taste-only identity under a hosted hub")
                .required(["hub"])
                .optional(common)
                .output(output),
        )
}

fn mycelium_status() -> CommandSpec {
    let leaf = Leaf::new(["mycelium", "status"], "Show local site status")
        .opt(ArgSpec::positional("domain", 0, "DOMAIN").about("Domain name"))
        .opt(site_path())
        .opt(ArgSpec::option("--id", "SPORE_ID").about("Resolve a spore from the inventory"));
    let (command, _, optional, output) = leaf.parts();
    let common: Vec<String> = optional
        .iter()
        .filter(|id| *id != "domain" && *id != "id")
        .cloned()
        .collect();
    command
        .combination(
            Combination::new("mycelium_status_domain")
                .action("mycelium_status")
                .about("List sites or show one domain")
                .optional(
                    common
                        .iter()
                        .cloned()
                        .chain(std::iter::once("domain".to_string())),
                )
                .output(output.clone()),
        )
        .combination(
            Combination::new("mycelium_status_spore")
                .action("mycelium_status")
                .about("Resolve one spore id from one domain")
                .required(["domain", "id"])
                .optional(common)
                .output(output),
        )
}

fn mycelium_serve() -> CommandSpec {
    Leaf::new(
        ["mycelium", "serve"],
        "Serve a local mycelium site over HTTP",
    )
    .opt(ArgSpec::positional("domain", 0, "DOMAIN").about("Domain name"))
    .opt(site_path())
    .opt(
        ArgSpec::option_i64("--port", "PORT")
            .default_i64(8080)
            .about("TCP port"),
    )
    .stream()
    .single("serve", "mycelium_serve")
}

fn mycelium_nutrient_add() -> CommandSpec {
    Leaf::new(
        ["mycelium", "nutrient", "add"],
        "Add or update one nutrient method",
    )
    .req(ArgSpec::positional("domain", 0, "DOMAIN").about("Domain name"))
    .req(ArgSpec::option("--type", "TYPE").about("Nutrient method type"))
    .opt(
        ArgSpec::option("--with", "KEY=VALUE")
            .repeatable()
            .about("Nutrient parameter with a JSON value"),
    )
    .opt(site_path())
    .single("add", "mycelium_nutrient_add")
}

fn mycelium_nutrient_remove() -> CommandSpec {
    Leaf::new(
        ["mycelium", "nutrient", "remove"],
        "Remove one nutrient method",
    )
    .req(ArgSpec::positional("domain", 0, "DOMAIN").about("Domain name"))
    .req(ArgSpec::option("--type", "TYPE").about("Nutrient method type"))
    .opt(site_path())
    .single("remove", "mycelium_nutrient_remove")
}

fn mycelium_nutrient_clear() -> CommandSpec {
    Leaf::new(
        ["mycelium", "nutrient", "clear"],
        "Remove every nutrient method",
    )
    .req(ArgSpec::positional("domain", 0, "DOMAIN").about("Domain name"))
    .opt(site_path())
    .single("clear", "mycelium_nutrient_clear")
}

fn mycelium_spore_yank() -> CommandSpec {
    Leaf::new(
        ["mycelium", "spore", "yank"],
        "Delist a spore while retaining published bytes",
    )
    .req(ArgSpec::option("--id", "SPORE_ID").about("Spore id to delist"))
    .opt(ArgSpec::option("--domain", "DOMAIN").about("Domain used to locate the site"))
    .opt(site_path())
    .opt(ArgSpec::flag("--purge").about("Delete the published manifest and archive too"))
    .single("yank", "mycelium_spore_yank")
}

fn mycelium_spore_unyank() -> CommandSpec {
    Leaf::new(
        ["mycelium", "spore", "unyank"],
        "Restore a delisted spore from its retained manifest",
    )
    .req(ArgSpec::option("--id", "SPORE_ID").about("Spore id to restore"))
    .opt(ArgSpec::option("--domain", "DOMAIN").about("Domain used to locate the site"))
    .opt(site_path())
    .single("unyank", "mycelium_spore_unyank")
}

fn mycelium_pulse() -> CommandSpec {
    Leaf::new(["mycelium", "pulse"], "Send a signed mycelium to a Synapse")
        .req(ArgSpec::option("--file", "PATH").about("Signed mycelium JSON file"))
        .opt(synapse())
        .opt(synapse_token())
        .single("pulse", "mycelium_pulse")
}

fn synapse_discover() -> CommandSpec {
    Leaf::new(["synapse", "discover"], "Discover Synapse instances")
        .opt(synapse())
        .opt(synapse_token())
        .single("discover", "synapse_discover")
}

fn synapse_list() -> CommandSpec {
    Leaf::new(["synapse", "list"], "List configured Synapse nodes").single("list", "synapse_list")
}

fn synapse_health() -> CommandSpec {
    Leaf::new(["synapse", "health"], "Check one Synapse instance")
        .opt(ArgSpec::positional("synapse", 0, "DOMAIN_OR_URL").about("Synapse domain or URL"))
        .opt(synapse_token())
        .single("health", "synapse_health")
}

fn synapse_add() -> CommandSpec {
    Leaf::new(["synapse", "add"], "Add a Synapse node")
        .req(ArgSpec::positional("url", 0, "URL").about("Synapse URL"))
        .single("add", "synapse_add")
}

fn synapse_remove() -> CommandSpec {
    Leaf::new(["synapse", "remove"], "Remove a Synapse node")
        .req(ArgSpec::positional("domain", 0, "DOMAIN").about("Synapse domain"))
        .single("remove", "synapse_remove")
}

fn synapse_use() -> CommandSpec {
    Leaf::new(["synapse", "use"], "Select the default Synapse node")
        .req(ArgSpec::positional("domain", 0, "DOMAIN").about("Synapse domain"))
        .single("use", "synapse_use")
}

fn synapse_config() -> CommandSpec {
    Leaf::new(
        ["synapse", "config"],
        "Configure credentials for one Synapse",
    )
    .req(ArgSpec::positional("domain", 0, "DOMAIN").about("Synapse domain"))
    .opt(ArgSpec::option("--token-secret", "TOKEN").about("Token, or an empty string to clear"))
    .single("config", "synapse_config")
}

fn cache_list() -> CommandSpec {
    Leaf::new(["cache", "list"], "List cached spores").single("list", "cache_list")
}

fn cache_clean() -> CommandSpec {
    Leaf::new(["cache", "clean"], "Remove old or all cached items")
        .opt(ArgSpec::flag("--all").about("Remove every cached item"))
        .single("clean", "cache_clean")
}

fn cache_path() -> CommandSpec {
    Leaf::new(["cache", "path"], "Show a cached spore's filesystem path")
        .req(ArgSpec::positional("uri", 0, "CMN_URL").about("Spore URI"))
        .single("path", "cache_path")
}

fn config_list() -> CommandSpec {
    Leaf::new(["config", "list"], "Show merged Hypha configuration").single("list", "config_list")
}

fn config_set() -> CommandSpec {
    Leaf::new(["config", "set"], "Set one dotted configuration key")
        .req(ArgSpec::positional("key", 0, "KEY").about("Dotted configuration path"))
        .req(ArgSpec::positional("value", 1, "VALUE").about("Value to store"))
        .single("set", "config_set")
}

fn skill(action: &str, about: &str) -> CommandSpec {
    let leaf = Leaf::new(["skill", action], about)
        .opt(
            ArgSpec::option_enum("--agent", SKILL_AGENTS)
                .value_name("AGENT")
                .default("all")
                .about("Agent target"),
        )
        .opt(
            ArgSpec::option_enum("--scope", SKILL_SCOPES)
                .value_name("SCOPE")
                .default("personal")
                .about("Install scope"),
        )
        .opt(ArgSpec::option("--skills-dir", "PATH").about("Explicit skills directory"))
        .opt(ArgSpec::flag("--force").about("Replace an unmanaged target"));
    let (command, _, optional, output) = leaf.parts();
    let common: Vec<String> = optional
        .iter()
        .filter(|id| *id != "skills_dir" && *id != "agent")
        .cloned()
        .collect();
    let action_id = format!("skill_{action}");
    command
        .combination(
            Combination::new(format!("skill_{action}_standard"))
                .action(action_id.clone())
                .about("Use the selected agent's standard skills directory")
                .optional(
                    common
                        .iter()
                        .cloned()
                        .chain(std::iter::once("agent".to_string())),
                )
                .output(output.clone()),
        )
        .combination(
            Combination::new(format!("skill_{action}_explicit_directory"))
                .action(action_id)
                .about("Use an explicit directory for one concrete agent")
                .fixed_one_of("agent", SINGLE_SKILL_AGENTS)
                .required(["skills_dir"])
                .optional(common)
                .output(output),
        )
}
