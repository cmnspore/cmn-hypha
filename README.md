# Hypha

**The Local Site Manager for the Code Mycelial Network (CMN)**

Hypha is a CLI tool for managing your CMN site locally. It handles identity (Ed25519 keypairs), spore creation, signing, and spore consumption (taste, spawn, grow, absorb).

> **Full Reference**: See [docs/cli.md](docs/cli.md) for the generated CLI reference.

## Installation

```bash
cargo build --release
cp target/release/hypha /usr/local/bin/
```

## Quick Start

### Releasing Flow

```bash
# 1. Initialize a site (generates Ed25519 keypair)
hypha mycelium root cmn.dev

# 2. Deploy public/ directory so cmn.json is accessible over HTTPS
# No DNS TXT records required — public key is in cmn.json

# 3. Create spore metadata in your project
cd /path/to/project
hypha hatch --name my-tool --synopsis "A useful tool" --intent "Initial release"

# 4. Release (archive is default)
hypha release --domain cmn.dev

# With external git reference (archive also generated)
hypha release --domain cmn.dev --dist-git https://github.com/user/my-tool --dist-ref $(git rev-parse HEAD)

# 5. Deploy static files
rsync -avz ~/.cmn/mycelium/cmn.dev/public/ user@server:/var/www/
```

### Consuming Flow

```bash
# Discover a spore
hypha sense cmn://cmn.dev/b3.3yMR7vZQ9hL2xKJdFtN8wPcB6sY1mXgU4eH5pTa2

# Taste: fetch to cache and evaluate safety
hypha taste cmn://cmn.dev/b3.3yMR7vZQ9hL2xKJdFtN8wPcB6sY1mXgU4eH5pTa2
# Agent reviews code at cache_path, then records verdict:
hypha taste cmn://cmn.dev/b3.3yMR7vZQ9hL2xKJdFtN8wPcB6sY1mXgU4eH5pTa2 --verdict safe

# Spawn your own project (checks taste verdict first)
hypha spawn cmn://cmn.dev/b3.3yMR7vZQ9hL2xKJdFtN8wPcB6sY1mXgU4eH5pTa2
cd my-tool

# Make changes
vim src/main.rs
git add . && git commit -m "my changes"

# Grow: sync updates from upstream
hypha grow

# Grow + update bonds: also check dependencies for newer versions
hypha grow --bond --synapse https://synapse.cmn.dev

# Absorb changes from other forks (AI-assisted)
hypha absorb cmn://fork.dev/b3.8cQnH4xPmZ2vLkJdRt7wNbA9sF3eYgU1hK6pXq5
# Then: "Read .cmn/absorb/ABSORB.md and help me absorb these changes"

# Publish your version
hypha mycelium root mydomain.com
hypha hatch --domain mydomain.com --intent "My improvements"
hypha release --domain mydomain.com
```

### Sharing Taste Reports (via Hub)

Visitors without their own domain or server can share taste reports through a hosted hub, which auto-assigns a subdomain:

```bash
# One-time setup — generates key, computes subdomain, configures auto-submit
hypha mycelium root --hub cmnhub.com

# Register with the hub
curl -X POST https://cmnhub.com/synapse/pulse -H 'Content-Type: application/json' \
  -d @~/.cmn/mycelium/ed-xxx.cmnhub.com/public/.well-known/cmn.json

# From now on, taste reports are automatically signed and submitted:
hypha taste cmn://cmn.dev/b3.HASH --verdict safe
# → shared: true (auto-submits to cmnhub.com, no --domain or --synapse needed)
```

The `--hub` flag sets up `[defaults.taste]` in config so every subsequent `hypha taste --verdict` automatically signs with your hub domain and submits to the hub.

### Discovery & Evolution Tracking

```bash
# Search for spores by keyword (semantic search)
hypha search "HTTP client library" --synapse https://synapse.cmn.dev
hypha search "database" --synapse https://synapse.cmn.dev --domain cmn.dev --limit 5
hypha search "crypto" --synapse https://synapse.cmn.dev --license MIT

# Query bond graph — descendants (default direction: in)
hypha lineage cmn://cmn.dev/b3.3yMR7vZQ9hL2xKJdFtN8wPcB6sY1mXgU4eH5pTa2 --synapse https://synapse.example.com

# Trace ancestors (direction: out)
hypha lineage cmn://cmn.dev/b3.3yMR7vZQ9hL2xKJdFtN8wPcB6sY1mXgU4eH5pTa2 --direction out --synapse https://synapse.example.com

# Auto-discover and absorb from bonds
hypha absorb --discover --synapse https://synapse.example.com
```

## Commands Overview

```bash
# Releasing: Site management
hypha mycelium root <DOMAIN>                # Initialize site with keypair
hypha mycelium status                  # Show site status
hypha mycelium pulse --synapse <URL> --file <FILE>  # Notify synapse indexer

# Releasing: Spore management
hypha hatch [OPTIONS]                  # Create/update spore.core.json
hypha hatch bond set/remove/clear      # Manage bonds in spore.core.json
hypha hatch tree set/show              # Manage tree configuration
hypha release --domain <DOMAIN> [DIST]       # Sign and release

# Consuming: Discover & taste
hypha sense <URI>                      # Resolve URI, show metadata
hypha taste <URI>                      # Fetch to cache, evaluate safety
hypha taste <URI> --verdict safe       # Record verdict (auto-submits if hub configured)

# Hub setup (for visitors without a domain)
hypha mycelium root --hub cmnhub.com   # Generate key + configure auto-submit

# Consuming: Development
hypha spawn <URI>                      # Derive project with ownership
hypha grow                             # Sync from spawn source
hypha grow --bond --synapse <SYNAPSE>  # Also update + fetch bonds via lineage
hypha absorb <URI>... [--discover]     # Prepare for AI merge

# Consuming: Discovery
hypha search <QUERY> --synapse <SYNAPSE>      # Semantic search for spores
hypha lineage <URI> --synapse <SYNAPSE>         # Trace lineage (--direction in|out)

# Cache management
hypha cache list                       # List cached spores
hypha cache clean [--all]              # Remove cached items
hypha cache path <URI>                 # Show cache path
```

## Output Format

Default is JSON (for AI agents). Hypha writes a stdout JSONL event stream (startup/progress/final result). Use `--output plain` or `--output yaml` for human-readable output. Formatting is handled by `agent-first-data` (`output_json` / `output_plain` / `output_yaml`).

```bash
# JSON (default)
hypha mycelium status cmn.dev
# {"code":"ok","result":{...}}

# Plain (same data, rendered as logfmt key-value pairs)
hypha -o plain mycelium status cmn.dev
# code=ok result.domain=cmn.dev result.public_key=ed25519.5XmkQ9vZP8nL3xJdFtR7wNcA6sY2bKgU1eH9pXb4 result.site_path=/home/user/.cmn/mycelium/cmn.dev result.spore_count=5
```

## Development

```bash
# Build
cargo build

# Test
cargo test

# Manual testing
export CMN_HOME=/tmp/cmn-test
hypha mycelium root test.local
hypha -o plain mycelium status

# Cleanup
rm -rf /tmp/cmn-test && unset CMN_HOME
```

## Documentation

Regenerate the CLI reference with:

```bash
./scripts/generate-cli-doc.sh
```

| Document | Description |
|----------|-------------|
| [docs/cli.md](docs/cli.md) | Generated command reference from `src/cli.rs` |
| [docs/quickstart.md](docs/quickstart.md) | Install and first commands |
| [docs/releasing.md](docs/releasing.md) | Publishing flow and distribution options |
| [docs/evolution.md](docs/evolution.md) | Spawn, grow, absorb, bond, and lineage workflows |
| [docs/error-codes.md](docs/error-codes.md) | Exit codes and error handling |
