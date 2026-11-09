# Error Codes

Reference for error codes and messages in CMN tools.

## Hypha Errors

### Pipeline Error Codes

These codes correspond to stages in the visitor resolution pipeline (sense/taste/spawn/grow/absorb). Each error is a strict AFDATA event with a `trace.duration_ms` field.

| Code | Stage | Cause | Resolution |
|------|-------|-------|------------|
| `invalid_uri` | URI parse | Malformed CMN URI | Check URI syntax: `cmn://domain/b3.<hash>` |
| `key_fetch_failed` | Key fetch | cmn.json fetch or public key extraction failed | Check domain's cmn.json, check network |
| `cmn_failed` | cmn.json | cmn.json endpoint extraction failed | Check domain's cmn.json, check network |
| `manifest_failed` | Manifest | Mycelium or spore manifest fetch failed | Verify URI, domain may have removed it |
| `sig_failed` | Verification | Ed25519 signature invalid | Contact publisher, possible tampering |
| `hash_mismatch` | Hash check | Content doesn't match URI hash | Re-fetch, possible corruption |
| `spore_not_found` | Catalog | Spore not in mycelium catalog | Verify hash, spore may have been removed |
| `fetch_failed` | Download | Content download (HTTPS/git) failed | Check network, try later |

### Command-Specific Error Codes

| Code | Cause | Resolution |
|------|-------|------------|
| `DIR_EXISTS` | Target directory already exists | Remove directory first |
| `dir_error` | Directory creation failed | Check filesystem permissions |
| `spawn_error` | Spawn git/archive operation failed | Check git URL, disk space |
| `grow_error` | Grow local operation failed | Check git state, commit changes first |
| `LOCAL_MODIFIED` | Local files modified since spawn | Merge manually using cached old/new content paths shown in hint |
| `NO_SPAWN_REMOTE` | Git repo has no spawn remote, cannot auto-update | Merge manually using cached old/new content paths shown in hint |
| `GIT_URL_CHANGED` | Git repository URL changed since spawn | Re-spawn with new repository |
| `REPO_IDENTITY_ERR` | Root commit mismatch | Repository was recreated, re-spawn |
| `bond_error` | Bond operation failed | Check spore.core.json, disk space |
| `bond_read_failed` | Bond index could not be read | Check read permissions; delete a damaged `.cmn/bonds/bonds.json` and rerun bond to rebuild (historic directory names may change) |
| `bond_index_invalid` | Bond index is not valid JSON or has invalid entries | Delete `.cmn/bonds/bonds.json` and rerun bond to rebuild (historic directory names may change) |
| `bond_write_failed` | Bond index could not be saved atomically | Check permissions and disk space; symbolic-link targets are rejected |
| `bond_clean_failed` | A bond directory could not be removed or inspected | Fix the reported filesystem error and rerun; failed indexed deletions remain in the index |
| `bond_relation_conflict` | A desired sync URI already belongs to another relation | Use another URI or explicitly remove the existing bond; check and apply both exit 1 without writing |
| `bond_spec_read_failed` | `hatch bond sync` couldn't read `--spec` (file or stdin) | Check the path exists and is readable, or that stdin is being piped |
| `bond_spec_invalid` | `hatch bond sync` `--spec` is not valid JSON, or an entry is missing `id` or has an empty `domain` | Fix the spec: a JSON array of `{id, domain?, reason?, with?, uri?}` objects |
| `bond_site_invalid` | `hatch bond sync` `--site` is not `DOMAIN=PATH`, names a domain twice, or overlaps `--site-path`; or `--site-path` was given without `--domain` | Pass one `--site DOMAIN=PATH` per domain, and give `--site-path` only together with `--domain` |
| `missing_domain` | `hatch bond sync` needs a domain to resolve a spec entry's `uri` | Pass `--domain` (and `--site-path` or `--site` for a non-default site), or give the entry a `domain` or an explicit `uri` |
| `bond_uri_unresolved` | `hatch bond sync` couldn't resolve a spec entry's `uri`: not deployed, and no sibling `spore.core.json` for the dry-run fallback | Release the dependency first, or give the entry an explicit `uri` |
| `absorb_error` | Absorb local operation failed | Check permissions, disk space |
| `synapse_error` | Synapse query failed | Check Synapse URL and network |
| `NOT_TASTED` | Spore has not been tasted | Run `hypha taste <uri>`, review, record verdict |
| `TOXIC` | Spore tasted as toxic | Do not proceed; choose an alternative |

### Publisher Error Codes

| Code | Cause | Resolution |
|------|-------|------------|
| `init_error` | Failed to initialize site | Check permissions |
| `NO_SITE` | Site directory not found | Run `hypha mycelium root domain` |
| `NO_SPORE` | spore.core.json not found | Run `hypha hatch` first |
| `validation_error` | spore.core.json validation failed | Fix required fields |
| `sign_error` | Failed to sign manifest | Check private key |
| `invalid_args` | Invalid command arguments | Check required flags |
| `write_error` | Failed to write file | Check permissions |

### Runtime Diagnostic Codes

Non-fatal diagnostics are strict AFDATA `log` or `progress` events. With the default `--output-to split` routing they go to stderr; `--output-to stdout` or `--output-to stderr` can collapse all event kinds onto one ordered stream. Stable service log events use lowercase names such as `startup`, `server_started`, `http_ok`, `http_forbidden`, and `http_not_found`; generic library warnings use `event: "warn"`.

## Synapse Errors

### API Errors

| HTTP | Code | Message | Cause |
|------|------|---------|-------|
| 400 | `invalid_capsule` | Invalid JSON or schema | Fix the signed CMN document |
| 400 | `field_missing` | Required query field absent or invalid | Check the endpoint's query parameters |
| 400 | `cursor_invalid` | Invalid or overflowing search cursor | Use the cursor returned by the previous result page |
| 404 | `not_found` | Resource not found | Spore/domain not indexed |
| 422 | `uri_invalid` | Invalid URI format | Fix URI syntax |
| 422 | `hash_invalid` | Content hash mismatch | Verify the signed document's URI hash |
| 422 | `invalid_signature` | Signature or signing key invalid | Verify publisher identity and signature |
| 502 | `key_fetch_failed` | Domain identity could not be fetched | Check the domain and retry |
| 500 | `internal_error` | Server/storage error | Report to operator |

### Pulse-Specific Errors

| Code | Message | Cause |
|------|---------|-------|
| `version_not_newer` | Timestamp is not newer than cached | HTTP 409; mycelium already has this or a newer version |
| `timestamp_future` | Timestamp too far ahead | HTTP 422; check `pulse.max_clock_skew_s` or `pulse.max_taste_clock_skew_s` and system time |
| `rate_limited` | Domain notification received too recently | HTTP 429; retry after the cooldown |
| `cmn_fetch_failed` | Live domain entry could not be verified | HTTP 502; check the deployed `cmn.json` |

### Storage Errors

| Code | Message | Cause |
|------|---------|-------|
| `internal_error` | Storage operation failed | Check server logs, database connectivity, and disk space |

Codes are case-sensitive. Branch on the returned `error.code`; the tables list
common errors rather than every rejection an endpoint can produce.

## Error Response Format

### Hypha CLI — Error

The nested `error.code` field contains the specific error code. Parse `error.code` to branch on error type:

```json
{
  "kind": "error",
  "error": {
    "code": "sig_failed",
    "message": "Core signature verification failed: invalid signature",
    "retryable": false,
    "hint": "read the error field, check hypha --help for the expected input, and retry"
  },
  "trace": {
    "duration_ms": 0
  }
}
```

```json
{
  "kind": "error",
  "error": {
    "code": "invalid_uri",
    "message": "URI must start with 'cmn://'",
    "retryable": false,
    "hint": "use a CMN URI in the form cmn://domain or cmn://domain/b3.hash"
  },
  "trace": {
    "duration_ms": 0
  }
}
```

### Hypha CLI — Success with Trace

`sense` returns a `trace` field with resolution metadata (key fetch, caching, verification):

```json
{
  "kind": "result",
  "result": { "spore": { "..." : "..." } },
  "trace": {
    "cmn_url": "cmn://cmn.dev/b3.3yMR7vZQ9hL2xKJdFtN8wPcB6sY1mXgU4eH5pTa2",
    "cmn": { "resolved": true, "cached": true, "public_key": "ed25519.5XmkQ9vZP8nL3xJdFtR7wNcA6sY2bKgU1eH9pXb4" },
    "verified": { "core_signature": true, "capsule_signature": true }
  }
}
```

### Synapse API — Error

```json
{
  "kind": "error",
  "error": {
    "code": "invalid_signature",
    "message": "Signature verification failed for domain cmn.dev",
    "retryable": false,
    "hint": "verify the signing key and signature before retrying"
  },
  "trace": {
    "storage": "redb"
  }
}
```

### Synapse API — Success

Success responses include a `result` field with query data and a `trace` field with internal state:

```json
{
  "kind": "result",
  "result": {
    "query": { "hash": "b3.3yMR7vZQ9hL2xKJdFtN8wPcB6sY1mXgU4eH5pTa2", "direction": "inbound", "max_depth": 10 },
    "bonds": [],
    "total": 0,
    "max_depth_reached": false
  },
  "trace": {
    "storage": "redb"
  }
}
```

### Synapse Access Log

Every request produces a protocol-v1 log event. Request fields and optional `result`/`error` data live under `log`; `trace` merges request metadata with handler-level state.

JSON format (`log_format: json`):

```json
{
  "kind": "log",
  "log": {
    "method": "GET",
    "path": "/synapse/spore/b3.3yMR7vZQ9hL2xKJdFtN8wPcB6sY1mXgU4eH5pTa2",
    "status_code": 404,
    "error": {
      "code": "not_found",
      "message": "Spore not found",
      "retryable": false,
      "hint": "verify the domain/hash and wait for indexing before retrying"
    }
  },
  "trace": {
    "duration_ms": 1
  }
}
```

Plain format (`log_format: plain`):

```text
kind=log log.error.code=not_found log.error.message="Spore not found" log.error.retryable=false log.method=GET log.path=/synapse/spore/b3.3yMR7vZQ9hL2xKJdFtN8wPcB6sY1mXgU4eH5pTa2 log.status_code=404 trace.duration=1ms
```

The plain format is generated from the same JSON value using `agent_first_data::output_plain` (logfmt-style key/value output).

## Exit Codes

Hypha CLI exit codes:

| Code | Meaning |
|------|---------|
| 0 | Success |
| 1 | Domain or runtime failure; inspect `error.code` for network, verification, Git, or permission errors |
| 2 | Structural CLI rejection before the action runs |
| 4 | Output or stream redirection failure |
| 7 | `hatch bond sync --check` found drift (structured diff on stdout); pure order or group-position changes report `reordered=true` with empty `to_add`/`to_update`/`to_remove` lists. Conflicts remain general errors (1). |

## Debugging

### Structured Startup Diagnostics

```bash
hypha --log startup taste cmn://cmn.dev/b3.3yMR7vZQ9hL2xKJdFtN8wPcB6sY1mXgU4eH5pTa2
```

### Capture One Ordered Event Stream

```bash
hypha --output-to stdout --log startup taste cmn://cmn.dev/b3.3yMR7vZQ9hL2xKJdFtN8wPcB6sY1mXgU4eH5pTa2
```

Startup diagnostics include parsed arguments, redacted configuration, and
environment metadata. Use `--help` for the supported output and log selectors.

## Common Issues

### "Key fetch failed"

1. Verify `https://yourdomain.com/.well-known/cmn.json` is accessible
2. Check that the primary capsule in `cmn.json` contains a valid `key` field
3. Ensure HTTPS is properly configured on the domain

### "Signature verification failed"

1. Verify you're fetching the correct URI
2. Check if publisher updated their public key in cmn.json
3. Try fetching from Synapse mirror if available

### "Hash mismatch"

1. Inspect `hypha cache list`; if all cached content can be discarded, run `hypha cache clean --all`
2. Re-fetch the spore
3. Report to publisher if issue persists

### "Local files modified" during grow

Grow detected local changes and refused to overwrite. The error hint shows:
- **Old version** cache path (from spawned_from)
- **New version** cache path (run `hypha taste <uri>` first if not cached)

Compare the two with `diff -r <old>/content/ <new>/content/` and apply changes manually.
