# Delta Transfers

Hypha supports incremental archive updates via zstd dictionary compression. When a publisher provides a `delta_url` endpoint, `hypha grow` downloads only the difference between the old and new archive instead of the full archive.

## How It Works

Delta transfers use **zstd dictionary decompression**: the old archive serves as the dictionary, and the delta file is a zstd-compressed stream that references bytes from that dictionary.

```
Publisher side (hypha release):
  1. Compress new archive with old archive as zstd dictionary
  2. Publish delta at: delta_url template with {hash} and {old_hash}

Client side (hypha grow):
  1. Look up cached old archive from previous spawn/grow
  2. Fetch delta: GET delta_url.replace("{hash}", new).replace("{old_hash}", old)
  3. Decompress delta using old archive as zstd dictionary
  4. Result: full new archive (identical to downloading the full archive)
  5. Verify content hash matches spore URI (same as full download)
```

## Endpoint Template

The `delta_url` field on the `type: "archive"` endpoint uses two placeholders:

```json
{
  "endpoints": [
    {
      "type": "archive",
      "url": "https://example.com/cmn/archive/{hash}.tar.zst",
      "format": "tar+zstd",
      "delta_url": "https://example.com/cmn/archive/{hash}.from.{old_hash}.tar.zst"
    }
  ]
}
```

- `{hash}` — target (new) archive hash
- `{old_hash}` — base (old) archive hash
- Direction is always `old_hash → hash` (forward only)

## Fallback

Delta transfers are optional and best-effort:

1. If `delta_url` is not present in endpoints, skip delta
2. If the old archive is not in the local cache, skip delta
3. If the delta download fails (404, timeout, corruption), fall back to full archive
4. After successful delta application, the result is verified against the spore's content hash — identical to the full download path

## Implementation Notes for Other Clients

- **Compression library**: zstd with dictionary support (e.g., `zstd` crate in Rust, `zstandard` in Python, `fzstd` in JS)
- **Dictionary**: the raw decompressed tar bytes of the old archive (not the .tar.zst, the decompressed .tar)
- **Output**: raw tar bytes of the new archive, which are then zstd-compressed for local caching
- **Size budget**: clients should enforce a maximum decompressed size to prevent zip bombs (hypha uses `max_extract_bytes` from config, default 512 MB)
- **Cache reuse**: after applying a delta, cache the resulting archive for future delta chains (v1 → v2 → v3)
