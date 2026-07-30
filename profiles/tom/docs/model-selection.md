# Model and payload selection

Personal adapter only. Normative routing semantics live in
`north:orchestration/doctrine.md` and `north:orchestration/docs/routing.md`;
provider/account allocation lives in `north:docs/provider-architecture.md`. If
this file disagrees with either, those sources win. Shared policy never
chooses a concrete provider model, account, SDK, or subscription pool:
Orchestration describes the work; North resolves an executable runtime and
records both the request and the result.

## The envelope

The managed MCP envelope is the eight Orchestration fields plus North-owned
execution controls and the prompt:

```json
{
  "prompt": "implement the bounded change",
  "provider": "auto",
  "role": "implementer",
  "taskGrade": "mid",
  "domainRequirements": [],
  "topology": "worker",
  "tier": "standard",
  "reasoning": "medium",
  "posture": "deliver",
  "composition": {"kind": "preset", "id": "implementer", "overrides": []}
}
```

Direct `mcp__north__spawn` callers send this complete object. `north spawn
<template-id> "<prompt>"` hydrates a known template mechanically; `north
templates` renders the stock catalog and its resolved routing defaults.
Delegation is dependency-shape classified rather than a director alias:

```sh
north delegate "<task>" --role <worker-role> [spawn options] # atomic
north delegate "<task>" --composite [spawn options]          # composite
```

The `/delegate` chat adapter makes that classification while preserving one
user-facing verb. Context carriage is orthogonal: `--context <file>` on the
shell form; the chat adapter carries a concise session brief by default and
`--new` omits it. Managed North paths fail closed rather than inventing a
mode or missing axes.

## Runtime allocation

Use `provider:"auto"` unless the user or task explicitly pins a provider or
account (account pins are exceptional and never inferred from the current
session). North filters for authentication and enforceable capabilities,
reads subscription-usage signals, applies the configured allocation policy,
and resolves tier+reasoning via the provider catalogs. It may substitute
provider/account/model only before side effects and only while preserving
tier, reasoning, and authority; any degradation is explicit and recorded.
Never route through API keys or API-credit balances.

Verification attachment (worker evidence, verifier siblings, whole-outcome
attestation, the coordinator's spot-check budget) is doctrine Law 7 — not
restated here. Personal domain/posture defaults:
`~/.agents/docs/praxis/README.md`.
