# Praxis — personal residue (canonical blocks live in orchestration)

The generic spawn-payload system — roles, grades, topologies, postures, model
deltas, and the compose/elicit procedures — is CANONICAL in Orchestration
(`north:orchestration/doctrine.md`, `north:orchestration/docs/routing.md`, and
the blocks under `north:orchestration/docs/`). Edit it there; it ships to
everyone, and north's harness reads it from there at spawn time. Nothing in
this directory restates it. This directory holds only what is personal:

## Domain bootstrap (defaults by entry point)

| Domain | Path signal | Default posture | Notes |
|---|---|---|---|
| Client delivery | `~/code/client/*` | deliver (preserve on existing code) | Deadline-real. Ladder hard: glue minimized. Confidential — no cross-references out. |
| Novel core / research | `~/code/beagle`, north core, new primitives | explore → deliver once shaped | Priors law ACTIVE: distrust fluent defaults, derive and verify. Core inversion: hand-build the deliverable. |
| Infrastructure / config | `~/code/nixos-config`, dotfiles, CI | deliver, preserve-leaning | Reproducibility rules; blast radius = every future rebuild. |
| Others' code | `~/code/reference/*` | read-only | Never edit; license check before leveraging. |

Domain sets defaults; task shape can override a default; orchestration's escape
hatch overrides everything.

## Fable self-report

`self-reports/fable.md` — historical model introspection (with §11 generic
payload and §12 compilation method + trial predictions). It is calibration
evidence, not routing policy; current provider catalogs and North availability
signals decide whether any concrete model is eligible.

## Verification override — OpenAI lanes

`verification-override-openai.md` — paste-able brief block binding the
verification doctrine
(`~/.agents/docs/verification-doctrine.md`) as
imperatives: terminal states per pass, claim contracts, tier fixed at intake,
probe budgets, stop rule. Attach to every OpenAI-provider lane doing
implementation or verification work. Demand provenance per the freeze rule:
repeated verification tarpits in unsupervised OpenAI lanes (2026-07, fram
cache work).

## Change policy — the freeze rule

Edit this structure only after a real spawn hit a misfit **twice**. Never
speculatively — no new roles, postures, domains, or deltas ahead of demand.
