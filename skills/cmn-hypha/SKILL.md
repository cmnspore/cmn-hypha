---
name: cmn-hypha
description: "Use the hypha CLI to discover, evaluate, spawn, develop, and release CMN spores."
allowed-tools: Bash, Read, Edit, Write, Glob, Grep
---

# Hypha

Use `hypha` for Code Mycelial Network (CMN) work. The CLI emits Agent-First Data JSON by default. In the default `--output-to split` mode, final results go to stdout while errors, progress, and logs go to stderr.

## Core Commands

- `hypha --help` returns the root `cli-help-v2` model, including every legal root shape and directly callable subcommand help path.
- `hypha <command> --help` returns every legal shape of that command in one round trip; use `hypha --docs` for the whole generated registry reference.
- `hypha sense <URI>` inspects a CMN domain or spore without downloading code.
- `hypha search <QUERY> --synapse synapse.cmn.dev` searches for spores.
- `hypha taste <URI>` downloads a spore for review; rerun with `--verdict safe|toxic|...` to record a verdict.
- `hypha spawn <URI> [DIR] --vcs git --bond` creates a working copy and fetches referenced spores.
- `hypha grow --synapse synapse.cmn.dev` updates a spawned working copy.
- `hypha hatch ...` creates or updates `spore.core.json`.
- `hypha release --domain <domain>` signs and publishes the current spore.
- `hypha mycelium root <domain>` initializes a publishing site.

## Conventions

- Treat `spore.core.json` as the source of truth for identity, synopsis, intent, license, bonds, and tree hashing.
- Review downloaded code before recording positive taste verdicts; `.git` and `.cmn` are protected receive-time control paths, not a complete sandbox, so inspect build scripts, package manager configs, editor configs, shell hooks, CI files, and language-specific runners.
- Read `hint` on every failed AFDATA response before retrying.
- Treat exit 2 as a structural CLI rejection that never ran the action; treat exit 1 as a domain failure and branch on `error.code`.
- Use `--output json` for automation unless the user asks for YAML or plain output.
- Keep the default `--output-to split` when stdout is captured as result data. Use `--output-to stdout` only for a consumer that reads one ordered event stream and branches on `kind`.
- Prefer `SYNAPSE_TOKEN_SECRET` over putting a Synapse token in argv. If an existing workflow must use `--synapse-token-secret`, do not record or repeat the raw command line.
- Use `--stdout-file` or `--stderr-file` when the consumer needs AFDATA-managed stream redirection.
