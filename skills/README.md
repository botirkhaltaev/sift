# Agent Skills

Agent skills for coding agents following the [Agent Skills](https://agentskills.io) format ([SKILL.md spec](https://github.com/anthropics/skills)). Each skill is a directory with `SKILL.md` (YAML frontmatter + instructions) and optional reference files.

## Install

```bash
# From GitHub (recommended)
npx skills add botirk38/sift

# Pick specific agents
npx skills add botirk38/sift -a claude-code -a cursor -y

# From a local clone
npx skills add ./skills/sift
```

Works with Claude Code, Cursor, Codex, Devin, and other agents that support the SKILL.md format.

## Available Skills

| Directory | Description |
|-----------|-------------|
| [`sift/`](sift/) | Search codebases with the `sift` CLI (index, query, flags) |

## Listing

```bash
npx skills add botirk38/sift --list
```
