---
name: importing-skills-reference
description: Detailed inventory, adaptation, registration, and activation procedure for third-party skill imports. Use only when importing-skills-distilled calls for procedural detail.
---

# Importing skills reference

## Source inventory

For a GitHub URL, clone into `~/code/resources/<repo>` or a temporary directory. Inventory every directory containing `SKILL.md`, then inspect its referenced scripts, references, assets, hooks, agent templates, and tool dependencies. License filenames to inspect include `LICENSE*`, `COPYING*`, and `NOTICE*`.

## Payload adaptation

- Omit root installation docs, `.git`, plugin manifests, and provider-specific authority unless the adapted skill actually needs them.
- Use lowercase letters, digits, and hyphens for the directory slug and make it equal frontmatter `name`.
- Add optional `category`, `hooks`, or `agents` only when deliberately used.
- Retain only resources referenced by the adapted skill.
- Record attribution and license material in `north:THIRD_PARTY_NOTICES.md`, including required `NOTICE` content.

## Ownership and discovery

Store each tracked skill at `agent-machinery:skills/<slug>/` and add one `skill` unit per slug to `agent-machinery:catalog.json`, with its source naming the tracked `SKILL.md`.

```text
agent-machinery package -> North catalog composition -> immutable shared generation -> provider adapters
```

## Checks and activation

Run skill-creator `quick_validate.py` for each imported directory, `north:cli/tests/agent-catalog-test.clj`, and the nearest checks for imported scripts. After the package and operator revisions land and North consumes them, run:

```text
north config agents sync
```

Confirm each slug in `north config agents skills`, the generated shared skill projection, and every configured provider adapter.
