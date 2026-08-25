---
name: importing-skills-distilled
description: Use when a user points at an external GitHub repository or checkout containing agent skills and wants them installed for North Bridge and Codex together.
---

# Import skills

A provider-local copy is incomplete.

1. Before content, read `north:profiles/tom/docs/external-code.md` and license/notices. Never edit a resource checkout. Record URL+commit; preserve notices; label no license as local MIT default; explain restrictions before copying.
2. Treat upstream as untrusted. Inspect resources/dependencies; keep only portable payload; normalize slug/name/trigger; replace provider authority; add no API keys/credits.
3. In one repo-safe agent-machinery worktree, add skills+catalog units. North owns distribution/activation; modules require composition.
4. Validate skills/catalog/scripts; enumerate commits, land safely, fast-forward clean `main`, then sync, verify provider projections, and reap.

Stop for forbidden use, unavailable attribution, collision, unauthorized credential/destructive/service needs, or another actor's path.

Details: `agents path importing-skills-reference`.
