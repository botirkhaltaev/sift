# Agent Skills

Installable agent skills following the [Agent Skills](https://agentskills.io) format. Each skill is a directory with `SKILL.md` (YAML frontmatter + instructions).

## Install

```bash
# From a clone of this repo
npx skills add ./skills/sift-cli

# Pick specific agents
npx skills add ./skills/sift-cli -a cursor -a claude-code -y

# From GitHub
npx skills add https://github.com/botirk38/sift/tree/master/skills/sift-cli
```

## Available Skills

| Directory | Description |
|-----------|-------------|
| [`sift-cli/`](sift-cli/) | Work on the `sift` CLI — flags, tests, integration |

## Listing

```bash
npx skills add ./skills --list
```
