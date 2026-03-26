# Error Codes

Reference for error codes and messages in CMN tools.

## Hypha Errors

### Pipeline Error Codes

These codes correspond to stages in the visitor resolution pipeline (sense/taste/spawn/grow/absorb). Each error includes a `trace` field in the final JSON event showing how far the operation progressed before failure.

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
| `absorb_error` | Absorb local operation failed | Check permissions, disk space |
| `synapse_error` | Synapse query failed | Check Synapse URL and network |
| `NOT_TASTED` | Spore has not been tasted | Run `hypha taste <uri>`, review, record verdict |
| `TOXIC` | Spore tasted as toxic | Do not proceed; choose an alternative |

### Publisher Error Codes

| Code | Cause | Resolution |
|------|-------|------------|
| `init_error` | Failed to initialize site | Check permissions |
| `NO_SITE` | Site directory not found | Run `hypha mycelium root --domain domain` |
| `NO_SPORE` | spore.core.json not found | Run `hypha hatch` first |
| `validation_error` | spore.core.json validation failed | Fix required fields |
| `sign_error` | Failed to sign manifest | Check private key |
| `invalid_args` | Invalid command arguments | Check required flags |
| `write_error` | Failed to write file | Check permissions |

### Runtime Diagnostic Codes

Non-fatal diagnostic events emitted during operations. These use the same Agent-First Data protocol structure and are written to the runtime stdout event stream.

| Code | Context | Meaning |
|------|---------|---------|
| `CACHE_WARN` | capsule/mycelium cache | Cache write failed (operation continues) |
| `DOWNLOAD_FAILED` | taste/absorb | HTTPS download failed, trying next dist source |
| `CLONE_FAILED` | taste/absorb | Git clone failed, trying next dist source |
| `TASTE_DEP` | taste --with-deps | Fetching a dependency |
| `SAVE_WARN` | spawn/grow | Failed to save .cmn/spawned-from/spore.json |
| `ABSORB_DISCOVER` | absorb --discover | Number of sources discovered from lineage |
| `ABSORB_FETCH` | absorb | Fetching a source for absorption |
| `SIG_VERIFIED` | absorb | Signature verification succeeded |
| `HTTP_OK` | mycelium serve | Successful HTTP request |
| `HTTP_ERROR` | mycelium serve | HTTP 500 response |
| `HTTP_NOT_FOUND` | mycelium serve | HTTP 404 response |

## Synapse Errors

### API Errors

| HTTP | Code | Message | Cause |
|------|------|---------|-------|
| 400 | `INVALID_PROTOCOL` | Invalid protocol version | Protocol not `cmn/1` |
| 400 | `INVALID_REQUEST` | Malformed request body | Fix request JSON |
| 401 | `SIGNATURE_INVALID` | Signature verification failed | Invalid signature |
| 404 | `NOT_FOUND` | Resource not found | Spore/domain not indexed |
| 422 | `INVALID_URI` | Invalid URI format | Fix URI syntax |
| 500 | `INTERNAL_ERROR` | Server error | Report to operator |

### Pulse-Specific Errors

| Code | Message | Cause |
|------|---------|-------|
| `VERSION_TOO_OLD` | Timestamp older than cached | Mycelium already has newer version |
| `VERSION_CONFLICT` | Same timestamp, different hash | Conflicting updates detected |
| `TIMESTAMP_TOO_FAR_IN_FUTURE` | Timestamp too far ahead | Clock skew exceeds `pulse.max_clock_skew` (default 300s), check system time |

### Storage Errors

| Code | Message | Cause |
|------|---------|-------|
| `DB_CONNECTION_FAILED` | Database unreachable | Check PostgreSQL connection |
| `DB_QUERY_FAILED` | Query execution error | Database issue |
| `STORAGE_FULL` | Storage limit reached | Increase storage capacity |

## Error Response Format

### Hypha CLI — Error

The `code` field contains the specific error code. Parse `code` to branch on error type:

```json
{
  "code": "SIG_FAILED",
  "error": "Core signature verification failed: invalid signature",
  "trace": {
    "duration_ms": 0
  }
}
```

```json
{
  "code": "invalid_uri",
  "error": "URI must start with 'cmn://'",
  "trace": {
    "duration_ms": 0
  }
}
```

### Hypha CLI — Success with Trace

`sense` returns a `trace` field with resolution metadata (key fetch, caching, verification):

```json
{
  "code": "ok",
  "result": { "spore": { "..." : "..." } },
  "trace": {
    "uri": "cmn://cmn.dev/b3.3yMR7vZQ9hL2xKJdFtN8wPcB6sY1mXgU4eH5pTa2",
    "cmn": { "resolved": true, "cached": true, "public_key": "ed25519.5XmkQ9vZP8nL3xJdFtR7wNcA6sY2bKgU1eH9pXb4" },
    "verified": { "core_signature": true, "capsule_signature": true }
  }
}
```

### Synapse API — Error

```json
{
  "code": "error",
  "error": "Signature verification failed for domain cmn.dev",
  "trace": {
    "error_code": "SIGNATURE_INVALID",
    "storage": "redb"
  }
}
```

### Synapse API — Success

Success responses include a `result` field with query data and a `trace` field with internal state:

```json
{
  "code": "ok",
  "result": {
    "query": { "hash": "b3.3yMR7vZQ9hL2xKJdFtN8wPcB6sY1mXgU4eH5pTa2", "max_depth": 10 },
    "lineage": []
  },
  "trace": {
    "max_depth_reached": false,
    "storage": "redb"
  }
}
```

### Synapse Access Log

Every request produces a single access log line using Agent-First Data fields at the top level (`code`, request fields, optional `result`/`error`, and `trace`). The `trace` field merges request metadata with handler-level state.

JSON format (`log_format: json`):

```json
{
  "code": "request",
  "method": "GET",
  "path": "/synapse/spore/b3.3yMR7vZQ9hL2xKJdFtN8wPcB6sY1mXgU4eH5pTa2",
  "status_code": 404,
  "error": "Spore not found",
  "trace": {
    "duration_ms": 1,
    "error_code": "NOT_FOUND"
  }
}
```

Plain format (`log_format: plain`):

```text
code=request method=GET path=/synapse/spore/b3.3yMR7vZQ9hL2xKJdFtN8wPcB6sY1mXgU4eH5pTa2 status_code=404 error="Spore not found" trace.duration=1ms trace.error_code=NOT_FOUND
```

The plain format is generated from the same JSON value using `agent_first_data::output_plain` (logfmt-style key/value output).

## Exit Codes

Hypha CLI exit codes:

| Code | Meaning |
|------|---------|
| 0 | Success |
| 1 | General error |
| 2 | Invalid arguments |
| 3 | Network error |
| 4 | Verification failed |
| 5 | Git operation failed |
| 6 | Permission denied |

## Debugging

### Verbose Output

```bash
hypha --verbose taste cmn://cmn.dev/b3.3yMR7vZQ9hL2xKJdFtN8wPcB6sY1mXgU4eH5pTa2
```

### Debug Logging

```bash
RUST_LOG=debug hypha taste cmn://cmn.dev/b3.3yMR7vZQ9hL2xKJdFtN8wPcB6sY1mXgU4eH5pTa2
```

### Trace Logging

```bash
RUST_LOG=trace hypha taste cmn://cmn.dev/b3.3yMR7vZQ9hL2xKJdFtN8wPcB6sY1mXgU4eH5pTa2
```

## Common Issues

### "Key fetch failed"

1. Verify `https://yourdomain.com/.well-known/cmn.json` is accessible
2. Check that `cmn.json` contains a valid `public_key` field
3. Ensure HTTPS is properly configured on the domain

### "Signature verification failed"

1. Verify you're fetching the correct URI
2. Check if publisher updated their public key in cmn.json
3. Try fetching from Synapse mirror if available

### "Hash mismatch"

1. Clear cache: `hypha cache clean --domain cmn.dev`
2. Re-fetch the spore
3. Report to publisher if issue persists

### "Local files modified" during grow

Grow detected local changes and refused to overwrite. The error hint shows:
- **Old version** cache path (from spawned_from)
- **New version** cache path (run `hypha taste <uri>` first if not cached)

Compare the two with `diff -r <old>/content/ <new>/content/` and apply changes manually.
