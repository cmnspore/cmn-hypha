<!-- Generated. Do not edit by hand. -->

# Hypha CLI Reference

> Regenerate with `hypha --help --recursive --output markdown`.

# hypha - CMN Client - A bio-digital extension for Visitors to release and absorb Spores

```text
CMN Client - A bio-digital extension for Visitors to release and absorb Spores

Usage: hypha [OPTIONS] <COMMAND>

Commands:
  sense      Resolve a CMN URI and show metadata without downloading
  taste      Evaluate spore: download for review, or record a verdict
  spawn      Create a working copy of a spore (auto-detects best distribution format)
  grow       Pull latest changes from spawn source via Synapse lineage
  absorb     Prepare spores for AI-assisted merge
  bond       Fetch all bonds from spore.core.json to .cmn/bonds/
  replicate  Copy a spore to your domain (same hash, re-signed capsule)
  hatch      Create or update spore.core.json in working directory
  release    Sign and publish spore to mycelium site
  lineage    Trace spore lineage: descendants (in, default) or ancestors (out)
  search     Search for spores by keyword (semantic search via Synapse)
  mycelium   Manage local mycelium site
  synapse    Manage Synapse node connections
  cache      Manage local cache
  config     View or modify hypha configuration
  skill      Install, uninstall, or inspect the bundled Hypha agent skill

Options:
  -o, --output <OUTPUT>
          Output format: json (default), yaml, plain; help also accepts markdown

          [default: json]

      --log <LOG>
          Log categories (comma-separated): startup, request, ...

  -h, --help
          Print help. Add --recursive to expand every nested subcommand; add --output json|yaml|markdown to render this help in another format.

  -V, --version
          Print version

All output follows Agent-First Data format:
  {"code": "ok", "result": {...}, "trace": {...}}

Quick start (try with cmn.dev):
  hypha sense cmn://cmn.dev
  hypha sense cmn://cmn.dev/b3.3yMR7vZQ9hL2xKJdFtN8wPcB6sY1mXgU4eH5pTa2
  hypha spawn cmn://cmn.dev/b3.3yMR7vZQ9hL2xKJdFtN8wPcB6sY1mXgU4eH5pTa2
  hypha cache list

More help:
  hypha <command> --help   Show one command layer
  hypha --help --recursive Expand every command and flag
  hypha --help --recursive --output markdown
                            Generate recursive Markdown reference
```

## hypha sense - Resolve a CMN URI and show metadata without downloading

```text
Resolve a CMN URI and show metadata without downloading

Usage: sense [OPTIONS] <URI>

Arguments:
  <URI>
          CMN URI (cmn://DOMAIN or cmn://DOMAIN/HASH)

Options:
      --id <ID>
          Spore id to resolve from the domain's latest mycelium inventory

  -h, --help
          Print help (see a summary with '-h')

URI types:
  cmn://DOMAIN                       List all spores on a site
  cmn://DOMAIN/HASH                  View a specific spore
  cmn://DOMAIN --id SPORE_ID         View latest spore with id from a site

Examples:
  hypha sense cmn://cmn.dev
  hypha sense cmn://cmn.dev --id cmn-spec
  hypha sense cmn://cmn.dev/b3.3yMR7vZQ9hL2xKJdFtN8wPcB6sY1mXgU4eH5pTa2
  hypha sense cmn://cmn.dev -o yaml
```

## hypha taste - Evaluate spore: download for review, or record a verdict

```text
Evaluate spore: download for review, or record a verdict

Usage: taste [OPTIONS] <URI>

Arguments:
  <URI>
          CMN URI (e.g., cmn://cmn.dev/HASH)

Options:
      --verdict <VERDICT>
          Record verdict: sweet, fresh, safe, rotten, or toxic

      --notes <NOTES>
          Notes about the verdict

      --synapse <SYNAPSE>
          Synapse URL to pull/share taste reports

      --synapse-token-secret <SYNAPSE_TOKEN_SECRET>
          Auth token for synapse (overrides configured token)

      --domain <DOMAIN>
          Domain to sign report with (requires --synapse and --verdict)

  -h, --help
          Print help (see a summary with '-h')

Without --verdict: downloads the spore for local review.
With --verdict: records a verdict (sweet, fresh, safe, rotten, toxic).

Examples:
  hypha taste cmn://cmn.dev/b3.3yMR7vZQ9hL2xKJdFtN8wPcB6sY1mXgU4eH5pTa2
  hypha taste cmn://cmn.dev/b3.3yMR7vZQ9hL2xKJdFtN8wPcB6sY1mXgU4eH5pTa2 --verdict safe --notes "Reviewed: clean code"
  hypha taste cmn://cmn.dev/b3.3yMR7vZQ9hL2xKJdFtN8wPcB6sY1mXgU4eH5pTa2 --verdict safe --domain cmn.dev --synapse https://synapse.cmn.dev
```

## hypha spawn - Create a working copy of a spore (auto-detects best distribution format)

```text
Create a working copy of a spore (auto-detects best distribution format)

Usage: spawn [OPTIONS] <URI> [DIRECTORY]

Arguments:
  <URI>
          CMN URI (e.g., cmn://cmn.dev/HASH)

  [DIRECTORY]
          Target directory (default: ./<spore-id>)

Options:
      --vcs <TYPE>
          Initialize version control after spawn (e.g., --vcs git)

          [possible values: git, none]

      --dist <SOURCE>
          Preferred distribution source: archive (default) or git

          [possible values: archive, git]

      --bond
          Fetch bonds after spawn

  -h, --help
          Print help (see a summary with '-h')

Distribution sources (auto-detected):
  archive    Download .tar.zst archive (default, fastest)
  git        Clone from dist.git URL if available

Examples:
  hypha spawn cmn://cmn.dev/b3.3yMR7vZQ9hL2xKJdFtN8wPcB6sY1mXgU4eH5pTa2
  hypha spawn cmn://cmn.dev/b3.3yMR7vZQ9hL2xKJdFtN8wPcB6sY1mXgU4eH5pTa2 my-project --vcs git
  hypha spawn cmn://cmn.dev/b3.3yMR7vZQ9hL2xKJdFtN8wPcB6sY1mXgU4eH5pTa2 --dist git
```

## hypha grow - Pull latest changes from spawn source via Synapse lineage

```text
Pull latest changes from spawn source via Synapse lineage

Usage: grow [OPTIONS]

Options:
      --dist <SOURCE>
          Override distribution source: archive or git

          [possible values: archive, git]

      --synapse <SYNAPSE>
          Synapse to query for updates (domain or URL)

      --synapse-token-secret <SYNAPSE_TOKEN_SECRET>
          Auth token for synapse (overrides configured token)

      --bond
          Also check depends_on/follows/extends bonds for updates via Synapse lineage, and fetch all bonds to .cmn/bonds/

  -h, --help
          Print help (see a summary with '-h')

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
```

## hypha absorb - Prepare spores for AI-assisted merge

```text
Prepare spores for AI-assisted merge

Usage: absorb [OPTIONS] [URIS]...

Arguments:
  [URIS]...
          CMN URIs to absorb (e.g., cmn://cmn.dev/HASH)

Options:
      --discover
          Auto-discover descendants from current spore's spawned_from bonds

      --synapse <SYNAPSE>
          Synapse server URL (required with --discover)

      --synapse-token-secret <SYNAPSE_TOKEN_SECRET>
          Auth token for synapse (overrides configured token)

      --max-depth <MAX_DEPTH>
          Maximum depth for lineage discovery (default: 10)

          [default: 10]

  -h, --help
          Print help (see a summary with '-h')

Absorb downloads spores into .cmn/absorb/ for AI-assisted merge.
Use --discover to auto-discover descendants via Synapse.

Examples:
  hypha absorb cmn://cmn.dev/b3.3yMR7vZQ9hL2xKJdFtN8wPcB6sY1mXgU4eH5pTa2
  hypha absorb --discover --synapse https://synapse.cmn.dev
  hypha absorb --discover --synapse https://synapse.cmn.dev --max-depth 5
```

## hypha bond - Fetch all bonds from spore.core.json to .cmn/bonds/

```text
Fetch all bonds from spore.core.json to .cmn/bonds/

Usage: bond [OPTIONS]

Options:
      --clean
          Clean orphaned bonds not in spore.core.json

      --status
          Show bond status without fetching

  -h, --help
          Print help (see a summary with '-h')

Examples:
  hypha bond
  hypha bond --status
  hypha bond --clean
```

## hypha replicate - Copy a spore to your domain (same hash, re-signed capsule)

```text
Copy a spore to your domain (same hash, re-signed capsule)

Usage: replicate [OPTIONS] --domain <DOMAIN> [URIS]...

Arguments:
  [URIS]...
          CMN URI(s) to replicate

Options:
      --refs
          Replicate all non-self bonds from spore.core.json

      --domain <DOMAIN>
          Target domain (required)

      --site-path <SITE_PATH>
          Custom site directory

  -h, --help
          Print help (see a summary with '-h')

Replicates spores from another domain to yours. The hash stays the same
because core + core_signature are preserved. Only capsule_signature changes.

Examples:
  hypha replicate cmn://other.dev/HASH --domain my.dev
  hypha replicate --refs --domain my.dev
```

## hypha hatch - Create or update spore.core.json in working directory

```text
Create or update spore.core.json in working directory

Usage: hatch [OPTIONS]
       hatch <COMMAND>

Commands:
  bond  Manage bonds in spore.core.json
  tree  Manage tree configuration in spore.core.json
  help  Print this message or the help of the given subcommand(s)

Options:
      --id <ID>
          Opaque identifier stored in spore.core.json

      --version <VERSION>
          Version string (e.g., 1.0.0)

      --name <NAME>
          Display name

      --domain <DOMAIN>
          Publisher domain

      --synopsis <SYNOPSIS>
          Short description

      --intent <INTENT>
          Why this spore exists — permanent across releases (repeatable)

      --mutations <MUTATIONS>
          What changed relative to the spawned-from parent (repeatable)

      --license <LICENSE>
          License (SPDX identifier)

  -h, --help
          Print help (see a summary with '-h')

Examples:
  hypha hatch --id my-tool --name "My Tool" --synopsis "A useful tool"
  hypha hatch --intent "Provide a reusable HTTP client for CMN agents" --mutations "Initial release"
  hypha hatch --license MIT --domain cmn.dev

Subcommands:
  hypha hatch bond set/remove/clear   Manage bonds in spore.core.json
  hypha hatch tree set/show            Manage tree configuration
```

### hypha hatch bond - Manage bonds in spore.core.json

```text
Manage bonds in spore.core.json

Usage: bond <COMMAND>

Commands:
  set     Add or update a bond (upsert by URI)
  remove  Remove bonds by URI and/or relation
  clear   Remove all bonds
  help    Print this message or the help of the given subcommand(s)

Options:
  -h, --help
          Print help (see a summary with '-h')

Examples:
  hypha hatch bond set --uri cmn://cmn.dev/b3.abc --relation follows --id my-lib --reason "Core library"
  hypha hatch bond set --uri cmn://cmn.dev/b3.abc --with 'mints=["https://mint.example.com"]'
  hypha hatch bond remove --relation follows
  hypha hatch bond clear
```

#### hypha hatch bond set - Add or update a bond (upsert by URI)

```text
Add or update a bond (upsert by URI)

Usage: set [OPTIONS] --uri <URI>

Options:
      --uri <URI>
          Bond URI (match key)

      --relation <RELATION>
          Bond relation (required for new bonds)

      --id <ID>
          Bond id

      --reason <REASON>
          Bond reason

      --with <KEY=VALUE>
          Bond parameters (KEY=VALUE, value is parsed as JSON; repeatable)

  -h, --help
          Print help
```

#### hypha hatch bond remove - Remove bonds by URI and/or relation

```text
Remove bonds by URI and/or relation

Usage: remove [OPTIONS]

Options:
      --uri <URI>
          Remove bonds matching this URI

      --relation <RELATION>
          Remove bonds matching this relation

  -h, --help
          Print help
```

#### hypha hatch bond clear - Remove all bonds

```text
Remove all bonds

Usage: clear

Options:
  -h, --help
          Print help
```

### hypha hatch tree - Manage tree configuration in spore.core.json

```text
Manage tree configuration in spore.core.json

Usage: tree <COMMAND>

Commands:
  set   Set tree configuration fields
  show  Show current tree configuration
  help  Print this message or the help of the given subcommand(s)

Options:
  -h, --help
          Print help
```

#### hypha hatch tree set - Set tree configuration fields

```text
Set tree configuration fields

Usage: set [OPTIONS]

Options:
      --algorithm <ALGORITHM>
          Hash algorithm (e.g., blob_tree_blake3_nfc)

      --exclude-names <EXCLUDE_NAMES>...
          File/directory names to exclude from hashing (repeatable)

      --follow-rules <FOLLOW_RULES>...
          Ignore-rule files to follow (repeatable)

  -h, --help
          Print help
```

#### hypha hatch tree show - Show current tree configuration

```text
Show current tree configuration

Usage: show

Options:
  -h, --help
          Print help
```

## hypha release - Sign and publish spore to mycelium site

```text
Sign and publish spore to mycelium site

Usage: release [OPTIONS] --domain <DOMAIN>

Options:
      --domain <DOMAIN>
          Target domain (required)

      --source <SOURCE>
          Spore source directory (default: current directory)

      --site-path <SITE_PATH>
          Custom site directory (default: ~/.cmn/mycelium/<domain>)

      --dist-git <DIST_GIT>
          External git repository URL

      --dist-ref <DIST_REF>
          Git ref: tag/branch/commit (requires --dist-git)

      --archive <FORMAT>
          Archive format for release generation (currently only: zstd)

          [default: zstd]

      --dry-run
          Pre-compute URI without writing any files

  -h, --help
          Print help (see a summary with '-h')

Requires `hypha mycelium root` first to set up the site.

Examples:
  hypha release --domain cmn.dev
  hypha release --domain cmn.dev --source ./my-spore
  hypha release --domain cmn.dev --dry-run          # pre-compute URI without releasing
  hypha release --domain cmn.dev --archive zstd
  hypha release --domain cmn.dev --dist-git https://github.com/user/repo --dist-ref v1.0
```

## hypha lineage - Trace spore lineage: descendants (in, default) or ancestors (out)

```text
Trace spore lineage: descendants (in, default) or ancestors (out)

Usage: lineage [OPTIONS] <URI>

Arguments:
  <URI>
          CMN URI (e.g., cmn://cmn.dev/HASH)

Options:
      --direction <DIRECTION>
          Direction: in (descendants, default) or out (ancestors)

          [possible values: in, out]

      --synapse <SYNAPSE>
          Synapse server (domain or URL, default: configured default)

      --synapse-token-secret <SYNAPSE_TOKEN_SECRET>
          Auth token for synapse (overrides configured token)

      --max-depth <MAX_DEPTH>
          Maximum traversal depth (default: 10)

          [default: 10]

  -h, --help
          Print help (see a summary with '-h')

Direction:
  --direction in   Find descendants / forks (default)
  --direction out  Trace ancestors / spawn chain

Examples:
  hypha lineage cmn://cmn.dev/b3.3yMR7vZQ9hL2xKJdFtN8wPcB6sY1mXgU4eH5pTa2 --synapse https://synapse.cmn.dev
  hypha lineage cmn://cmn.dev/b3.3yMR7vZQ9hL2xKJdFtN8wPcB6sY1mXgU4eH5pTa2 --direction out --synapse https://synapse.cmn.dev
  hypha lineage cmn://cmn.dev/b3.3yMR7vZQ9hL2xKJdFtN8wPcB6sY1mXgU4eH5pTa2 --synapse https://synapse.cmn.dev --max-depth 5
```

## hypha search - Search for spores by keyword (semantic search via Synapse)

```text
Search for spores by keyword (semantic search via Synapse)

Usage: search [OPTIONS] <QUERY>

Arguments:
  <QUERY>
          Search query text

Options:
      --synapse <SYNAPSE>
          Synapse server (domain or URL, default: configured default)

      --synapse-token-secret <SYNAPSE_TOKEN_SECRET>
          Auth token for synapse (overrides configured token)

      --domain <DOMAIN>
          Filter by domain

      --license <LICENSE>
          Filter by license (SPDX identifier)

      --bonds <BONDS>
          Filter by bond relationship (format: relation:uri, comma-separated for AND)

      --limit <LIMIT>
          Maximum results (default: 20)

          [default: 20]

  -h, --help
          Print help (see a summary with '-h')

Examples:
  hypha search "protocol spec" --synapse https://synapse.cmn.dev
  hypha search "data format" --synapse https://synapse.cmn.dev --domain cmn.dev
  hypha search "agent tools" --synapse https://synapse.cmn.dev --license MIT --limit 5
  hypha search "http client" --bonds spawned_from:cmn://cmn.dev/b3.abc123
```

## hypha mycelium - Manage local mycelium site

```text
Manage local mycelium site

Usage: mycelium <COMMAND>

Commands:
  root      Establish a new site for a domain (or update existing)
  status    Show site status
  serve     Start a local HTTP server to serve the site (for debugging)
  nutrient  Manage nutrient methods (add/remove/clear)
  pulse     Send a pulse to a synapse indexer
  help      Print this message or the help of the given subcommand(s)

Options:
  -h, --help
          Print help
```

### hypha mycelium root - Establish a new site for a domain (or update existing)

```text
Establish a new site for a domain (or update existing)

Usage: root [OPTIONS] [DOMAIN]

Arguments:
  [DOMAIN]
          Domain name (auto-computed when --hub is used)

Options:
      --hub <HUB>
          Hub domain (e.g., cmnhub.com). Generates a key, computes subdomain from pubkey (ed-<base32>), and sets domain + endpoints automatically

      --site-path <SITE_PATH>
          Custom site directory (default: ~/.cmn/mycelium/<domain>)

      --name <NAME>
          Site or author name

      --synopsis <SYNOPSIS>
          Brief description of the site or author

      --bio <BIO>
          Bio (markdown)

      --endpoints-base <ENDPOINTS_BASE>
          Base URL for endpoints (e.g., https://example.com)

  -h, --help
          Print help (see a summary with '-h')

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
```

### hypha mycelium status - Show site status

```text
Show site status

Usage: status [OPTIONS] [DOMAIN]

Arguments:
  [DOMAIN]
          Domain name (optional, lists all if not specified)

Options:
      --site-path <SITE_PATH>
          Custom site directory

      --id <ID>
          Spore id to resolve from the local mycelium inventory

  -h, --help
          Print help (see a summary with '-h')

Examples:
  hypha mycelium status
  hypha mycelium status cmn.dev
  hypha mycelium status cmn.dev --id cmn-spec --site-path deploy/cmn.dev
```

### hypha mycelium serve - Start a local HTTP server to serve the site (for debugging)

```text
Start a local HTTP server to serve the site (for debugging)

Usage: serve [OPTIONS] [DOMAIN]

Arguments:
  [DOMAIN]
          Domain name

Options:
      --site-path <SITE_PATH>
          Custom site directory

      --port <PORT>
          Port to listen on (default: 8080)

          [default: 8080]

  -h, --help
          Print help (see a summary with '-h')

Examples:
  hypha mycelium serve
  hypha mycelium serve cmn.dev --port 3000
```

### hypha mycelium nutrient - Manage nutrient methods (add/remove/clear)

```text
Manage nutrient methods (add/remove/clear)

Usage: nutrient <COMMAND>

Commands:
  add     Add or update a nutrient method (upsert by type)
  remove  Remove a nutrient method by type
  clear   Remove all nutrient methods
  help    Print this message or the help of the given subcommand(s)

Options:
  -h, --help
          Print help (see a summary with '-h')

Examples:
  hypha mycelium nutrient add cmn.dev --type lightning_address --with address=user@example.com
  hypha mycelium nutrient add cmn.dev --type url --with url=https://example.com --with label=Donate
  hypha mycelium nutrient remove cmn.dev --type url
  hypha mycelium nutrient clear cmn.dev
```

#### hypha mycelium nutrient add - Add or update a nutrient method (upsert by type)

```text
Add or update a nutrient method (upsert by type)

Usage: add [OPTIONS] --type <TYPE> <DOMAIN>

Arguments:
  <DOMAIN>
          Domain name

Options:
      --type <TYPE>
          Nutrient method type (e.g., lightning_address, url, evm, solana)

      --with <KEY=VALUE>
          Nutrient parameters (KEY=VALUE, value is parsed as JSON; repeatable)

      --site-path <SITE_PATH>
          Custom site directory

  -h, --help
          Print help
```

#### hypha mycelium nutrient remove - Remove a nutrient method by type

```text
Remove a nutrient method by type

Usage: remove [OPTIONS] --type <TYPE> <DOMAIN>

Arguments:
  <DOMAIN>
          Domain name

Options:
      --type <TYPE>
          Nutrient method type to remove

      --site-path <SITE_PATH>
          Custom site directory

  -h, --help
          Print help
```

#### hypha mycelium nutrient clear - Remove all nutrient methods

```text
Remove all nutrient methods

Usage: clear [OPTIONS] <DOMAIN>

Arguments:
  <DOMAIN>
          Domain name

Options:
      --site-path <SITE_PATH>
          Custom site directory

  -h, --help
          Print help
```

### hypha mycelium pulse - Send a pulse to a synapse indexer

```text
Send a pulse to a synapse indexer

Usage: pulse [OPTIONS] --file <FILE>

Options:
      --synapse <SYNAPSE>
          Synapse server (domain or URL, default: configured default)

      --synapse-token-secret <SYNAPSE_TOKEN_SECRET>
          Auth token for synapse (overrides configured token)

      --file <FILE>
          Path to signed mycelium.json

  -h, --help
          Print help (see a summary with '-h')

Examples:
  hypha mycelium pulse --synapse synapse.cmn.dev --file ~/.cmn/mycelium/cmn.dev/public/cmn/mycelium/<hash>.json
  hypha mycelium pulse --synapse https://synapse.cmn.dev --file ~/.cmn/mycelium/cmn.dev/public/cmn/mycelium/<hash>.json
```

## hypha synapse - Manage Synapse node connections

```text
Manage Synapse node connections

Usage: synapse <COMMAND>

Commands:
  discover  Discover Synapse instances via the network
  list      List configured Synapse nodes
  health    Check health of a Synapse instance
  add       Add a Synapse node
  remove    Remove a Synapse node
  use       Set default Synapse node
  config    Configure a Synapse node (token, etc.)
  help      Print this message or the help of the given subcommand(s)

Options:
  -h, --help
          Print help
```

### hypha synapse discover - Discover Synapse instances via the network

```text
Discover Synapse instances via the network

Usage: discover [OPTIONS]

Options:
      --synapse <SYNAPSE>
          Synapse to query (domain or URL, default: configured default)

      --synapse-token-secret <SYNAPSE_TOKEN_SECRET>
          Auth token for synapse (overrides configured token)

  -h, --help
          Print help (see a summary with '-h')

Examples:
  hypha synapse discover
  hypha synapse discover --synapse https://synapse.cmn.dev
```

### hypha synapse list - List configured Synapse nodes

```text
List configured Synapse nodes

Usage: list

Options:
  -h, --help
          Print help (see a summary with '-h')

Examples:
  hypha synapse list
```

### hypha synapse health - Check health of a Synapse instance

```text
Check health of a Synapse instance

Usage: health [OPTIONS] [SYNAPSE]

Arguments:
  [SYNAPSE]
          Synapse domain or URL (default: configured default)

Options:
      --synapse-token-secret <SYNAPSE_TOKEN_SECRET>
          Auth token for synapse (overrides configured token)

  -h, --help
          Print help (see a summary with '-h')

Examples:
  hypha synapse health
  hypha synapse health synapse.cmn.dev
  hypha synapse health https://synapse.cmn.dev
```

### hypha synapse add - Add a Synapse node

```text
Add a Synapse node

Usage: add <URL>

Arguments:
  <URL>
          Synapse URL

Options:
  -h, --help
          Print help (see a summary with '-h')

Examples:
  hypha synapse add https://synapse.cmn.dev
```

### hypha synapse remove - Remove a Synapse node

```text
Remove a Synapse node

Usage: remove <DOMAIN>

Arguments:
  <DOMAIN>
          Synapse domain

Options:
  -h, --help
          Print help (see a summary with '-h')

Examples:
  hypha synapse remove synapse.cmn.dev
```

### hypha synapse use - Set default Synapse node

```text
Set default Synapse node

Usage: use <DOMAIN>

Arguments:
  <DOMAIN>
          Synapse domain

Options:
  -h, --help
          Print help (see a summary with '-h')

Examples:
  hypha synapse use synapse.cmn.dev
```

### hypha synapse config - Configure a Synapse node (token, etc.)

```text
Configure a Synapse node (token, etc.)

Usage: config [OPTIONS] <DOMAIN>

Arguments:
  <DOMAIN>
          Synapse domain

Options:
      --token-secret <TOKEN_SECRET>
          Auth token (use empty string to clear)

  -h, --help
          Print help (see a summary with '-h')

Examples:
  hypha synapse config synapse.cmn.dev --token-secret sk-abc123
  hypha synapse config synapse.cmn.dev --token-secret ""    # clear token
```

## hypha cache - Manage local cache

```text
Manage local cache

Usage: cache <COMMAND>

Commands:
  list   List all cached spores
  clean  Remove old or all cached items
  path   Show local filesystem path for a cached spore
  help   Print this message or the help of the given subcommand(s)

Options:
  -h, --help
          Print help
```

### hypha cache list - List all cached spores

```text
List all cached spores

Usage: list

Options:
  -h, --help
          Print help (see a summary with '-h')

Examples:
  hypha cache list
  hypha cache list -o yaml
```

### hypha cache clean - Remove old or all cached items

```text
Remove old or all cached items

Usage: clean [OPTIONS]

Options:
      --all
          Remove all cached items

  -h, --help
          Print help (see a summary with '-h')

Examples:
  hypha cache clean --all
```

### hypha cache path - Show local filesystem path for a cached spore

```text
Show local filesystem path for a cached spore

Usage: path <URI>

Arguments:
  <URI>
          CMN URI (e.g., cmn://cmn.dev/HASH)

Options:
  -h, --help
          Print help (see a summary with '-h')

Examples:
  hypha cache path cmn://cmn.dev/b3.3yMR7vZQ9hL2xKJdFtN8wPcB6sY1mXgU4eH5pTa2
```

## hypha config - View or modify hypha configuration

```text
View or modify hypha configuration

Usage: config <COMMAND>

Commands:
  list  Show current configuration (merged defaults + config.toml)
  set   Set a configuration value
  help  Print this message or the help of the given subcommand(s)

Options:
  -h, --help
          Print help
```

### hypha config list - Show current configuration (merged defaults + config.toml)

```text
Show current configuration (merged defaults + config.toml)

Usage: list

Options:
  -h, --help
          Print help (see a summary with '-h')

Examples:
  hypha config list
  hypha config list -o yaml
```

### hypha config set - Set a configuration value

```text
Set a configuration value

Usage: set <KEY> <VALUE>

Arguments:
  <KEY>
          Config key (dotted path, e.g. cache.cmn_ttl_s)

  <VALUE>
          Value to set

Options:
  -h, --help
          Print help (see a summary with '-h')

Dotted keys map to TOML sections:
  cache.path            Custom cache directory
  cache.cmn_ttl_s       cmn.json cache TTL in seconds
  cache.key_trust_ttl_s Key trust cache TTL in seconds
  cache.key_trust_refresh_mode
                        Key trust refresh mode: expired | always | offline
  cache.key_trust_synapse_witness_mode
                        Key trust fallback when domain is offline: allow | require_domain
  cache.spore_max_download_bytes
                        Max spore archive download bytes
  cache.spore_max_extract_bytes
                        Max total bytes extracted from a spore archive
  cache.spore_max_extract_files
                        Max files extracted from a spore archive
  cache.spore_max_extract_file_bytes
                        Max bytes extracted for one spore archive file
  cache.spore_reject_path_components
                        TOML string array of protected received path components
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
  hypha config set cache.spore_max_download_bytes 1073741824
  hypha config set cache.spore_reject_path_components '[".git", ".cmn"]'
  hypha config set cache.path /tmp/hypha-cache
  hypha config set defaults.synapse synapse.cmn.dev
  hypha config set defaults.taste.synapse cmnhub.com
  hypha config set defaults.taste.domain ed-xxx.cmnhub.com
```

## hypha skill - Install, uninstall, or inspect the bundled Hypha agent skill

```text
Install, uninstall, or inspect the bundled Hypha agent skill

Usage: skill <COMMAND>

Commands:
  status     Report whether the bundled Hypha skill is installed and current
  install    Install or refresh the bundled Hypha skill
  uninstall  Remove the bundled Hypha skill
  help       Print this message or the help of the given subcommand(s)

Options:
  -h, --help
          Print help (see a summary with '-h')

Examples:
  hypha skill status
  hypha skill install --agent codex
  hypha skill install --agent claude-code --scope project
  hypha skill uninstall --agent opencode --skills-dir /tmp/skills --force
```

### hypha skill status - Report whether the bundled Hypha skill is installed and current

```text
Report whether the bundled Hypha skill is installed and current

Usage: status [OPTIONS]

Options:
      --agent <AGENT>
          Agent target to manage

          [default: all]
          [possible values: all, codex, claude-code, opencode]

      --scope <SCOPE>
          Install scope

          [default: personal]
          [possible values: personal, project]

      --skills-dir <SKILLS_DIR>
          Explicit skills directory; requires a single --agent

      --force
          Overwrite or remove an unmanaged skill at the target path

  -h, --help
          Print help
```

### hypha skill install - Install or refresh the bundled Hypha skill

```text
Install or refresh the bundled Hypha skill

Usage: install [OPTIONS]

Options:
      --agent <AGENT>
          Agent target to manage

          [default: all]
          [possible values: all, codex, claude-code, opencode]

      --scope <SCOPE>
          Install scope

          [default: personal]
          [possible values: personal, project]

      --skills-dir <SKILLS_DIR>
          Explicit skills directory; requires a single --agent

      --force
          Overwrite or remove an unmanaged skill at the target path

  -h, --help
          Print help
```

### hypha skill uninstall - Remove the bundled Hypha skill

```text
Remove the bundled Hypha skill

Usage: uninstall [OPTIONS]

Options:
      --agent <AGENT>
          Agent target to manage

          [default: all]
          [possible values: all, codex, claude-code, opencode]

      --scope <SCOPE>
          Install scope

          [default: personal]
          [possible values: personal, project]

      --skills-dir <SKILLS_DIR>
          Explicit skills directory; requires a single --agent

      --force
          Overwrite or remove an unmanaged skill at the target path

  -h, --help
          Print help
```
