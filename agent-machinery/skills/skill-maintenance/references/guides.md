# Harness authoring guides

These are outside guides for writing bootstrap instructions, skills and hooks.
Read them when designing or auditing the harness, not for routine skill edits.

## Refresh every 90 days

A guide older than 90 days is stale. Before any harness redesign or audit, run
`bun run check:guides` from this package's root. If it reports stale guides:

1. Run `bun run refresh:guides`.
2. Re-check each link-only source below.
3. Read the diff, then update the numbers on this page and any bootstrap
   rule, skill or hook the new guidance contradicts.
4. Land the result as a single commit.

## Vendored copies

Openly licensed sources are copied verbatim into `guides/`, pinned to an
upstream revision. `guides/sources.json` records each source URL, license and
retrieval date. Don't edit these copies; refresh them instead.

| File | What it covers |
|---|---|
| [agentskills-specification.md](guides/agentskills-specification.md) | The Agent Skills format: frontmatter limits and progressive disclosure |
| [agentskills-best-practices.md](guides/agentskills-best-practices.md) | Scoping a skill as a coherent unit, moderate detail, procedures over declarations, gotchas |
| [agentskills-optimizing-descriptions.md](guides/agentskills-optimizing-descriptions.md) | Writing a trigger description and testing it against queries that should and shouldn't fire it |
| [agentskills-evaluating-skills.md](guides/agentskills-evaluating-skills.md) | Testing a skill against real tasks |
| [anthropic-skill-creator.md](guides/anthropic-skill-creator.md) | Anthropic's skill-creation workflow |
| [openai-codex-prompting-guide.md](guides/openai-codex-prompting-guide.md) | Prompting Codex models, plus how Codex loads `AGENTS.md` |

## Link-only sources

These sources carry no reuse license, so only their links and key points are
kept here.
- [Claude skill authoring best practices](https://platform.claude.com/docs/en/agents-and-tools/agent-skills/best-practices):
  keep the `SKILL.md` body under 500 lines; link reference files directly
  from `SKILL.md`, one level deep; give reference files over 100 lines a
  table of contents; write descriptions in the third person, saying what the
  skill does and when to use it; build evaluations before writing extensive
  documentation.
- [OpenAI: Using GPT-6](https://developers.openai.com/api/docs/guides/prompt-guidance.md):
  Astra is "more sensitive to instructions contained in skills and other
  files, such as `AGENTS.md`", and OpenAI recommends auditing them. User
  instructions outrank a skill's. A request phrased as "can you…" or "I want
  to…" means do the work.
- [Codex: custom instructions with AGENTS.md](https://developers.openai.com/codex/guides/agents-md):
  global, then repo root, then deeper directories, at most one file per
  directory; the combined size is capped at 32 KiB by default
  (`project_doc_max_bytes`).
- [Codex skills](https://developers.openai.com/codex/skills): how Codex finds
  and lists skills.
- [Claude Code memory](https://code.claude.com/docs/en/memory) and
  [hooks](https://code.claude.com/docs/en/hooks): how `CLAUDE.md` loads, and
  which hook events can add context.
- [HumanLayer: Writing a good CLAUDE.md](https://www.humanlayer.dev/blog/writing-a-good-claude-md):
  frontier models follow roughly 150–200 instructions with reasonable
  consistency, and the system prompt already uses about 50; keep the file
  under 300 lines, with under 60 better; enforce style with linters and hooks
  rather than prose.

## Size targets (last checked 2026-10-07)

| Item | Target |
|---|---|
| Always-loaded bootstrap | Under 300 lines, about 100–150 rules at most |
| Combined Codex `AGENTS.md` | 32 KiB cap |
| Skill description | At most 1,024 characters; a few sentences |
| `SKILL.md` body | Under 500 lines and about 5,000 tokens |
| Reference depth | One level from `SKILL.md` |

## What the guides agree on

- Add only what the model doesn't already know, and cut everything else.
- Give one default rather than a menu of options.
- Concrete examples and templates steer behaviour better than abstract rules.
- Size a skill like a function, as one coherent unit. Too narrow, and several
  skills load at once and conflict. Too broad, and it fires on the wrong tasks.
- Say exactly when to read each reference file.
- Codex: don't ask it for upfront plans, preambles or running status updates,
  because these make it stop early.
