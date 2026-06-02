# Agent Skills

Installable agent skills following the [Agent Skills](https://agentskills.io) format. Each skill is a directory with `SKILL.md` (YAML frontmatter + instructions).

## Install

```bash
# From a clone of this repo
npx skills add ./skills/sift

# Pick specific agents
npx skills add ./skills/sift -a cursor -a claude-code -y

# From GitHub
npx skills add https://github.com/botirk38/sift/tree/master/skills/sift
```

## Available Skills

| Directory | Description |
|-----------|-------------|
| [`sift/`](sift/) | Search codebases with the `sift` CLI (index, query, flags) |

## Listing

```bash
npx skills add ./skills --list
```
