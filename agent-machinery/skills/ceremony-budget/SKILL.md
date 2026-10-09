---
name: ceremony-budget
description: >-
  Decide how much hardening, security, signing, compatibility or test isolation a change needs before adding any, and harden one named guarantee when it does. Use when tempted to add process, safeguards or "while I'm here" fixes.
grounded: 2026-10-09
written: 2026-10-09
---

# Ceremony budget

1. Use the bootstrap profile's baseline: zero ceremony for prototype, daily operation for tooling, repository rules for client.
2. Escalate one mechanism only for a named current consumer, failure and failure cost.
3. Keep each investment axis independent.
4. State the named guarantee, allowed degradation and surviving state before hardening.
5. Repair its weakest resource, retry, cancellation or persistence boundary with bounded retries and idempotence where repeated effects cause harm.
6. Exercise the named failure without weakening existing gates.

Read [hardening](references/hardening.md) when choosing failure paths or [rationale](references/rationale.md) when revising this skill.
