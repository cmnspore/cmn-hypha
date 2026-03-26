# Spawn/Grow/Bond/Absorb Workflow Guide

This guide covers the visitor workflow for deriving, syncing, bonding, and merging code from CMN spores.

## 1. Ownership Philosophy

CMN has a unique ownership model:

> **Once you spawn a spore, it becomes your own project.**

| Concept | Traditional Git | CMN |
|---------|-----------------|-----|
| Fork | Copy to your account, maintain link | Spawn and own - no "fork" concept |
| Pull | Update from origin (you don't own it) | Sync with spawn source (you choose to follow) |
| Push | Send changes to remote | Release under your domain |

## 2. Commands Overview

| Command | Purpose | When to Use |
|---------|---------|-------------|
| `spawn` | Derive from verified source with full history | Start developing your version |
| `bond` | Fetch all bonds to `.cmn/bonds/` | After spawn, after adding dependencies |
| `grow` | Sync updates from `spawned_from` source | Following source releases |
| `absorb` | Merge changes from any spore | Cherry-pick features from forks/siblings |

## 3. Spawn - Derive from Source

```bash
hypha spawn cmn://cmn.dev/b3.3yMR7vZQ9hL2xKJdFtN8wPcB6sY1mXgU4eH5pTa2
cd my-spore
```

### Archive Source Workflow (Default)

1. Verify spore signature against domain's public key
2. Download and extract archive to target directory
3. If `--vcs git`: initialize git repository with initial commit
4. Save source manifest to `.cmn/spawned-from/spore.json`

### Git Source Workflow

Used with `--dist git`:

1. Verify spore signature against domain's public key
2. Clone git repository to cache: `~/.cache/cmn/hypha/{domain}/repos/{root_commit}/`
3. Checkout verified ref to target directory
4. If `--vcs git`: keep .git and set up remotes; otherwise remove .git
5. Save source manifest to `.cmn/spawned-from/spore.json`

### After Spawn

`spore.core.json` is **not modified** by spawn — it stays identical to the upstream version. The spawn origin is tracked in `.cmn/spawned-from/spore.json` (local metadata, not part of the content hash).

Without `--vcs`:
```
my-spore/
├── src/
├── spore.core.json    ← Upstream content (unchanged)
└── .cmn/
    └── spawned-from/
        └── spore.json ← Source manifest (local metadata)
```

With `--vcs git`:
```
my-spore/
├── src/
├── spore.core.json
├── .git/
└── .cmn/
    └── spawned-from/
        └── spore.json
```

When ready to release your own version, set domain and key:
```bash
hypha hatch --domain mydomain.com
```

## 4. Grow - Sync with Spawn Source

When the spawn source releases a new version:

```bash
cd my-spore
hypha grow

# Also check bonds (depends_on/follows/extends) for updates + fetch to .cmn/bonds/
hypha grow --bond --synapse https://synapse.cmn.dev
```

### Grow Flow

1. Read spawn origin from `.cmn/spawned-from/spore.json`
2. Query Synapse lineage for newer versions
3. Fetch and verify new spore manifest + signatures
4. **Taste gate** — new version must have been tasted
5. **Local modification check** (see below)
6. Apply update (git checkout or archive replace)
7. Update `.cmn/spawned-from/spore.json` to new version

### Local Modification Check

Grow refuses to overwrite local changes. The check depends on the workspace:

| Workspace | Check | Clean | Dirty |
|-----------|-------|-------|-------|
| No .git | tree hash == spawned_from hash | Archive replace | **Error** + hint |
| .git + spawn remote + git dist | git status | Git fetch + checkout | **Error** + hint |
| .git without spawn remote | git status | **Error** + hint | **Error** + hint |

When blocked, the error includes cache paths for manual merge:
```
Old: ~/.cmn/cache/{domain}/spore/{old_hash}/content/
New: ~/.cmn/cache/{domain}/spore/{new_hash}/content/
```

### Archive Source Grow

For spores without git or without a spawn remote, grow does a full archive replacement (skipping `.git/` and `.cmn/`). `spore.core.json` is replaced with the upstream version — spawn does not modify it, so there is no conflict.

## 5. Bond - Fetch Bonds

Bond reads `spore.core.json` bonds and fetches tasted-safe referenced spores to `.cmn/bonds/`. It excludes `spawned_from` (handled by `grow`) and `absorbed_from` (historical — the merge is already done):

```bash
cd my-spore
hypha bond
```

### What Hypha Does

1. Read all bonds from `spore.core.json`
2. For each bond: check taste verdict (must be tasted)
3. Fetch tasted-safe spores to `.cmn/bonds/{id-or-hash}/` (uses `id` when present)
4. Write `bonds.json` index mapping dir names to hashes, URIs, relations, and names

### Directory Structure

```
my-spore/
├── src/
├── spore.core.json
└── .cmn/                    ← gitignored
    └── bonds/
        ├── bonds.json       ← index of all bonds
        ├── signing-lib/       ← depends_on (id="signing-lib")
        │   ├── spore.json
        │   └── content/
        └── b3.8cQnH4xPm.../   ← follows (no id — hash used)
            ├── spore.json
            └── content/
```

### Build Integration

For `depends_on` bonds, use `bonds.json` to look up the dir name and set up build system paths pointing to `.cmn/bonds/{id-or-hash}/content/`:

```toml
# Cargo.toml
[dependencies]
parsing_lib = { path = ".cmn/bonds/signing-lib/content" }
```

```json
// package.json
{ "dependencies": { "parsing-lib": "file:.cmn/bonds/signing-lib/content" } }
```

### When to Bond

| Scenario | Command |
|----------|---------|
| After spawn | `hypha spawn <uri> --bond` or `hypha bond` |
| After grow | `hypha grow --bond` (also checks for bond updates via lineage) |
| After absorb added new `depends_on` | `hypha bond` |
| After editing spore.core.json bonds | `hypha bond` |
| Clean up orphaned bonds | `hypha bond --clean` |

### Taste Gate

Every bonded spore must be individually tasted as `safe` before bond places it in your working directory. No shortcuts — trusting one spore from a domain does not extend to other spores on that domain.

```bash
# Taste each reference first
hypha taste cmn://pub.com/b3.8cQnH4xPmZ2vLkJdRt7wNbA9sF3eYgU1hK6pXq5
hypha taste cmn://cmn.dev/b3.3yMR7vZQ9hL2xKJdFtN8wPcB6sY1mXgU4eH5pTa2

# Then bond-fetch
hypha bond
```

## 6. Absorb - AI-Assisted Merge

Unlike `grow` (which syncs from `spawned_from`), `absorb` prepares code from any spore(s) for AI-assisted merging:

```bash
# Single source
hypha absorb cmn://other.com/b3.8cQnH4xPmZ2vLkJdRt7wNbA9sF3eYgU1hK6pXq5

# Multiple sources
hypha absorb cmn://spawn-a.com/... cmn://spawn-b.com/...

# Auto-discover from lineage
hypha absorb --discover --synapse https://synapse.example.com
```

### What Hypha Does

1. Fetch and verify each target spore signature
2. Extract code to `.cmn/absorb/{hash}/`
3. Generate `.cmn/absorb/ABSORB.md` with structured prompt

### Directory Structure

```
.cmn/absorb/
├── ABSORB.md                    # AI prompt (main entry point)
├── b3.3yMR7vZQ9.../             # Source 1
│   ├── spore.json               # Full spore manifest
│   └── content/                 # Extracted source code
├── b3.3yMR7vZQ9....report.md   # AI-generated report
└── b3.8cQnH4xPm.../             # Source 2
    ├── spore.json
    └── content/
```

### AI Workflow Phases

1. **Analyze Each Source** - Create detailed report per source
2. **Cross-Source Analysis** - Detect overlaps and conflicts
3. **Merge Plan** - Propose plan, wait for user approval
4. **Execute Merge** - Apply approved changes
5. **Update Bonds** - Add `absorbed_from` entries to spore.core.json
6. **Cleanup** - Remove `.cmn/absorb/`

### Usage with AI

```
"Read .cmn/absorb/ABSORB.md and help me absorb these changes"
```

### Key Differences from Grow

| Aspect | grow | absorb |
|--------|------|--------|
| Source | `spawned_from` reference only | Any spore URI(s) |
| Count | Single source | Multiple sources supported |
| Execution | Hypha performs git merge | AI agent handles merge |
| Workflow | Automatic | Phased with user decisions |
| Relation | Updates `.cmn/spawned-from/` | Adds `absorbed_from` entries |

## 7. Evolution Tracking

Query the evolution network via Synapse:

```bash
# Who did this spore come from?
hypha ancestors cmn://... --synapse https://synapse.example.com

# Who forked/evolved this spore?
hypha lineage cmn://... --synapse https://synapse.example.com
```

## 8. Releasing Your Version

After modifying a spawned spore:

```bash
# 1. Set up your own domain (one-time)
hypha mycelium root --domain mydomain.com

# 2. Update spore.core.json
hypha hatch --domain mydomain.com --intent "Added my feature"

# 3. Replicate non-self bonds to your domain (recommended)
hypha replicate --bonds --domain mydomain.com

# 4. Release under your domain
hypha release --domain mydomain.com

# Result: Two independent spores exist
# cmn://cmn.dev/b3.3yMR7vZQ9hL2xKJdFtN8wPcB6sY1mXgU4eH5pTa2   (original)
# cmn://mydomain.com/b3.8cQnH4xPmZ2vLkJdRt7wNbA9sF3eYgU1hK6pXq5   (yours)
# Plus: all bonds replicated on mydomain.com
```

### Replicate Convention

Publishers SHOULD replicate all bonds that point to other domains before releasing. This ensures visitors can reach all bonded spores from your domain alone.

```bash
# Replicate a single spore
hypha replicate cmn://other.com/b3.8cQnH4xPmZ2vLkJdRt7wNbA9sF3eYgU1hK6pXq5 --domain mydomain.com

# Replicate all non-self bonds and update spore.core.json URIs
hypha replicate --bonds --domain mydomain.com
```

A replicate is an exact copy (same hash, same `core`). The original authorship is preserved in `core.domain` — your domain only re-signs the capsule.

## 9. Local Cache Structure

```
~/.cache/cmn/hypha/
└── {domain}/
    ├── mycelium/
    │   ├── cmn.json            # Domain entry point cache (includes public key)
    │   ├── mycelium.json      # Full mycelium manifest
    │   └── status.json        # Fetch status
    │
    ├── spore/
    │   └── {hash}/            # Downloaded spore
    │       ├── spore.json
    │       └── content/
    │
    └── repos/                 # Git repository cache
        └── {root_commit}/     # Identified by first commit SHA
            └── .git/          # Bare repository (verified)
```

**Why `root_commit` as identifier:**
- Stable: Never changes, even with new commits
- Unique: Each repository has unique first commit
- Secure: Verifiable to confirm same repository

## 10. Bond Types

| Relation | Description | Grow | Bond |
|----------|-------------|------|------|
| `spawned_from` | Source derived from | Syncs to latest | Excluded (handled by grow) |
| `absorbed_from` | Source merged into this | — | Excluded (historical) |
| `depends_on` | Runtime/build dependency | — | Fetched to `.cmn/bonds/` (build system uses this) |
| `follows` | Convention/standard adhered to | — | Fetched to `.cmn/bonds/` |
| `extends` | Strain family hierarchy | — | Fetched to `.cmn/bonds/` |

Each bond has `uri` (required), `relation` (required), and optional `reason` (explains why this bond exists). See [§2.4 Bond Types](/spec/03-spore/#24-bond-types) for details.

## 11. Complete Workflow Diagram

```
┌─────────────────────────────────────────────────────────────────┐
│                     PUBLISHER (cmn.dev)                          │
│                                                                  │
│   develop → release → publish                                    │
│                           │                                      │
│                           ▼                                      │
│            cmn://cmn.dev/b3.3yMR7vZQ9hL2x...               │
└─────────────────────────────────────────────────────────────────┘
                                │
                                │ hypha taste → hypha spawn
                                ▼
┌─────────────────────────────────────────────────────────────────┐
│                           VISITOR                                │
│                                                                  │
│   ~/projects/my-spore/                                          │
│   ├── src/                                                      │
│   ├── spore.core.json  (upstream content, unchanged by spawn)    │
│   ├── .git/                                                     │
│   └── .cmn/bonds/      (bonds)                                  │
│                           │                                      │
│                           │ hypha bond (fetch tasted bonds) │
│                           │ develop & commit locally             │
│                           ▼                                      │
│   hypha grow [--bond]                                            │
│   ├── Phase 1: fetch to cache (network, verify)                 │
│   └── Phase 2: git checkout / archive replace (local)            │
│                           │                                      │
│                           │ ready to publish your version        │
│                           ▼                                      │
│   hypha release --domain mydomain.com                                  │
│                           │                                      │
│                           ▼                                      │
│                   cmn://mydomain.com/b3.8cQnH4xPm...       │
└─────────────────────────────────────────────────────────────────┘
```
