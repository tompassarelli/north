---
name: program-stewardship-distilled
description: >-
  Default distilled workflow for choosing an engineering-quality posture,
  recording a consequential deferral, or routing a craftsmanship or hardening
  pass. Use when quality investment for a workstream needs a deliberate choice.
---

# Program stewardship

Start from the internally resolved `project-exposure-v1` profile. When no
profile or concrete exposure facts are supplied, silently use the validated
volatile owner-controlled research default; record no profile artifact,
sidecar, or form. Materialize a machine sidecar only at a boundary that needs
it. A lifecycle budget is a ceiling, never a checklist: an eligible mechanism
does not create work.

Unless concrete facts say otherwise, choose fast owner-controlled research:
the shortest useful artifact, an 80/20 stopping point, ordinary expected use,
and one cheap decision-changing check. Test the thesis quickly and reasonably,
not conclusively. Safety and exact bounded-claim correctness remain binding.

Classify quality axes independently:

- changeability earns effort only when it lowers current or clearly next-change
  cost; speculative abstractions are presumed harmful;
- claim correctness gets the cheapest check that can falsify the thesis;
- robustness gets observed failures and named expected inputs, not imagined
  aliens, hostile users, scale, or edge cases;
- security gets universal secret/destructive-operation boundaries, then only
  threat controls tied to an actual asset, entry point, trust boundary, and
  plausible impact;
- operations and assurance get a budget only from actual live state, external
  dependence, or an explicit requirement at that exact seam.

Escalation requires a named consumer or boundary, plausible failure mode,
material consequence, and the smallest mechanism that changes the immediate
decision. Missing any fact means stay at the research floor. Public source, a
CLI, a Store, a daemon, a process, durable local data, or future users do not by
themselves establish production. One escalated axis never raises another.

Preserve relevant evidence and existing security, durability, compatibility,
and data promises unless changed. Keep facts/effects clear; reject polish-only
frameworks and degraded public vocabulary/errors. Separate broad restructuring.
Record consequential deferrals only in the existing mechanism, with reason and
reopening event.

Route structure to `program-craftsmanship-distilled`, guarantees to
`production-hardening-distilled`; use `planning-distilled`/`verification-distilled` as applicable. Invent no
recurring review, sidecar, or global ledger without its own objective/authority.
Stop at the floor when more is speculative, out of scope, or poor value.

For the posture worksheet and deferral examples, run
`agents path program-stewardship-reference` and read its `SKILL.md` completely.
