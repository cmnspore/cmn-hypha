---
name: cmn-hypha
description: "Use the hypha CLI to discover, evaluate, spawn, develop, and release CMN spores."
disable-model-invocation: true
allowed-tools: Bash, Read, Edit, Write, Glob, Grep
---

# Hypha

Use `hypha` for Code Mycelial Network (CMN) work. The CLI emits Agent-First Data JSON by default.

## Core Commands

- `hypha --help` prints first-level help; use `hypha <command> --help` for one command or `hypha --help --recursive` for the full recursive reference.
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
- Use `--output json` for automation unless the user asks for YAML or plain output.
