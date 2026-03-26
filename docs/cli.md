<!-- Generated. Do not edit by hand. -->

# Hypha CLI Reference

> Regenerate with `./scripts/generate-cli-doc.sh`.

# Command-Line Help for `hypha`

This document contains the help content for the `hypha` command-line program.

**Command Overview:**

* [`hypha`↴](#hypha)
* [`hypha sense`↴](#hypha-sense)
* [`hypha taste`↴](#hypha-taste)
* [`hypha spawn`↴](#hypha-spawn)
* [`hypha grow`↴](#hypha-grow)
* [`hypha absorb`↴](#hypha-absorb)
* [`hypha bond`↴](#hypha-bond)
* [`hypha replicate`↴](#hypha-replicate)
* [`hypha hatch`↴](#hypha-hatch)
* [`hypha hatch bond`↴](#hypha-hatch-bond)
* [`hypha hatch bond set`↴](#hypha-hatch-bond-set)
* [`hypha hatch bond remove`↴](#hypha-hatch-bond-remove)
* [`hypha hatch bond clear`↴](#hypha-hatch-bond-clear)
* [`hypha hatch tree`↴](#hypha-hatch-tree)
* [`hypha hatch tree set`↴](#hypha-hatch-tree-set)
* [`hypha hatch tree show`↴](#hypha-hatch-tree-show)
* [`hypha release`↴](#hypha-release)
* [`hypha lineage`↴](#hypha-lineage)
* [`hypha search`↴](#hypha-search)
* [`hypha mycelium`↴](#hypha-mycelium)
* [`hypha mycelium root`↴](#hypha-mycelium-root)
* [`hypha mycelium status`↴](#hypha-mycelium-status)
* [`hypha mycelium serve`↴](#hypha-mycelium-serve)
* [`hypha mycelium nutrient`↴](#hypha-mycelium-nutrient)
* [`hypha mycelium nutrient add`↴](#hypha-mycelium-nutrient-add)
* [`hypha mycelium nutrient remove`↴](#hypha-mycelium-nutrient-remove)
* [`hypha mycelium nutrient clear`↴](#hypha-mycelium-nutrient-clear)
* [`hypha mycelium pulse`↴](#hypha-mycelium-pulse)
* [`hypha synapse`↴](#hypha-synapse)
* [`hypha synapse discover`↴](#hypha-synapse-discover)
* [`hypha synapse list`↴](#hypha-synapse-list)
* [`hypha synapse health`↴](#hypha-synapse-health)
* [`hypha synapse add`↴](#hypha-synapse-add)
* [`hypha synapse remove`↴](#hypha-synapse-remove)
* [`hypha synapse use`↴](#hypha-synapse-use)
* [`hypha synapse config`↴](#hypha-synapse-config)
* [`hypha cache`↴](#hypha-cache)
* [`hypha cache list`↴](#hypha-cache-list)
* [`hypha cache clean`↴](#hypha-cache-clean)
* [`hypha cache path`↴](#hypha-cache-path)
* [`hypha config`↴](#hypha-config)
* [`hypha config list`↴](#hypha-config-list)
* [`hypha config set`↴](#hypha-config-set)

## `hypha`

CMN Client - A bio-digital extension for Visitors to release and absorb Spores

**Usage:** `hypha [OPTIONS] <COMMAND>`

All output follows Agent-First Data format:
  {"code": "ok", "result": {...}, "trace": {...}}

Quick start (try with cmn.dev):
  hypha sense cmn://cmn.dev
  hypha sense cmn://cmn.dev/b3.3yMR7vZQ9hL2xKJdFtN8wPcB6sY1mXgU4eH5pTa2
  hypha spawn cmn://cmn.dev/b3.3yMR7vZQ9hL2xKJdFtN8wPcB6sY1mXgU4eH5pTa2
  hypha cache list

###### **Subcommands:**

* `sense` — Resolve a CMN URI and show metadata without downloading
* `taste` — Evaluate spore: download for review, or record a verdict
* `spawn` — Create a working copy of a spore (auto-detects best distribution format)
* `grow` — Pull latest changes from spawn source via Synapse lineage
* `absorb` — Prepare spores for AI-assisted merge
* `bond` — Fetch all bonds from spore.core.json to .cmn/bonds/
* `replicate` — Copy a spore to your domain (same hash, re-signed capsule)
* `hatch` — Create or update spore.core.json in working directory
* `release` — Sign and publish spore to mycelium site
* `lineage` — Trace spore lineage: descendants (in, default) or ancestors (out)
* `search` — Search for spores by keyword (semantic search via Synapse)
* `mycelium` — Manage local mycelium site
* `synapse` — Manage Synapse node connections
* `cache` — Manage local cache
* `config` — View or modify hypha configuration

###### **Options:**

* `-o`, `--output <OUTPUT>` — Output format

  Default value: `json`
* `--log <LOG>` — Log categories (comma-separated): startup, request, ...



## `hypha sense`

Resolve a CMN URI and show metadata without downloading

**Usage:** `hypha sense <URI>`

URI types:
  cmn://DOMAIN                       List all spores on a site
  cmn://DOMAIN/HASH                  View a specific spore

Examples:
  hypha sense cmn://cmn.dev
  hypha sense cmn://cmn.dev/b3.3yMR7vZQ9hL2xKJdFtN8wPcB6sY1mXgU4eH5pTa2
  hypha sense cmn://cmn.dev -o yaml

###### **Arguments:**

* `<URI>` — CMN URI (cmn://DOMAIN or cmn://DOMAIN/HASH)



## `hypha taste`

Evaluate spore: download for review, or record a verdict

**Usage:** `hypha taste [OPTIONS] <URI>`

Without --verdict: downloads the spore for local review.
With --verdict: records a verdict (sweet, fresh, safe, rotten, toxic).

Examples:
  hypha taste cmn://cmn.dev/b3.3yMR7vZQ9hL2xKJdFtN8wPcB6sY1mXgU4eH5pTa2
  hypha taste cmn://cmn.dev/b3.3yMR7vZQ9hL2xKJdFtN8wPcB6sY1mXgU4eH5pTa2 --verdict safe --notes "Reviewed: clean code"
  hypha taste cmn://cmn.dev/b3.3yMR7vZQ9hL2xKJdFtN8wPcB6sY1mXgU4eH5pTa2 --verdict safe --domain cmn.dev --synapse https://synapse.cmn.dev

###### **Arguments:**

* `<URI>` — CMN URI (e.g., cmn://cmn.dev/HASH)

###### **Options:**

* `--verdict <VERDICT>` — Record verdict: sweet, fresh, safe, rotten, or toxic
* `--notes <NOTES>` — Notes about the verdict
* `--synapse <SYNAPSE>` — Synapse URL to pull/share taste reports
* `--synapse-token-secret <SYNAPSE_TOKEN_SECRET>` — Auth token for synapse (overrides configured token)
* `--domain <DOMAIN>` — Domain to sign report with (requires --synapse and --verdict)



## `hypha spawn`

Create a working copy of a spore (auto-detects best distribution format)

**Usage:** `hypha spawn [OPTIONS] <URI> [DIRECTORY]`

Distribution sources (auto-detected):
  archive    Download .tar.zst archive (default, fastest)
  git        Clone from dist.git URL if available

Examples:
  hypha spawn cmn://cmn.dev/b3.3yMR7vZQ9hL2xKJdFtN8wPcB6sY1mXgU4eH5pTa2
  hypha spawn cmn://cmn.dev/b3.3yMR7vZQ9hL2xKJdFtN8wPcB6sY1mXgU4eH5pTa2 my-project --vcs git
  hypha spawn cmn://cmn.dev/b3.3yMR7vZQ9hL2xKJdFtN8wPcB6sY1mXgU4eH5pTa2 --dist git

###### **Arguments:**

* `<URI>` — CMN URI (e.g., cmn://cmn.dev/HASH)
* `<DIRECTORY>` — Target directory (default: ./<spore-id>)

###### **Options:**

* `--vcs <TYPE>` — Initialize version control after spawn (e.g., --vcs git)
* `--dist <SOURCE>` — Preferred distribution source: archive (default) or git
* `--bond` — Fetch bonds after spawn



## `hypha grow`

Pull latest changes from spawn source via Synapse lineage

**Usage:** `hypha grow [OPTIONS]`

Run inside a previously spawned directory to update it.
Uses Synapse lineage to discover newer versions from the same publisher.

If local files have been modified (git dirty or tree hash mismatch),
grow refuses to overwrite them and shows the cache path for manual merge.

Examples:
  hypha grow
  hypha grow --synapse synapse.cmn.dev
  hypha grow --dist git
  hypha grow --dist archive
  hypha grow --bond --synapse synapse.cmn.dev

###### **Options:**

* `--dist <SOURCE>` — Override distribution source: archive or git
* `--synapse <SYNAPSE>` — Synapse to query for updates (domain or URL)
* `--synapse-token-secret <SYNAPSE_TOKEN_SECRET>` — Auth token for synapse (overrides configured token)
* `--bond` — Also check depends_on/follows/extends bonds for updates via Synapse lineage, and fetch all bonds to .cmn/bonds/



## `hypha absorb`

Prepare spores for AI-assisted merge

**Usage:** `hypha absorb [OPTIONS] [URIS]...`

Absorb downloads spores into .cmn/absorb/ for AI-assisted merge.
Use --discover to auto-discover descendants via Synapse.

Examples:
  hypha absorb cmn://cmn.dev/b3.3yMR7vZQ9hL2xKJdFtN8wPcB6sY1mXgU4eH5pTa2
  hypha absorb --discover --synapse https://synapse.cmn.dev
  hypha absorb --discover --synapse https://synapse.cmn.dev --max-depth 5

###### **Arguments:**

* `<URIS>` — CMN URIs to absorb (e.g., cmn://cmn.dev/HASH)

###### **Options:**

* `--discover` — Auto-discover descendants from current spore's spawned_from bonds
* `--synapse <SYNAPSE>` — Synapse server URL (required with --discover)
* `--synapse-token-secret <SYNAPSE_TOKEN_SECRET>` — Auth token for synapse (overrides configured token)
* `--max-depth <MAX_DEPTH>` — Maximum depth for lineage discovery (default: 10)

  Default value: `10`



## `hypha bond`

Fetch all bonds from spore.core.json to .cmn/bonds/

**Usage:** `hypha bond [OPTIONS]`

Examples:
  hypha bond
  hypha bond --status
  hypha bond --clean

###### **Options:**

* `--clean` — Clean orphaned bonds not in spore.core.json
* `--status` — Show bond status without fetching



## `hypha replicate`

Copy a spore to your domain (same hash, re-signed capsule)

**Usage:** `hypha replicate [OPTIONS] --domain <DOMAIN> [URIS]...`

Replicates spores from another domain to yours. The hash stays the same
because core + core_signature are preserved. Only capsule_signature changes.

Examples:
  hypha replicate cmn://other.dev/HASH --domain my.dev
  hypha replicate --refs --domain my.dev

###### **Arguments:**

* `<URIS>` — CMN URI(s) to replicate

###### **Options:**

* `--refs` — Replicate all non-self bonds from spore.core.json
* `--domain <DOMAIN>` — Target domain (required)
* `--site-path <SITE_PATH>` — Custom site directory



## `hypha hatch`

Create or update spore.core.json in working directory

**Usage:** `hypha hatch [OPTIONS]
       hatch <COMMAND>`

Examples:
  hypha hatch --id my-tool --name "My Tool" --synopsis "A useful tool"
  hypha hatch --intent "Provide a reusable HTTP client for CMN agents" --mutations "Initial release"
  hypha hatch --license MIT --domain cmn.dev

Subcommands:
  hypha hatch bond set/remove/clear   Manage bonds in spore.core.json
  hypha hatch tree set/show            Manage tree configuration

###### **Subcommands:**

* `bond` — Manage bonds in spore.core.json
* `tree` — Manage tree configuration in spore.core.json

###### **Options:**

* `--id <ID>` — Opaque identifier stored in spore.core.json
* `--version <VERSION>` — Version string (e.g., 1.0.0)
* `--name <NAME>` — Display name
* `--domain <DOMAIN>` — Publisher domain
* `--synopsis <SYNOPSIS>` — Short description
* `--intent <INTENT>` — Why this spore exists — permanent across releases (repeatable)
* `--mutations <MUTATIONS>` — What changed relative to the spawned-from parent (repeatable)
* `--license <LICENSE>` — License (SPDX identifier)



## `hypha hatch bond`

Manage bonds in spore.core.json

**Usage:** `hypha hatch bond <COMMAND>`

Examples:
  hypha hatch bond set --uri cmn://cmn.dev/b3.abc --relation follows --id my-lib --reason "Core library"
  hypha hatch bond set --uri cmn://cmn.dev/b3.abc --with 'mints=["https://mint.example.com"]'
  hypha hatch bond remove --relation follows
  hypha hatch bond clear

###### **Subcommands:**

* `set` — Add or update a bond (upsert by URI)
* `remove` — Remove bonds by URI and/or relation
* `clear` — Remove all bonds



## `hypha hatch bond set`

Add or update a bond (upsert by URI)

**Usage:** `hypha hatch bond set [OPTIONS] --uri <URI>`

###### **Options:**

* `--uri <URI>` — Bond URI (match key)
* `--relation <RELATION>` — Bond relation (required for new bonds)
* `--id <ID>` — Bond id
* `--reason <REASON>` — Bond reason
* `--with <KEY=VALUE>` — Bond parameters (KEY=VALUE, value is parsed as JSON; repeatable)



## `hypha hatch bond remove`

Remove bonds by URI and/or relation

**Usage:** `hypha hatch bond remove [OPTIONS]`

###### **Options:**

* `--uri <URI>` — Remove bonds matching this URI
* `--relation <RELATION>` — Remove bonds matching this relation



## `hypha hatch bond clear`

Remove all bonds

**Usage:** `hypha hatch bond clear`



## `hypha hatch tree`

Manage tree configuration in spore.core.json

**Usage:** `hypha hatch tree <COMMAND>`

###### **Subcommands:**

* `set` — Set tree configuration fields
* `show` — Show current tree configuration



## `hypha hatch tree set`

Set tree configuration fields

**Usage:** `hypha hatch tree set [OPTIONS]`

###### **Options:**

* `--algorithm <ALGORITHM>` — Hash algorithm (e.g., blob_tree_blake3_nfc)
* `--exclude-names <EXCLUDE_NAMES>` — File/directory names to exclude from hashing (repeatable)
* `--follow-rules <FOLLOW_RULES>` — Ignore-rule files to follow (repeatable)



## `hypha hatch tree show`

Show current tree configuration

**Usage:** `hypha hatch tree show`



## `hypha release`

Sign and publish spore to mycelium site

**Usage:** `hypha release [OPTIONS] --domain <DOMAIN>`

Requires `hypha mycelium root` first to set up the site.

Examples:
  hypha release --domain cmn.dev
  hypha release --domain cmn.dev --source ./my-spore
  hypha release --domain cmn.dev --dry-run          # pre-compute URI without releasing
  hypha release --domain cmn.dev --archive zstd
  hypha release --domain cmn.dev --dist-git https://github.com/user/repo --dist-ref v1.0

###### **Options:**

* `--domain <DOMAIN>` — Target domain (required)
* `--source <SOURCE>` — Spore source directory (default: current directory)
* `--site-path <SITE_PATH>` — Custom site directory (default: ~/.cmn/mycelium/<domain>)
* `--dist-git <DIST_GIT>` — External git repository URL
* `--dist-ref <DIST_REF>` — Git ref: tag/branch/commit (requires --dist-git)
* `--archive <FORMAT>` — Archive format for release generation (currently only: zstd)

  Default value: `zstd`
* `--dry-run` — Pre-compute URI without writing any files



## `hypha lineage`

Trace spore lineage: descendants (in, default) or ancestors (out)

**Usage:** `hypha lineage [OPTIONS] <URI>`

Direction:
  --direction in   Find descendants / forks (default)
  --direction out  Trace ancestors / spawn chain

Examples:
  hypha lineage cmn://cmn.dev/b3.3yMR7vZQ9hL2xKJdFtN8wPcB6sY1mXgU4eH5pTa2 --synapse https://synapse.cmn.dev
  hypha lineage cmn://cmn.dev/b3.3yMR7vZQ9hL2xKJdFtN8wPcB6sY1mXgU4eH5pTa2 --direction out --synapse https://synapse.cmn.dev
  hypha lineage cmn://cmn.dev/b3.3yMR7vZQ9hL2xKJdFtN8wPcB6sY1mXgU4eH5pTa2 --synapse https://synapse.cmn.dev --max-depth 5

###### **Arguments:**

* `<URI>` — CMN URI (e.g., cmn://cmn.dev/HASH)

###### **Options:**

* `--direction <DIRECTION>` — Direction: in (descendants, default) or out (ancestors)
* `--synapse <SYNAPSE>` — Synapse server (domain or URL, default: configured default)
* `--synapse-token-secret <SYNAPSE_TOKEN_SECRET>` — Auth token for synapse (overrides configured token)
* `--max-depth <MAX_DEPTH>` — Maximum traversal depth (default: 10)

  Default value: `10`



## `hypha search`

Search for spores by keyword (semantic search via Synapse)

**Usage:** `hypha search [OPTIONS] <QUERY>`

Examples:
  hypha search "protocol spec" --synapse https://synapse.cmn.dev
  hypha search "data format" --synapse https://synapse.cmn.dev --domain cmn.dev
  hypha search "agent tools" --synapse https://synapse.cmn.dev --license MIT --limit 5
  hypha search "http client" --bonds spawned_from:cmn://cmn.dev/b3.abc123

###### **Arguments:**

* `<QUERY>` — Search query text

###### **Options:**

* `--synapse <SYNAPSE>` — Synapse server (domain or URL, default: configured default)
* `--synapse-token-secret <SYNAPSE_TOKEN_SECRET>` — Auth token for synapse (overrides configured token)
* `--domain <DOMAIN>` — Filter by domain
* `--license <LICENSE>` — Filter by license (SPDX identifier)
* `--bonds <BONDS>` — Filter by bond relationship (format: relation:uri, comma-separated for AND)
* `--limit <LIMIT>` — Maximum results (default: 20)

  Default value: `20`



## `hypha mycelium`

Manage local mycelium site

**Usage:** `hypha mycelium <COMMAND>`

###### **Subcommands:**

* `root` — Establish a new site for a domain (or update existing)
* `status` — Show site status
* `serve` — Start a local HTTP server to serve the site (for debugging)
* `nutrient` — Manage nutrient methods (add/remove/clear)
* `pulse` — Send a pulse to a synapse indexer



## `hypha mycelium root`

Establish a new site for a domain (or update existing)

**Usage:** `hypha mycelium root [OPTIONS] [DOMAIN]`

Creates ~/.cmn/mycelium/<domain>/ with key pair and site structure.
Run this once before `hypha release`.

With --hub, creates a taste-only account on a hosted hub (e.g. cmnhub.com):
  1. Generates ed25519 key pair
  2. Computes subdomain from pubkey (ed-<base32>.hub)
  3. Creates taste-only cmn.json with taste endpoint
  4. Registers hub as a synapse node
  5. Sets [defaults.taste] so `hypha taste` auto-submits

After --hub, register with the hub then taste without extra flags:
  curl -X POST https://cmnhub.com/synapse/pulse -H 'Content-Type: application/json' \
    -d @~/.cmn/mycelium/ed-xxx.cmnhub.com/public/.well-known/cmn.json
  hypha taste cmn://example.com/b3.HASH --verdict safe

Examples:
  hypha mycelium root cmn.dev --name "CMN" --synopsis "Code Mycelial Network"
  hypha mycelium root cmn.dev --endpoints-base https://cmn.dev
  hypha mycelium root example.com --site-path /custom/path
  hypha mycelium root --hub cmnhub.com

###### **Arguments:**

* `<DOMAIN>` — Domain name (auto-computed when --hub is used)

###### **Options:**

* `--hub <HUB>` — Hub domain (e.g., cmnhub.com). Generates a key, computes subdomain from pubkey (ed-<base32>), and sets domain + endpoints automatically
* `--site-path <SITE_PATH>` — Custom site directory (default: ~/.cmn/mycelium/<domain>)
* `--name <NAME>` — Site or author name
* `--synopsis <SYNOPSIS>` — Brief description of the site or author
* `--bio <BIO>` — Bio (markdown)
* `--endpoints-base <ENDPOINTS_BASE>` — Base URL for endpoints (e.g., https://example.com)



## `hypha mycelium status`

Show site status

**Usage:** `hypha mycelium status [OPTIONS] [DOMAIN]`

Examples:
  hypha mycelium status
  hypha mycelium status cmn.dev

###### **Arguments:**

* `<DOMAIN>` — Domain name (optional, lists all if not specified)

###### **Options:**

* `--site-path <SITE_PATH>` — Custom site directory



## `hypha mycelium serve`

Start a local HTTP server to serve the site (for debugging)

**Usage:** `hypha mycelium serve [OPTIONS] [DOMAIN]`

Examples:
  hypha mycelium serve
  hypha mycelium serve cmn.dev --port 3000

###### **Arguments:**

* `<DOMAIN>` — Domain name

###### **Options:**

* `--site-path <SITE_PATH>` — Custom site directory
* `--port <PORT>` — Port to listen on (default: 8080)

  Default value: `8080`



## `hypha mycelium nutrient`

Manage nutrient methods (add/remove/clear)

**Usage:** `hypha mycelium nutrient <COMMAND>`

Examples:
  hypha mycelium nutrient add cmn.dev --type lightning_address --with address=user@example.com
  hypha mycelium nutrient add cmn.dev --type url --with url=https://example.com --with label=Donate
  hypha mycelium nutrient remove cmn.dev --type url
  hypha mycelium nutrient clear cmn.dev

###### **Subcommands:**

* `add` — Add or update a nutrient method (upsert by type)
* `remove` — Remove a nutrient method by type
* `clear` — Remove all nutrient methods



## `hypha mycelium nutrient add`

Add or update a nutrient method (upsert by type)

**Usage:** `hypha mycelium nutrient add [OPTIONS] --type <TYPE> <DOMAIN>`

###### **Arguments:**

* `<DOMAIN>` — Domain name

###### **Options:**

* `--type <TYPE>` — Nutrient method type (e.g., lightning_address, url, evm, solana)
* `--with <KEY=VALUE>` — Nutrient parameters (KEY=VALUE, value is parsed as JSON; repeatable)
* `--site-path <SITE_PATH>` — Custom site directory



## `hypha mycelium nutrient remove`

Remove a nutrient method by type

**Usage:** `hypha mycelium nutrient remove [OPTIONS] --type <TYPE> <DOMAIN>`

###### **Arguments:**

* `<DOMAIN>` — Domain name

###### **Options:**

* `--type <TYPE>` — Nutrient method type to remove
* `--site-path <SITE_PATH>` — Custom site directory



## `hypha mycelium nutrient clear`

Remove all nutrient methods

**Usage:** `hypha mycelium nutrient clear [OPTIONS] <DOMAIN>`

###### **Arguments:**

* `<DOMAIN>` — Domain name

###### **Options:**

* `--site-path <SITE_PATH>` — Custom site directory



## `hypha mycelium pulse`

Send a pulse to a synapse indexer

**Usage:** `hypha mycelium pulse [OPTIONS] --file <FILE>`

Examples:
  hypha mycelium pulse --synapse synapse.cmn.dev --file ~/.cmn/mycelium/cmn.dev/public/cmn/mycelium/<hash>.json
  hypha mycelium pulse --synapse https://synapse.cmn.dev --file ~/.cmn/mycelium/cmn.dev/public/cmn/mycelium/<hash>.json

###### **Options:**

* `--synapse <SYNAPSE>` — Synapse server (domain or URL, default: configured default)
* `--synapse-token-secret <SYNAPSE_TOKEN_SECRET>` — Auth token for synapse (overrides configured token)
* `--file <FILE>` — Path to signed mycelium.json



## `hypha synapse`

Manage Synapse node connections

**Usage:** `hypha synapse <COMMAND>`

###### **Subcommands:**

* `discover` — Discover Synapse instances via the network
* `list` — List configured Synapse nodes
* `health` — Check health of a Synapse instance
* `add` — Add a Synapse node
* `remove` — Remove a Synapse node
* `use` — Set default Synapse node
* `config` — Configure a Synapse node (token, etc.)



## `hypha synapse discover`

Discover Synapse instances via the network

**Usage:** `hypha synapse discover [OPTIONS]`

Examples:
  hypha synapse discover
  hypha synapse discover --synapse https://synapse.cmn.dev

###### **Options:**

* `--synapse <SYNAPSE>` — Synapse to query (domain or URL, default: configured default)
* `--synapse-token-secret <SYNAPSE_TOKEN_SECRET>` — Auth token for synapse (overrides configured token)



## `hypha synapse list`

List configured Synapse nodes

**Usage:** `hypha synapse list`

Examples:
  hypha synapse list



## `hypha synapse health`

Check health of a Synapse instance

**Usage:** `hypha synapse health [OPTIONS] [SYNAPSE]`

Examples:
  hypha synapse health
  hypha synapse health synapse.cmn.dev
  hypha synapse health https://synapse.cmn.dev

###### **Arguments:**

* `<SYNAPSE>` — Synapse domain or URL (default: configured default)

###### **Options:**

* `--synapse-token-secret <SYNAPSE_TOKEN_SECRET>` — Auth token for synapse (overrides configured token)



## `hypha synapse add`

Add a Synapse node

**Usage:** `hypha synapse add <URL>`

Examples:
  hypha synapse add https://synapse.cmn.dev

###### **Arguments:**

* `<URL>` — Synapse URL



## `hypha synapse remove`

Remove a Synapse node

**Usage:** `hypha synapse remove <DOMAIN>`

Examples:
  hypha synapse remove synapse.cmn.dev

###### **Arguments:**

* `<DOMAIN>` — Synapse domain



## `hypha synapse use`

Set default Synapse node

**Usage:** `hypha synapse use <DOMAIN>`

Examples:
  hypha synapse use synapse.cmn.dev

###### **Arguments:**

* `<DOMAIN>` — Synapse domain



## `hypha synapse config`

Configure a Synapse node (token, etc.)

**Usage:** `hypha synapse config [OPTIONS] <DOMAIN>`

Examples:
  hypha synapse config synapse.cmn.dev --token-secret sk-abc123
  hypha synapse config synapse.cmn.dev --token-secret ""    # clear token

###### **Arguments:**

* `<DOMAIN>` — Synapse domain

###### **Options:**

* `--token-secret <TOKEN_SECRET>` — Auth token (use empty string to clear)



## `hypha cache`

Manage local cache

**Usage:** `hypha cache <COMMAND>`

###### **Subcommands:**

* `list` — List all cached spores
* `clean` — Remove old or all cached items
* `path` — Show local filesystem path for a cached spore



## `hypha cache list`

List all cached spores

**Usage:** `hypha cache list`

Examples:
  hypha cache list
  hypha cache list -o yaml



## `hypha cache clean`

Remove old or all cached items

**Usage:** `hypha cache clean [OPTIONS]`

Examples:
  hypha cache clean --all

###### **Options:**

* `--all` — Remove all cached items



## `hypha cache path`

Show local filesystem path for a cached spore

**Usage:** `hypha cache path <URI>`

Examples:
  hypha cache path cmn://cmn.dev/b3.3yMR7vZQ9hL2xKJdFtN8wPcB6sY1mXgU4eH5pTa2

###### **Arguments:**

* `<URI>` — CMN URI (e.g., cmn://cmn.dev/HASH)



## `hypha config`

View or modify hypha configuration

**Usage:** `hypha config <COMMAND>`

###### **Subcommands:**

* `list` — Show current configuration (merged defaults + config.toml)
* `set` — Set a configuration value



## `hypha config list`

Show current configuration (merged defaults + config.toml)

**Usage:** `hypha config list`

Examples:
  hypha config list
  hypha config list -o yaml



## `hypha config set`

Set a configuration value

**Usage:** `hypha config set <KEY> <VALUE>`

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
  hypha config set defaults.taste.domain ed-xxx.cmnhub.com

###### **Arguments:**

* `<KEY>` — Config key (dotted path, e.g. cache.cmn_ttl_s)
* `<VALUE>` — Value to set
