//! Single source of truth for Hypha's CLI shape and user-facing help text.
//!
//! Keep command descriptions, examples, and argument docs here so both runtime
//! help and generated reference docs stay aligned.

use clap::{CommandFactory, Parser, Subcommand};
use serde::Serialize;

#[derive(Parser)]
#[command(name = "hypha")]
#[command(version)]
#[command(about = "CMN Client - A bio-digital extension for Visitors to release and absorb Spores")]
#[command(after_long_help = concat!(
    "All output follows Agent-First Data format:\n",
    "  {\"code\": \"ok\", \"result\": {...}, \"trace\": {...}}\n",
    "\n",
    "Quick start (try with cmn.dev):\n",
    "  hypha sense cmn://cmn.dev\n",
    "  hypha sense cmn://cmn.dev/", "b3.3yMR7vZQ9hL2xKJdFtN8wPcB6sY1mXgU4eH5pTa2", "\n",
    "  hypha spawn cmn://cmn.dev/", "b3.3yMR7vZQ9hL2xKJdFtN8wPcB6sY1mXgU4eH5pTa2", "\n",
    "  hypha cache list",
))]
pub struct Cli {
    /// Output format
    #[arg(short, long, default_value = "json", global = true)]
    pub output: String,

    /// Log categories (comma-separated): startup, request, ...
    #[arg(long, value_delimiter = ',', global = true)]
    pub log: Vec<String>,

    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand, Serialize)]
#[serde(tag = "command", rename_all = "snake_case")]
pub enum Commands {
    // ═══════════════════════════════════════════
    // Spore Lifecycle Commands (top-level)
    // ═══════════════════════════════════════════
    /// Resolve a CMN URI and show metadata without downloading
    #[command(after_long_help = concat!(
        "URI types:\n",
        "  cmn://DOMAIN                       List all spores on a site\n",
        "  cmn://DOMAIN/HASH                  View a specific spore\n",
        "\n",
        "Examples:\n",
        "  hypha sense cmn://cmn.dev\n",
        "  hypha sense cmn://cmn.dev/", "b3.3yMR7vZQ9hL2xKJdFtN8wPcB6sY1mXgU4eH5pTa2", "\n",
        "  hypha sense cmn://cmn.dev -o yaml",
    ))]
    Sense {
        /// CMN URI (cmn://DOMAIN or cmn://DOMAIN/HASH)
        uri: String,
    },

    /// Evaluate spore: download for review, or record a verdict
    #[command(after_long_help = concat!(
        "Without --verdict: downloads the spore for local review.\n",
        "With --verdict: records a verdict (sweet, fresh, safe, rotten, toxic).\n",
        "\n",
        "Examples:\n",
        "  hypha taste cmn://cmn.dev/", "b3.3yMR7vZQ9hL2xKJdFtN8wPcB6sY1mXgU4eH5pTa2", "\n",
        "  hypha taste cmn://cmn.dev/", "b3.3yMR7vZQ9hL2xKJdFtN8wPcB6sY1mXgU4eH5pTa2",
            " --verdict safe --notes \"Reviewed: clean code\"\n",
        "  hypha taste cmn://cmn.dev/", "b3.3yMR7vZQ9hL2xKJdFtN8wPcB6sY1mXgU4eH5pTa2",
            " --verdict safe --domain cmn.dev --synapse https://synapse.cmn.dev",
    ))]
    Taste {
        /// CMN URI (e.g., cmn://cmn.dev/HASH)
        uri: String,
        /// Record verdict: sweet, fresh, safe, rotten, or toxic
        #[arg(long, value_name = "VERDICT")]
        verdict: Option<substrate::TasteVerdict>,
        /// Notes about the verdict
        #[arg(long)]
        notes: Option<String>,
        /// Synapse URL to pull/share taste reports
        #[arg(long)]
        synapse: Option<String>,
        /// Auth token for synapse (overrides configured token)
        #[arg(long)]
        synapse_token_secret: Option<String>,
        /// Domain to sign report with (requires --synapse and --verdict)
        #[arg(long)]
        domain: Option<String>,
    },

    /// Create a working copy of a spore (auto-detects best distribution format)
    #[command(after_long_help = concat!(
        "Distribution sources (auto-detected):\n",
        "  archive    Download .tar.zst archive (default, fastest)\n",
        "  git        Clone from dist.git URL if available\n",
        "\n",
        "Examples:\n",
        "  hypha spawn cmn://cmn.dev/", "b3.3yMR7vZQ9hL2xKJdFtN8wPcB6sY1mXgU4eH5pTa2", "\n",
        "  hypha spawn cmn://cmn.dev/", "b3.3yMR7vZQ9hL2xKJdFtN8wPcB6sY1mXgU4eH5pTa2",
            " my-project --vcs git\n",
        "  hypha spawn cmn://cmn.dev/", "b3.3yMR7vZQ9hL2xKJdFtN8wPcB6sY1mXgU4eH5pTa2",
            " --dist git",
    ))]
    Spawn {
        /// CMN URI (e.g., cmn://cmn.dev/HASH)
        uri: String,
        /// Target directory (default: ./<spore-id>)
        directory: Option<String>,
        /// Initialize version control after spawn (e.g., --vcs git)
        #[arg(long, value_name = "TYPE")]
        vcs: Option<String>,
        /// Preferred distribution source: archive (default) or git
        #[arg(long, value_name = "SOURCE")]
        dist: Option<String>,
        /// Fetch bonds after spawn
        #[arg(long)]
        bond: bool,
    },

    /// Pull latest changes from spawn source via Synapse lineage
    #[command(after_long_help = "\
Run inside a previously spawned directory to update it.
Uses Synapse lineage to discover newer versions from the same publisher.

If local files have been modified (git dirty or tree hash mismatch),
grow refuses to overwrite them and shows the cache path for manual merge.

Examples:
  hypha grow
  hypha grow --synapse synapse.cmn.dev
  hypha grow --dist git
  hypha grow --dist archive
  hypha grow --bond --synapse synapse.cmn.dev")]
    Grow {
        /// Override distribution source: archive or git
        #[arg(long, value_name = "SOURCE")]
        dist: Option<String>,
        /// Synapse to query for updates (domain or URL)
        #[arg(long)]
        synapse: Option<String>,
        /// Auth token for synapse (overrides configured token)
        #[arg(long)]
        synapse_token_secret: Option<String>,
        /// Also check depends_on/follows/extends bonds for updates via Synapse lineage, and fetch all bonds to .cmn/bonds/
        #[arg(long)]
        bond: bool,
    },

    /// Prepare spores for AI-assisted merge
    #[command(after_long_help = concat!(
        "Absorb downloads spores into .cmn/absorb/ for AI-assisted merge.\n",
        "Use --discover to auto-discover descendants via Synapse.\n",
        "\n",
        "Examples:\n",
        "  hypha absorb cmn://cmn.dev/", "b3.3yMR7vZQ9hL2xKJdFtN8wPcB6sY1mXgU4eH5pTa2", "\n",
        "  hypha absorb --discover --synapse https://synapse.cmn.dev\n",
        "  hypha absorb --discover --synapse https://synapse.cmn.dev --max-depth 5",
    ))]
    Absorb {
        /// CMN URIs to absorb (e.g., cmn://cmn.dev/HASH)
        #[arg(required_unless_present = "discover")]
        uris: Vec<String>,
        /// Auto-discover descendants from current spore's spawned_from bonds
        #[arg(long)]
        discover: bool,
        /// Synapse server URL (required with --discover)
        #[arg(long)]
        synapse: Option<String>,
        /// Auth token for synapse (overrides configured token)
        #[arg(long)]
        synapse_token_secret: Option<String>,
        /// Maximum depth for lineage discovery (default: 10)
        #[arg(long, default_value = "10")]
        max_depth: u32,
    },

    /// Fetch all bonds from spore.core.json to .cmn/bonds/
    #[command(after_long_help = "\
Examples:
  hypha bond
  hypha bond --status
  hypha bond --clean")]
    Bond {
        /// Clean orphaned bonds not in spore.core.json
        #[arg(long)]
        clean: bool,
        /// Show bond status without fetching
        #[arg(long)]
        status: bool,
    },

    /// Copy a spore to your domain (same hash, re-signed capsule)
    #[command(after_long_help = "\
Replicates spores from another domain to yours. The hash stays the same
because core + core_signature are preserved. Only capsule_signature changes.

Examples:
  hypha replicate cmn://other.dev/HASH --domain my.dev
  hypha replicate --refs --domain my.dev")]
    Replicate {
        /// CMN URI(s) to replicate
        #[arg(required_unless_present = "refs")]
        uris: Vec<String>,
        /// Replicate all non-self bonds from spore.core.json
        #[arg(long)]
        refs: bool,
        /// Target domain (required)
        #[arg(long)]
        domain: String,
        /// Custom site directory
        #[arg(long)]
        site_path: Option<String>,
    },

    /// Create or update spore.core.json in working directory
    #[command(after_long_help = "\
Examples:
  hypha hatch --id my-tool --name \"My Tool\" --synopsis \"A useful tool\"
  hypha hatch --intent \"Provide a reusable HTTP client for CMN agents\" --mutations \"Initial release\"
  hypha hatch --license MIT --domain cmn.dev

Subcommands:
  hypha hatch bond set/remove/clear   Manage bonds in spore.core.json
  hypha hatch tree set/show            Manage tree configuration")]
    #[command(args_conflicts_with_subcommands = true)]
    Hatch {
        /// Opaque identifier stored in spore.core.json
        #[arg(long)]
        id: Option<String>,
        /// Version string (e.g., 1.0.0)
        #[arg(long)]
        version: Option<String>,
        /// Display name
        #[arg(long)]
        name: Option<String>,
        /// Publisher domain
        #[arg(long)]
        domain: Option<String>,
        /// Short description
        #[arg(long)]
        synopsis: Option<String>,
        /// Why this spore exists — permanent across releases (repeatable)
        #[arg(long)]
        intent: Vec<String>,
        /// What changed relative to the spawned-from parent (repeatable)
        #[arg(long)]
        mutations: Vec<String>,
        /// License (SPDX identifier)
        #[arg(long)]
        license: Option<String>,

        #[command(subcommand)]
        #[serde(skip)]
        command: Option<HatchCommands>,
    },

    /// Sign and publish spore to mycelium site
    #[command(after_long_help = "\
Requires `hypha mycelium root` first to set up the site.

Examples:
  hypha release --domain cmn.dev
  hypha release --domain cmn.dev --source ./my-spore
  hypha release --domain cmn.dev --dry-run          # pre-compute URI without releasing
  hypha release --domain cmn.dev --archive zstd
  hypha release --domain cmn.dev --dist-git https://github.com/user/repo --dist-ref v1.0")]
    Release {
        /// Target domain (required)
        #[arg(long)]
        domain: String,
        /// Spore source directory (default: current directory)
        #[arg(long)]
        source: Option<String>,
        /// Custom site directory (default: ~/.cmn/mycelium/<domain>)
        #[arg(long)]
        site_path: Option<String>,
        /// External git repository URL
        #[arg(long)]
        dist_git: Option<String>,
        /// Git ref: tag/branch/commit (requires --dist-git)
        #[arg(long)]
        dist_ref: Option<String>,
        /// Archive format for release generation (currently only: zstd)
        #[arg(long, value_name = "FORMAT", default_value = "zstd")]
        archive: String,
        /// Pre-compute URI without writing any files
        #[arg(long)]
        dry_run: bool,
    },

    // ═══════════════════════════════════════════
    // Discovery Commands
    // ═══════════════════════════════════════════
    /// Trace spore lineage: descendants (in, default) or ancestors (out)
    #[command(after_long_help = concat!(
        "Direction:\n",
        "  --direction in   Find descendants / forks (default)\n",
        "  --direction out  Trace ancestors / spawn chain\n",
        "\n",
        "Examples:\n",
        "  hypha lineage cmn://cmn.dev/", "b3.3yMR7vZQ9hL2xKJdFtN8wPcB6sY1mXgU4eH5pTa2",
            " --synapse https://synapse.cmn.dev\n",
        "  hypha lineage cmn://cmn.dev/", "b3.3yMR7vZQ9hL2xKJdFtN8wPcB6sY1mXgU4eH5pTa2",
            " --direction out --synapse https://synapse.cmn.dev\n",
        "  hypha lineage cmn://cmn.dev/", "b3.3yMR7vZQ9hL2xKJdFtN8wPcB6sY1mXgU4eH5pTa2",
            " --synapse https://synapse.cmn.dev --max-depth 5",
    ))]
    Lineage {
        /// CMN URI (e.g., cmn://cmn.dev/HASH)
        uri: String,
        /// Direction: in (descendants, default) or out (ancestors)
        #[arg(long)]
        direction: Option<String>,
        /// Synapse server (domain or URL, default: configured default)
        #[arg(long)]
        synapse: Option<String>,
        /// Auth token for synapse (overrides configured token)
        #[arg(long)]
        synapse_token_secret: Option<String>,
        /// Maximum traversal depth (default: 10)
        #[arg(long, default_value = "10")]
        max_depth: u32,
    },

    /// Search for spores by keyword (semantic search via Synapse)
    #[command(after_long_help = "\
Examples:
  hypha search \"protocol spec\" --synapse https://synapse.cmn.dev
  hypha search \"data format\" --synapse https://synapse.cmn.dev --domain cmn.dev
  hypha search \"agent tools\" --synapse https://synapse.cmn.dev --license MIT --limit 5
  hypha search \"http client\" --bonds spawned_from:cmn://cmn.dev/b3.abc123")]
    Search {
        /// Search query text
        query: String,
        /// Synapse server (domain or URL, default: configured default)
        #[arg(long)]
        synapse: Option<String>,
        /// Auth token for synapse (overrides configured token)
        #[arg(long)]
        synapse_token_secret: Option<String>,
        /// Filter by domain
        #[arg(long)]
        domain: Option<String>,
        /// Filter by license (SPDX identifier)
        #[arg(long)]
        license: Option<String>,
        /// Filter by bond relationship (format: relation:uri, comma-separated for AND)
        #[arg(long)]
        bonds: Option<String>,
        /// Maximum results (default: 20)
        #[arg(long, default_value = "20")]
        limit: u32,
    },

    // ═══════════════════════════════════════════
    // Infrastructure Commands
    // ═══════════════════════════════════════════
    /// Manage local mycelium site
    Mycelium {
        #[command(subcommand)]
        #[serde(flatten)]
        action: MyceliumAction,
    },

    /// Manage Synapse node connections
    Synapse {
        #[command(subcommand)]
        #[serde(flatten)]
        action: SynapseAction,
    },

    /// Manage local cache
    Cache {
        #[command(subcommand)]
        #[serde(flatten)]
        action: CacheAction,
    },

    /// View or modify hypha configuration
    Config {
        #[command(subcommand)]
        #[serde(flatten)]
        action: ConfigAction,
    },
}

#[derive(Subcommand, Serialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum HatchCommands {
    /// Manage bonds in spore.core.json
    #[command(after_long_help = "\
Examples:
  hypha hatch bond set --uri cmn://cmn.dev/b3.abc --relation follows --id my-lib --reason \"Core library\"
  hypha hatch bond set --uri cmn://cmn.dev/b3.abc --with 'mints=[\"https://mint.example.com\"]'
  hypha hatch bond remove --relation follows
  hypha hatch bond clear")]
    Bond {
        #[command(subcommand)]
        #[serde(flatten)]
        command: HatchBondCommands,
    },
    /// Manage tree configuration in spore.core.json
    Tree {
        #[command(subcommand)]
        #[serde(flatten)]
        command: HatchTreeCommands,
    },
}

#[derive(Subcommand, Serialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum HatchBondCommands {
    /// Add or update a bond (upsert by URI)
    Set {
        /// Bond URI (match key)
        #[arg(long)]
        uri: String,
        /// Bond relation (required for new bonds)
        #[arg(long)]
        relation: Option<substrate::BondRelation>,
        /// Bond id
        #[arg(long)]
        id: Option<String>,
        /// Bond reason
        #[arg(long)]
        reason: Option<String>,
        /// Bond parameters (KEY=VALUE, value is parsed as JSON; repeatable)
        #[arg(long = "with", value_name = "KEY=VALUE")]
        with_entries: Vec<String>,
    },
    /// Remove bonds by URI and/or relation
    Remove {
        /// Remove bonds matching this URI
        #[arg(long)]
        uri: Option<String>,
        /// Remove bonds matching this relation
        #[arg(long)]
        relation: Option<substrate::BondRelation>,
    },
    /// Remove all bonds
    Clear,
}

#[derive(Subcommand, Serialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum HatchTreeCommands {
    /// Set tree configuration fields
    Set {
        /// Hash algorithm (e.g., blob_tree_blake3_nfc)
        #[arg(long)]
        algorithm: Option<String>,
        /// File/directory names to exclude from hashing (repeatable)
        #[arg(long, num_args = 1..)]
        exclude_names: Option<Vec<String>>,
        /// Ignore-rule files to follow (repeatable)
        #[arg(long, num_args = 1..)]
        follow_rules: Option<Vec<String>>,
    },
    /// Show current tree configuration
    Show,
}

#[derive(Subcommand, Serialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum MyceliumAction {
    /// Establish a new site for a domain (or update existing)
    #[command(after_long_help = "\
Creates ~/.cmn/mycelium/<domain>/ with key pair and site structure.
Run this once before `hypha release`.

With --hub, creates a taste-only account on a hosted hub (e.g. cmnhub.com):
  1. Generates ed25519 key pair
  2. Computes subdomain from pubkey (ed-<base32>.hub)
  3. Creates taste-only cmn.json with taste endpoint
  4. Registers hub as a synapse node
  5. Sets [defaults.taste] so `hypha taste` auto-submits

After --hub, register with the hub then taste without extra flags:
  curl -X POST https://cmnhub.com/synapse/pulse -H 'Content-Type: application/json' \\
    -d @~/.cmn/mycelium/ed-xxx.cmnhub.com/public/.well-known/cmn.json
  hypha taste cmn://example.com/b3.HASH --verdict safe

Examples:
  hypha mycelium root cmn.dev --name \"CMN\" --synopsis \"Code Mycelial Network\"
  hypha mycelium root cmn.dev --endpoints-base https://cmn.dev
  hypha mycelium root example.com --site-path /custom/path
  hypha mycelium root --hub cmnhub.com")]
    Root {
        /// Domain name (auto-computed when --hub is used)
        domain: Option<String>,
        /// Hub domain (e.g., cmnhub.com). Generates a key, computes subdomain
        /// from pubkey (ed-<base32>), and sets domain + endpoints automatically.
        #[arg(long, conflicts_with = "endpoints_base")]
        hub: Option<String>,
        /// Custom site directory (default: ~/.cmn/mycelium/<domain>)
        #[arg(long)]
        site_path: Option<String>,
        /// Site or author name
        #[arg(long)]
        name: Option<String>,
        /// Brief description of the site or author
        #[arg(long)]
        synopsis: Option<String>,
        /// Bio (markdown)
        #[arg(long)]
        bio: Option<String>,
        /// Base URL for endpoints (e.g., https://example.com)
        #[arg(long)]
        endpoints_base: Option<String>,
    },
    /// Show site status
    #[command(after_long_help = "\
Examples:
  hypha mycelium status
  hypha mycelium status cmn.dev")]
    Status {
        /// Domain name (optional, lists all if not specified)
        domain: Option<String>,
        /// Custom site directory
        #[arg(long)]
        site_path: Option<String>,
    },
    /// Start a local HTTP server to serve the site (for debugging)
    #[command(after_long_help = "\
Examples:
  hypha mycelium serve
  hypha mycelium serve cmn.dev --port 3000")]
    Serve {
        /// Domain name
        domain: Option<String>,
        /// Custom site directory
        #[arg(long)]
        site_path: Option<String>,
        /// Port to listen on (default: 8080)
        #[arg(long, default_value = "8080")]
        port: u16,
    },
    /// Manage nutrient methods (add/remove/clear)
    #[command(after_long_help = "\
Examples:
  hypha mycelium nutrient add cmn.dev --type lightning_address --with address=user@example.com
  hypha mycelium nutrient add cmn.dev --type url --with url=https://example.com --with label=Donate
  hypha mycelium nutrient remove cmn.dev --type url
  hypha mycelium nutrient clear cmn.dev")]
    Nutrient {
        #[command(subcommand)]
        #[serde(flatten)]
        command: NutrientCommands,
    },
    /// Send a pulse to a synapse indexer
    #[command(after_long_help = "\
Examples:
  hypha mycelium pulse --synapse synapse.cmn.dev --file ~/.cmn/mycelium/cmn.dev/public/cmn/mycelium/<hash>.json
  hypha mycelium pulse --synapse https://synapse.cmn.dev --file ~/.cmn/mycelium/cmn.dev/public/cmn/mycelium/<hash>.json")]
    Pulse {
        /// Synapse server (domain or URL, default: configured default)
        #[arg(long)]
        synapse: Option<String>,
        /// Auth token for synapse (overrides configured token)
        #[arg(long)]
        synapse_token_secret: Option<String>,
        /// Path to signed mycelium.json
        #[arg(long)]
        file: String,
    },
}

#[derive(Subcommand, Serialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum NutrientCommands {
    /// Add or update a nutrient method (upsert by type)
    Add {
        /// Domain name
        domain: String,
        /// Nutrient method type (e.g., lightning_address, url, evm, solana)
        #[arg(long = "type", value_name = "TYPE")]
        method_type: String,
        /// Nutrient parameters (KEY=VALUE, value is parsed as JSON; repeatable)
        #[arg(long = "with", value_name = "KEY=VALUE")]
        with_entries: Vec<String>,
        /// Custom site directory
        #[arg(long)]
        site_path: Option<String>,
    },
    /// Remove a nutrient method by type
    Remove {
        /// Domain name
        domain: String,
        /// Nutrient method type to remove
        #[arg(long = "type", value_name = "TYPE")]
        method_type: String,
        /// Custom site directory
        #[arg(long)]
        site_path: Option<String>,
    },
    /// Remove all nutrient methods
    Clear {
        /// Domain name
        domain: String,
        /// Custom site directory
        #[arg(long)]
        site_path: Option<String>,
    },
}

#[derive(Subcommand, Serialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum SynapseAction {
    /// Discover Synapse instances via the network
    #[command(after_long_help = "\
Examples:
  hypha synapse discover
  hypha synapse discover --synapse https://synapse.cmn.dev")]
    Discover {
        /// Synapse to query (domain or URL, default: configured default)
        #[arg(long)]
        synapse: Option<String>,
        /// Auth token for synapse (overrides configured token)
        #[arg(long)]
        synapse_token_secret: Option<String>,
    },
    /// List configured Synapse nodes
    #[command(after_long_help = "\
Examples:
  hypha synapse list")]
    List,
    /// Check health of a Synapse instance
    #[command(after_long_help = "\
Examples:
  hypha synapse health
  hypha synapse health synapse.cmn.dev
  hypha synapse health https://synapse.cmn.dev")]
    Health {
        /// Synapse domain or URL (default: configured default)
        synapse: Option<String>,
        /// Auth token for synapse (overrides configured token)
        #[arg(long)]
        synapse_token_secret: Option<String>,
    },
    /// Add a Synapse node
    #[command(after_long_help = "\
Examples:
  hypha synapse add https://synapse.cmn.dev")]
    Add {
        /// Synapse URL
        url: String,
    },
    /// Remove a Synapse node
    #[command(after_long_help = "\
Examples:
  hypha synapse remove synapse.cmn.dev")]
    Remove {
        /// Synapse domain
        domain: String,
    },
    /// Set default Synapse node
    #[command(after_long_help = "\
Examples:
  hypha synapse use synapse.cmn.dev")]
    Use {
        /// Synapse domain
        domain: String,
    },
    /// Configure a Synapse node (token, etc.)
    #[command(after_long_help = "\
Examples:
  hypha synapse config synapse.cmn.dev --token-secret sk-abc123
  hypha synapse config synapse.cmn.dev --token-secret \"\"    # clear token")]
    Config {
        /// Synapse domain
        domain: String,
        /// Auth token (use empty string to clear)
        #[arg(long)]
        token_secret: Option<String>,
    },
}

#[derive(Subcommand, Serialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum CacheAction {
    /// List all cached spores
    #[command(after_long_help = "\
Examples:
  hypha cache list
  hypha cache list -o yaml")]
    List,
    /// Remove old or all cached items
    #[command(after_long_help = "\
Examples:
  hypha cache clean --all")]
    Clean {
        /// Remove all cached items
        #[arg(long)]
        all: bool,
    },
    /// Show local filesystem path for a cached spore
    #[command(after_long_help = concat!(
        "Examples:\n",
        "  hypha cache path cmn://cmn.dev/", "b3.3yMR7vZQ9hL2xKJdFtN8wPcB6sY1mXgU4eH5pTa2",
    ))]
    Path {
        /// CMN URI (e.g., cmn://cmn.dev/HASH)
        uri: String,
    },
}

#[derive(Subcommand, Serialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum ConfigAction {
    /// Show current configuration (merged defaults + config.toml)
    #[command(after_long_help = "\
Examples:
  hypha config list
  hypha config list -o yaml")]
    List,
    /// Set a configuration value
    #[command(after_long_help = "\
Dotted keys map to TOML sections:
  cache.path            Custom cache directory
  cache.cmn_ttl_s       cmn.json cache TTL in seconds
  cache.key_trust_ttl_s Key trust cache TTL in seconds
  cache.key_trust_refresh_mode
                        Key trust refresh mode: expired | always | offline
  cache.key_trust_synapse_witness_mode
                        Key trust fallback when domain is offline: allow | require_domain
  cache.clock_skew_tolerance_s
                        Clock skew tolerance in seconds for key trust TTL (default: 300)
  defaults.synapse      Default synapse domain
  defaults.domain       Default domain for publishing (release)
  defaults.taste.synapse
                        Synapse to submit taste reports to (overrides defaults.synapse for taste)
  defaults.taste.domain Domain to sign taste reports with (overrides defaults.domain for taste)

Examples:
  hypha config set cache.cmn_ttl_s 600
  hypha config set cache.key_trust_ttl_s 604800
  hypha config set cache.key_trust_refresh_mode offline
  hypha config set cache.key_trust_synapse_witness_mode require_domain
  hypha config set cache.path /tmp/hypha-cache
  hypha config set defaults.synapse synapse.cmn.dev
  hypha config set defaults.taste.synapse cmnhub.com
  hypha config set defaults.taste.domain ed-xxx.cmnhub.com")]
    Set {
        /// Config key (dotted path, e.g. cache.cmn_ttl_s)
        key: String,
        /// Value to set
        value: String,
    },
}

pub fn parse_or_exit() -> Cli {
    let raw: Vec<String> = std::env::args().collect();

    // --help: recursive plain-text help (all subcommands expanded)
    if raw.iter().any(|a| a == "--help" || a == "-h") {
        let subcommand_path: Vec<&str> = raw[1..]
            .iter()
            .take_while(|a| !a.starts_with('-'))
            .map(|s| s.as_str())
            .collect();
        let mut stdout = std::io::stdout();
        let _ = std::io::Write::write_all(
            &mut stdout,
            agent_first_data::cli_render_help(&Cli::command(), &subcommand_path).as_bytes(),
        );
        std::process::exit(0);
    }
    // --help-markdown: Markdown for doc generation
    if raw.iter().any(|a| a == "--help-markdown") {
        let subcommand_path: Vec<&str> = raw[1..]
            .iter()
            .take_while(|a| !a.starts_with('-'))
            .map(|s| s.as_str())
            .collect();
        let mut stdout = std::io::stdout();
        let _ = std::io::Write::write_all(
            &mut stdout,
            agent_first_data::cli_render_help_markdown(&Cli::command(), &subcommand_path)
                .as_bytes(),
        );
        std::process::exit(0);
    }

    Cli::try_parse().unwrap_or_else(|e| {
        if matches!(e.kind(), clap::error::ErrorKind::DisplayVersion) {
            let mut stdout = std::io::stdout();
            let message = agent_first_data::output_json(&agent_first_data::build_json_ok(
                serde_json::json!({ "version": env!("CARGO_PKG_VERSION") }),
                None,
            ));
            let _ = std::io::Write::write_all(&mut stdout, message.as_bytes());
            let _ = std::io::Write::write_all(&mut stdout, b"\n");
            std::process::exit(0);
        }

        let mut stdout = std::io::stdout();
        let message =
            agent_first_data::output_json(&agent_first_data::build_cli_error(&e.to_string(), None));
        let _ = std::io::Write::write_all(&mut stdout, message.as_bytes());
        let _ = std::io::Write::write_all(&mut stdout, b"\n");
        std::process::exit(2);
    })
}
