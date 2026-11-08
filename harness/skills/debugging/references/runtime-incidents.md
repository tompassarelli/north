# Agent runtime incidents

Read for a named unresolved detail of seeding, diagnosis or closure.

## Evidence and signature

Delivery and reliability are independent lifecycles. Before a retry or replacement changes the evidence, preserve intended operation, emitted recipient/tool and argument shape, exact error or terminal signal, runtime/provider mode, actor/run identity, time, correction history and the affected seam. Store bounded evidence or exact references.

Each incident record holds `signature`, `generation`, `state`, `lifecycle_class`, `cause_class` and stable seam scope; intended and emitted operations, normalized argument shape, runtime mode, error class, first/last observation, count and preservation evidence; containment, delivery state, hypotheses, one reproduction and upstream owner; repair, regression, activation, primary-canary and restoration-debt evidence.

Derive the signature from `(lifecycle class, stable actor or sender role, intended operation or report boundary, emitted recipient/tool or payload surface, normalized argument or payload signature, runtime/provider mode, error class, stable affected seam scope)`. Exclude timestamps, run-specific and opaque IDs, and secret or volatile values, because a replacement run may reproduce the same cause and its ID would fragment one incident. Keep the seam scope, because dropping it would merge unrelated failures. Split a multi-seam observation deterministically, one record per scope. A complete tuple match updates last observation, count and evidence; a changed tuple is a new signature until evidence proves otherwise. For reporting or control payloads keep a normalized envelope/error signature, never the opaque payload.

## Containment and diagnosis

Quarantine only the defective call, run or seam and keep independent work moving. The runtime may admit a replacement or fallback only with typed proof that no provider side effect became observable, because that proof belongs to the run host, not an incident label; without it, do not retry or switch runtime and record the blocked delivery state. Preserve requested and resolved route evidence and any restoration debt.

Write each hypothesis with its predicted observation and what would refute it. Price and run one smallest safe, authorized, bounded reproduction with the expected signal declared in advance.

Lifecycle seam: `admission`, `startup`, `death`, `liveness`, `control` or `reporting`. Cause: `invocation`, `worker`, `provider-runtime`, `policy`, `external` or `unknown`.

## Reopening

`reproduction-blocked` and `reopened` stay open. A recurrence of a closed signature increments `generation` and enters `reopened`; prior evidence stays as history tagged with its generation and cannot satisfy closure. From `reopened`, make a fresh pass through every state before closing.

## Upstream reports

For a reproduced defect in an upstream agent runtime repaired in a downstream fork, prepare a sanitized report with the minimal reproduction, affected version, expected and actual behavior and the downstream fix outline. Publish it only when external reporting is within the authorized task, because publishing speaks for the operator; never include credentials, private transcript content or a pull request. Record its URL, the downstream repair commit and the upstream release or commit that retires the fork patch; record a pending report separately and never make it a prerequisite to delivery.
