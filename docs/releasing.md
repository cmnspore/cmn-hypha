# Releasing Spores

Complete guide to releasing code on the CMN network.

## Overview

Publishing on CMN means making your code discoverable and verifiable through your own domain. You maintain full sovereignty - no central registry controls your packages.

## Setup

### 1. Domain Identity

Your domain is your identity. Initialize it with an Ed25519 keypair:

```bash
hypha mycelium root yourdomain.com
```

This generates a keypair and creates `cmn.json` containing your public key. Deploy the `public/` directory to your domain's web server so that `https://yourdomain.com/.well-known/cmn.json` is accessible. No DNS TXT records are required.

**Key Management:**
- Private key: `$CMN_HOME/mycelium/{domain}/keys/private.pem`
- Back up your private key securely
- Losing it means losing publish ability for that domain

### 2. Mycelium Setup

Your mycelium is your site's catalog:

```bash
# Create mycelium.json
hypha mycelium root yourdomain.com --name "Your Name" --bio "Description"
```

## Creating Spores

### Initialize

```bash
cd your-project
hypha hatch --domain yourdomain.com
```

This creates `spore.core.json`:

```json
{
  "id": "cmn-spec",
  "domain": "cmn.dev",
  "name": "CMN Protocol Specification",
  "synopsis": "Code Mycelial Network - A sovereign-first protocol for code distribution",
  "intent": ["spec simplification and strict domain validation"],
  "mutations": [
    "§05 URI: remove regex and pseudocode, use prose parsing steps",
    "§05 URI §6: domain must be lowercase RFC 1123, reject uppercase instead of normalizing"
  ],
  "license": "CC0-1.0",
  "bonds": [],
  "tree": {
    "algorithm": "blob_tree_blake3_nfc",
    "exclude_names": [".git", ".cmn"],
    "follow_rules": [".gitignore"]
  }
}
```

### Metadata Fields

| Field | Required | Description |
|-------|----------|-------------|
| `id` | Yes | Unique identifier within your domain |
| `domain` | Yes | Your publishing domain |
| `name` | Yes | Human-readable name |
| `synopsis` | Yes | One-line description |
| `intent` | Yes | Why this release exists (array of strings) |
| `mutations` | No | What changed relative to parent (array of strings) |
| `license` | Yes | SPDX license identifier |
| `bonds` | No | Links to related spores |

### Bonds

Link to related spores in `spore.core.json`:

```json
{
  "bonds": [
    {
      "uri": "cmn://cmn.dev/b3.8cQnH4xPmZ2vLkJdRt7wNbA9sF3eYgU1hK6pXq5",
      "relation": "depends_on",
      "id": "signing-lib",
      "reason": "Provides Ed25519 signature verification"
    }
  ]
}
```

**Bond fields:** `uri` (required), `relation` (required), `id` (optional — used as bond directory name under `.cmn/bonds/`), `reason` (optional — why this bond exists).

**Relation Types (in spore.core.json):**
- `depends_on` - Runtime/build dependency
- `follows` - Convention/standard adhered to
- `extends` - Strain family hierarchy

**Auto-injected at release time** (must not appear in spore.core.json):
- `spawned_from` - Derived from this spore (read from `.cmn/spawned-from/`)
- `absorbed_from` - Merged changes from this spore
- `depends_on` - Runtime dependency
- `follows` - Convention or standard this spore adheres to
- `extends` - Strain family hierarchy

## Release Methods

### Archive (Default)

Archives are the default and recommended distribution method:

```bash
hypha release --domain yourdomain.com
```

**Result:**
- Generates `.tar.zst` archive (zstd compressed)
- Deterministic: reproducible tar (sorted files, zero timestamps)
- Visitors download and extract

**Format options:**

```bash
hypha release --domain yourdomain.com --archive zstd   # default and currently the only supported generation format
```

### External Git Reference

For projects hosted on public git providers:

```bash
hypha release --domain yourdomain.com --dist-git https://github.com/user/repo --dist-ref v1.0.0
```

**Result:**
- Spore points to external git URL + ref
- Archive is also generated as fallback
- Visitors can choose git or archive source

## Publishing Workflow

### 1. Prepare Release

Ensure your `spore.core.json` has intent set:

```bash
hypha hatch --intent "v1.0 release"
```

### 2. Release

```bash
hypha release --domain yourdomain.com
```

This signs and publishes the spore with a zstd archive, and updates the mycelium catalog in one step.

### 3. Notify Network (Optional)

Push to Synapse for discovery:

```bash
hypha mycelium pulse --synapse https://synapse.example.com --file ~/.cmn/mycelium/yourdomain.com/public/cmn/mycelium/<hash>.json
```

## Hosting Requirements

### Static Files

Your domain must serve:

```
https://yourdomain.com/.well-known/cmn.json              # Domain entry point
https://yourdomain.com/cmn/mycelium/b3.<hash>.json   # Mycelium manifest
https://yourdomain.com/cmn/spore/b3.<hash>.json      # Spore manifests
https://yourdomain.com/cmn/archive/<name>.tar.zst          # Archive files
```

Any static file hosting works: CDN, S3, nginx, Cloudflare Workers, etc.

## Updating Releases

### New Version

```bash
# Make changes and update intent
hypha hatch --intent "New feature"

# Release (signs spore and updates mycelium)
hypha release --domain yourdomain.com
```

### Deprecation

Add to spore.core.json:

```json
{
  "deprecated": true,
  "deprecated_message": "Use cmn://yourdomain.com/b3.<newversion> instead"
}
```

## Best Practices

1. **Semantic Versioning** - Use git tags like `v1.0.0`
2. **Clear Intent** - Write descriptive intent fields
3. **License Explicitly** - Always specify SPDX identifier
4. **Reference Origins** - Credit spawned_from sources
5. **Backup Keys** - Store private keys securely offline
