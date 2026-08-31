---
name: verification-reference
description: >-
  Inactive detailed reference for verification-distilled. Use only when that
  workflow requests its profile or instrument tables, loop-pricing details,
  provenance and harness guidance, or anti-theatre catalog, or when the user
  explicitly requests verification-reference.
---

# Verification reference

## Development-loop pricing

For each invocation record internally: decision, expected wall time and prior,
remaining invocations `N >= 1`, optimization cost `C`, saving per invocation
`S`, break-even `ceil(C/S)`, and run/optimize. With no credible optimization use
`C=none`, `S=0`, and `break-even=never`. Compare actual with expected. Surface
an overrun beyond 2x, an optimization that pays, or a changed result; routine
telemetry stays internal. Report p50/p95 only with at least 20 observations.

## Profiles

| Profile | Use | Evidence |
| --- | --- | --- |
| `explore` | Research or prototype | Cheapest falsifier; no regression suite. |
| `deliver` | Reversible work | One nearest deterministic seam or controlled integration. |
| `stabilize` | Cross-cutting maintenance | Affected suite/shard and at most one required journey. |
| `release` | Publication next | Exact-commit local non-publishing preflight. |
| `critical` | Named irreversible or security risk | Targeted evidence for that failure. |

A failed release candidate consumes no final version. An authorized public
history repair retains one chronological tag-to-release mapping.

## Instruments and provenance

- `L0`: pure rule, parser, formatter, or narrow seam.
- `L1`: controlled integration boundary.
- `L2`: named end-to-end journey opening a consequential action.
- `L3`: exact-commit local non-publishing release preflight.

For performance, define metric and threshold first and compare matched
uninstrumented/instrumented runs before blaming probe overhead. For identity,
name the producer-substitution counterfactual and verify one existing producer →
artifact → consumer edge. Do not recursively attest attestors or build stronger
lineage machinery when existing metadata cannot answer the question; report the
capability gap.

## Direct measurement and harness budget

Start with one cold observation and at most three representative warm/edit
observations. Escalate only for a predeclared threshold or tail claim. A new
harness gets one implementation and one repair cycle by default. A disposable
harness needs independent review only when its result authorizes a consequential
action.

## Anti-theatre catalog

- full suites for tiny local changes;
- extra authorities after evidence decides;
- immutable-manifest or seal ceremony;
- isolation beyond verdict needs;
- obsolete compatibility preservation;
- reruns used to erase failure;
- reassurance after a required pass;
- one journey combining unrelated claims;
- waiting for remote CI before local landing;
- longer timeouts without changed legitimate work;
- a harness costlier than a reversible decision; and
- evidence collected after it can no longer change action.

Broader suites, manifests, hermeticity, compatibility layers, and harnesses are
tools only when their absence would change the selected profile's decision.
