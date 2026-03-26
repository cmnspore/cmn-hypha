# Agent Integration Guide

Workflow patterns and skill templates for agents driving the Hypha CLI programmatically. All Hypha commands return structured JSON — parse `code` to branch on success/error.

## 1. Workflow Patterns

### 1.1 Discovery: Find and Inspect Spores

```bash
# Search for spores by keyword (requires Synapse with search enabled)
hypha search "HTTP client" --synapse https://synapse.cmn.dev --limit 5

# Inspect a specific spore (no download)
hypha sense cmn://cmn.dev/b3.<hash>

# Browse a domain's full inventory
hypha sense cmn://cmn.dev

# Query bond graph — descendants (default)
hypha lineage cmn://domain/b3.<hash> --synapse https://synapse.cmn.dev

# Trace ancestors
hypha lineage cmn://domain/b3.<hash> --direction out --synapse https://synapse.cmn.dev
```

**Decision flow:**
1. `search` → pick best result by `score` and `license`
2. `sense` → verify signatures, read synopsis and intent
3. `taste` → fetch to cache, review code, record verdict
4. Proceed to consume (if `safe`) or skip (if `toxic`)

### 1.2 Consumption: Taste, Spawn, and Use Spores

```bash
# Step 1: Taste — fetch to cache and review
hypha taste cmn://domain/b3.<hash>
# → Read code at result.cache_path, compare with result.parent.cache_path if present
# → Optionally pull others' reports: --synapse {SYNAPSE_URL}

# Step 2: Record verdict after reviewing the code
hypha taste cmn://domain/b3.<hash> --verdict safe --notes "Reviewed, no issues"

# Step 3: Spawn (checks taste verdict — blocked if not tasted or toxic)
hypha spawn cmn://domain/b3.<hash> [target_dir]

# Sync with upstream changes (git sources only, also checks taste)
hypha grow
```

**Parse spawn output:**
- `result.source_type`: `"git"` or `"archive"`
- `result.path`: filesystem path to the working copy
- `result.vcs`: `"git"` if a git repo was initialized (can `grow` later)

### 1.3 Evolution: Modify, Merge, and Re-release

**Intent and mutations are arrays** — pass `--intent` and `--mutations` multiple times to add multiple entries. Intent is the most important field: it explains *why* this release exists and is used for search, discovery, and trust evaluation. Write one point per entry, covering every key motivation.

```bash
# Prepare spore metadata (in working copy)
# --intent: WHY (most important — one entry per motivation point)
# --mutations: WHAT (one entry per concrete change)
hypha hatch --domain yourdomain.com \
  --name "My Fork" \
  --intent "Fix critical buffer overflow in HTTP parser (CVE-2025-XXXX)" \
  --intent "Improve input validation for untrusted payloads" \
  --mutations "Patch buffer overflow in src/parser.rs:parse_header()" \
  --mutations "Add bounds checking to all read_bytes() call sites" \
  --mutations "Add regression test for CVE-2025-XXXX"

# Merge code from another spore (prepares for AI-assisted merge)
hypha absorb cmn://other.dev/b3.<hash>

# Sign and generate static files to your mycelium site directory
hypha release --domain yourdomain.com
# Output: ~/.cmn/mycelium/yourdomain.com/public/
#   .well-known/cmn.json
#   cmn/mycelium/{hash}.json
#   cmn/spore/{hash}.json

# Deploy static files to your web server
# CMN is static-file-only — any hosting works (CDN, S3, nginx, Cloudflare, etc.)
rsync -av ~/.cmn/mycelium/yourdomain.com/public/ you@server:/var/www/yourdomain.com/
# Or: aws s3 sync ... | gsutil rsync ... | wrangler deploy ...

# Notify a Synapse indexer (AFTER files are live on the web)
hypha mycelium pulse --synapse https://synapse.cmn.dev \
  --file path/to/spore.json
```

## 2. Skill Templates

Ready-to-use prompt templates for orchestrating Hypha from an agent. `{SYNAPSE_URL}` can be any Synapse instance — a public one or one you run yourself (see [Deploying Synapse](/tools/synapse/synapse-deployment/)).

### 2.1 Discover and Evaluate

```
Given a need for "{CAPABILITY}", find candidate spores:

1. Run: hypha search "{CAPABILITY}" --synapse {SYNAPSE_URL} --limit 10
2. Parse result.results — rank by score, filter by license compatibility with {LICENSE}
3. For top 3 candidates, run: hypha sense <uri>  (URI uses b3.<hash> format)
4. Verify all have trace.verified.core_signature: true
5. For the best candidate, run: hypha taste <uri> --synapse {SYNAPSE_URL}
6. Read code at result.cache_path — check for security issues, intent consistency
7. If safe: hypha taste <uri> --verdict safe --notes "..."
8. Return ranked list with: uri, name, synopsis, license, domain, taste verdict
```

### 2.2 Taste, Spawn, and Integrate

```
Taste and spawn spore {URI} into the project:

1. Run: hypha taste {URI} --synapse {SYNAPSE_URL}
2. Read code at result.cache_path — review for security, compare with result.parent.cache_path
3. Check result.others_tastes for reference (others' verdicts are advisory, not authoritative)
4. Record verdict: hypha taste {URI} --verdict safe --notes "..."
5. Run: hypha spawn {URI} {TARGET_DIR}
6. If code == "NOT_TASTED", go to step 1
7. If code == "DIR_EXISTS", choose a new directory name
8. Read result.path to locate the working copy
9. If result.can_grow == true, note that future updates via `hypha grow` are available
```

### 2.3 Evolve and Release

```
Release changes from {SOURCE_DIR} under {DOMAIN}:

1. Summarize the conversation/task into intent points (WHY):
   - One --intent per key motivation or goal
   - Be specific: "Fix race condition in connection pool" not "Bug fix"
   - Intent is the primary field for search and trust — write it thoroughly
2. Summarize the diff into mutations (WHAT):
   - One --mutations per concrete modification
3. Run: hypha hatch --domain {DOMAIN} \
     --intent "{INTENT_1}" --intent "{INTENT_2}" \
     --mutations "{MUTATION_1}" --mutations "{MUTATION_2}"
4. Verify code == "ok"
5. Run: hypha release --domain {DOMAIN}
6. Parse result.uri for the published spore URI
7. Deploy static files from ~/.cmn/mycelium/{DOMAIN}/public/ to web hosting
8. Run: hypha mycelium pulse --synapse {SYNAPSE_URL} --file <spore_json_path>
9. Return the published URI
```

### 2.4 Full Lifecycle: Taste, Fork, Patch, Publish

```
Taste, fork {SOURCE_URI}, apply a patch, and publish under {DOMAIN}:

1. hypha taste {SOURCE_URI} --synapse {SYNAPSE_URL}
2. Read code at result.cache_path — review for security issues
3. hypha taste {SOURCE_URI} --verdict safe --notes "..." --domain {DOMAIN} --synapse {SYNAPSE_URL}
   (Signs and shares your taste report since you have a domain)
4. hypha spawn {SOURCE_URI} {WORK_DIR}
5. cd {WORK_DIR}
6. Apply modifications to source code
7. hypha hatch --domain {DOMAIN} --name "{NAME}" \
     --intent "{INTENT_1}" --intent "{INTENT_2}" \
     --mutations "{MUTATION_1}" --mutations "{MUTATION_2}"
8. hypha release --domain {DOMAIN}
9. Deploy static files from ~/.cmn/mycelium/{DOMAIN}/public/ to web hosting
10. hypha mycelium pulse --synapse {SYNAPSE_URL} --file <spore_json_path>
11. Return: {published_uri, spawned_from: {SOURCE_URI}}
```

## 3. Error Recovery

### Common Failures and Remediation

| Error Code | Cause | Recovery |
|------------|-------|----------|
| `NOT_TASTED` | Spore has not been tasted yet | Run `hypha taste <uri>`, review code, record verdict |
| `TOXIC` | Spore tasted as toxic (security risk) | Do not proceed; inspect the code or choose an alternative spore |
| `key_fetch_failed` | Domain's cmn.json unreachable | Retry after 30s; fall back to cached data if available |
| `sig_failed` | Signature mismatch (key rotated or tampered) | Re-fetch cmn.json key; if still fails, reject the spore |
| `fetch_failed` | Content download failed | Retry with backoff (5s, 15s, 60s); try alternative `dist` endpoints |
| `DIR_EXISTS` | Spawn target directory already exists | Append suffix or use a different directory name |
| `LOCAL_MODIFIED` | Local files modified | Merge manually using cached old/new content paths shown in hint |
| `NO_SPAWN_REMOTE` | Git repo has no spawn remote | Merge manually using cached old/new content paths shown in hint |
| `hash_mismatch` | Downloaded content doesn't match URI hash | Reject content; try mirror if available |
| `synapse_error` | Search/query failed on Synapse | Check Synapse URL; fall back to direct domain `sense` |

### Retry Strategy

```
For network errors (key_fetch_failed, fetch_failed, synapse_error):
  attempt 1: immediate
  attempt 2: wait 5s
  attempt 3: wait 15s
  attempt 4: wait 60s
  then: report failure with last error
```

## 4. Feature Detection

Check what a Synapse instance supports before using optional endpoints:

```bash
# Search (optional) — returns 503 if not configured
GET {SYNAPSE_URL}/synapse/search?q=test&limit=1

# Graph (optional) — returns 503 if not configured
GET {SYNAPSE_URL}/synapse/graph/stats
```

**Decision matrix:**

| Endpoint | HTTP 200 | HTTP 503 |
|----------|----------|----------|
| `/synapse/search` | Semantic search available | Use `sense` + manual filtering |
| `/synapse/graph/stats` | Graph traversal available | Use `/synapse/spore/:hash/lineage` (storage-based) |
| `/synapse/graph/search` | Relationship search available | Not available — browse lineage manually |
