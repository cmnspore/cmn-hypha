# Quick Start

New to CMN? Read [Concepts](concepts/) first for an overview of key terms and the trust model.

Get started with CMN in under 5 minutes.

## Prerequisites

- Rust toolchain (for building Hypha)
- A domain you control (for releasing spores) — for taste-only participation, `--hub` auto-assigns a subdomain on a hosted hub

## Install Hypha

```bash
# Download and extract
curl -L https://cmn.dev/download/cmn-tools -o cmn-tools.tar.zst
mkdir cmn-tools && tar --zstd -xf cmn-tools.tar.zst -C cmn-tools

# Build and install
cd cmn-tools
cargo build --release -p hypha
cp target/release/hypha ~/.local/bin/
```

## Try It: Explore a Live Spore

The CMN Protocol Specification is published on cmn.dev. Try these commands:

### 1. Inspect a Spore

```bash
hypha sense cmn://cmn.dev/b3.3yMR7vZQ9hL2xKJdFtN8wPcB6sY1mXgU4eH5pTa2
```

```json
{
  "code": "ok",
  "result": {
    "spore": {
      "$schema": "https://cmn.dev/schemas/v1/spore.json",
      "capsule": {
        "uri": "cmn://cmn.dev/b3.3yMR7vZQ9hL2xKJdFtN8wPcB6sY1mXgU4eH5pTa2",
        "core": {
          "name": "CMN Protocol Specification",
          "domain": "cmn.dev",
          "synopsis": "Code Mycelial Network - A sovereign-first protocol for code distribution",
          "intent": [
            "Switch to archive-first distribution model",
            "Update spec examples to reflect archive dist format instead of managed git repos"
          ],
          "license": "CC0-1.0",
          "mutations": [
            "§03 Spore: update dist examples from git repos to archive URLs",
            "§03 Spore: update mirror example to use archive dist",
            "§03 Spore: rename §6 from 'Git Distribution' to generic",
            "§E2E example: replace --git-repo with default archive release"
          ],
          "bonds": [
            {
              "uri": "cmn://cmn.dev/b3.8cQnH4xPmZ2vLkJdRt7wNbA9sF3eYgU1hK6pXq5",
              "relation": "spawned_from"
            }
          ],
          "tree": { "algorithm": "blob_tree_blake3_nfc", "exclude_names": [".git", ".cmn"], "follow_rules": [".gitignore"] }
        },
        "core_signature": "ed25519.5XmkQ9vZP8nL3xJdFtR7wNcA6sY2bKgU1eH9pXb4...",
        "dist": [
          { "type": "archive" }
        ]
      },
      "capsule_signature": "ed25519.5XmkQ9vZP8nL3xJdFtR7wNcA6sY2bKgU1eH9pXb4..."
    }
  },
  "trace": {
    "uri": "cmn://cmn.dev/b3.3yMR7vZQ9hL2xKJdFtN8wPcB6sY1mXgU4eH5pTa2",
    "cmn": { "resolved": true },
    "verified": { "core_signature": true, "capsule_signature": true }
  }
}
```

### 2. Browse a Domain

```bash
hypha sense cmn://cmn.dev
```

```json
{
  "code": "ok",
  "result": {
    "mycelium": {
      "$schema": "https://cmn.dev/schemas/v1/mycelium.json",
      "capsule": {
        "uri": "cmn://cmn.dev",
        "core": {
          "name": "cmn.dev",
          "domain": "cmn.dev",
          "synopsis": "",
          "updated_at_epoch_ms": 1770954854836,
          "spores": [
            {
              "id": "cmn-spec",
              "hash": "b3.3yMR7vZQ9hL2xKJdFtN8wPcB6sY1mXgU4eH5pTa2",
              "name": "CMN Protocol Specification",
              "synopsis": "Code Mycelial Network - A sovereign-first protocol for code distribution"
            },
            {
              "id": "cmn-tools",
              "hash": "b3.8cQnH4xPmZ2vLkJdRt7wNbA9sF3eYgU1hK6pXq5",
              "name": "CMN Tools",
              "synopsis": "Command-line tools for CMN protocol (hypha, synapse, substrate)"
            }
          ]
        },
        "core_signature": "ed25519.5XmkQ9vZP8nL3xJdFtR7wNcA6sY2bKgU1eH9pXb4..."
      },
      "capsule_signature": "ed25519.5XmkQ9vZP8nL3xJdFtR7wNcA6sY2bKgU1eH9pXb4..."
    }
  },
  "trace": {
    "uri": "cmn://cmn.dev",
    "cmn": { "resolved": true },
    "verified": { "core_signature": true, "capsule_signature": true }
  }
}
```

### 3. Spawn Your Own Copy

```bash
hypha spawn cmn://cmn.dev/b3.3yMR7vZQ9hL2xKJdFtN8wPcB6sY1mXgU4eH5pTa2

cd cmn-spec
# You own it - develop freely
```

## Sharing Taste Reports (via Hub)

A hosted hub auto-assigns you a subdomain (`ed-xxx.cmnhub.com`) derived from your public key, so you can sign and share taste reports without your own server:

```bash
# One-time setup — generates key pair, configures auto-submit
hypha mycelium root --hub cmnhub.com

# Register with the hub (submit the generated cmn.json)
curl -X POST https://cmnhub.com/synapse/pulse -H 'Content-Type: application/json' \
  -d @~/.cmn/mycelium/ed-xxx.cmnhub.com/public/.well-known/cmn.json
```

After setup, every taste verdict is automatically signed and submitted:

```bash
hypha taste cmn://cmn.dev/b3.HASH --verdict safe --notes "Reviewed source"
# → shared: true
```

No `--domain` or `--synapse` flags needed — the `--hub` setup configured `[defaults.taste]` in your config.

## Releasing Spores

### 1. Initialize Your Domain

```bash
# Generate keypair and create site structure
hypha mycelium root yourdomain.com
```

This generates an Ed25519 keypair and creates `cmn.json` with your public key. Deploy the `public/` directory to your domain's web server.

### 2. Create a Spore

```bash
cd your-project

# Initialize spore metadata
hypha hatch --domain yourdomain.com

# Edit spore.core.json with your project details
```

### 3. Release

```bash
# Release (default: zstd archive)
hypha release --domain yourdomain.com

# With external git reference
hypha release --domain yourdomain.com --dist-git https://github.com/user/repo --dist-ref v1.0.0
```

Your spore is now available at:
```
cmn://yourdomain.com/b3.<hash>
```

## Next Steps

- [Releasing Spores](./releasing.md) - Detailed releasing workflow
- [Spawn/Grow/Bond/Absorb Workflow](./evolution.md) - Spawning, syncing, bonding, and evolving spores
- [Hypha CLI Reference](cli.md) - Complete command reference
