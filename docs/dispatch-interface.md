# The North dispatch interface

**Status:** normative. **Version:** `north-dispatch-interface:v1`.

This is the contract between *what North decides* and *what actually runs a
lane*. It exists so that replacing the execution backend — native subagents
today, a North-owned harness later — is a **backend swap, not a rewrite**.

Everything North is good at lives above this line. Everything that has failed
lives below it. Keep the line sharp and the failures stay swappable.

---

## Why this document exists

North's dispatch layer was built *underneath* harnesses it does not own. It
inherited their sandbox, their process model, and their failure modes, and owned
none of the primitives. Over 2026-07-22..25 that produced 112 recorded deaths in
765 runs — 94 of them a single transport — and three defect classes that exist
**only** because the wrapper had to re-create something the host already had:

| Defect | Existed because |
|---|---|
| No parent wait primitive | native fan-out was disabled, so waiting was reimplemented |
| Lanes cannot bind sockets | inherited `networkAccess: false` |
| Lanes cannot commit in their tree | inherited sandbox `.git` protection |

None of these are failures of *the idea* of a dispatch layer. They are failures
of that layer's *position*. The interface below is the thing that survives the
move.

---

## The contract

```
thread ──▶ composition ──▶ capability envelope ──▶ run + evidence
```

Four stages. North owns all four **as data**. A backend owns only the execution
of stage 3, and reports stage 4 back.

### 1 · Thread (the intent)

The durable unit of work. Already fact-native; do not change it.

| Field | Meaning |
|---|---|
| `id` | stable thread identity |
| `title` | what the work is |
| `done_when` | zero or more bars: probe + expected result |
| `part_of` / `depends_on` | graph position |
| `committed` | intent is accepted, not merely captured |

**Invariant.** A thread is backend-agnostic. Nothing about *how* a lane executes
may ever be recorded on a thread.

### 2 · Composition (the staffing decision)

Who should do this, expressed on independent axes. Resolved from the
orchestration catalog; never inferred from the backend.

| Axis | Values |
|---|---|
| `role` | template id, or a bespoke id with a contract |
| `taskGrade` | novice · junior · mid · senior · staff · principal · distinguished |
| `topology` | `worker` · `orchestrator` |
| `tier` | economy · standard · senior · frontier |
| `reasoning` | low · medium · high · xhigh |
| `posture` | explore · deliver · evaluate · preserve |
| `domainRequirements` | expertise the brief must load |

**Invariant.** Composition is provider-neutral. `tier` is a *semantic* level;
resolving it to a concrete model is the backend's job, via the catalog. A
composition that names a vendor model is malformed.

### 3 · Capability envelope (the authority boundary)

What the lane may do — the **only** stage a backend is permitted to constrain,
and the stage where every inherited failure showed up.

| Capability | Grants |
|---|---|
| `filesystem.read` / `filesystem.search` / `filesystem.write` | file access |
| `shell` / `shell.readonly` | command execution |
| `web` | network fetch |
| `coordination` | may create and settle children |
| `graph-authoring.fram` | sealed mutating graph authority |

Plus an execution envelope: working tree, isolation mode, and lifetime.

**Invariants, and these are the load-bearing ones:**

- A backend **must** enforce the envelope exactly, or **refuse the dispatch**.
  Silent narrowing is the defect that produced the socket and `.git` failures —
  a lane believed it had authority it did not have, and discovered this only by
  failing at the task.
- A backend that cannot enforce a capability **must** fail closed at admission,
  with a typed reason, **before** any side effect.
- Refusal is a first-class, reportable outcome. It is not a death.

### 4 · Run + evidence (what happened)

The record. This is the layer that must never be lossy — the postmortem's single
highest-leverage finding was that this write failed on 170 of 765 runs, which is
why "it died" was unanswerable for three days.

| Field | Meaning |
|---|---|
| `run_id`, `thread`, `agent` | identity |
| `process_outcome` | ran · died · stalled · blocked_preflight · ran_empty |
| `delivery_outcome` | reported · verified · unverified · blocked |
| `delivery_reason` | typed cause, **required** when not `reported` |
| `bar_evidence` | per-bar probe + observed result |
| routing receipt | the composition actually applied |

**Invariants:**

- **A run record is written for every dispatch, including refusals and deaths.**
  A death with no record is worse than a death.
- `delivery_reason` must name a *cause*, never a *category*.
  `openai_provider_execution_failed` with no attached cause is a category and is
  not acceptable.
- Evidence attaches where the claim lives. A bare "done" is not an outcome.

---

## What a backend must implement

A backend is anything that can execute stage 3 and report stage 4.

```
launch(composition, envelope, prompt) ──▶ run record
```

Required:

1. **Admit or refuse**, typed, before side effects.
2. **Enforce the envelope** exactly, or refuse.
3. **Report a run record**, always, including on failure.
4. **Support waiting** — a parent must be able to await children without
   spinning turns and without being classified as failed.
5. **Attach causes** — every failure carries its underlying error.

Explicitly *not* required: a specific process model, sandbox, or worktree
strategy. Those are backend-internal and must never leak upward into a thread or
a composition.

## Backends

| Backend | Status |
|---|---|
| Native subagents | **target for now.** Satisfies (4) and (5) natively; (1)–(3) wrap around it. |
| `codex-app-server` | **frozen.** 94/112 deaths; violates (5) — throws without cause. Reachable only by explicit pin. |
| Anthropic agent SDK | supported. |
| North-owned harness | future. The point of this interface is that arriving here changes nothing above stage 3. |

---

## The test that this interface is real

> Can the execution backend be replaced without editing a thread, a composition,
> or a bar?

If yes, the interface holds. If no, something below the line has leaked above
it — and that leak is what turns "swap the backend" back into "rewrite North."

Every failure catalogued in the 72-hour postmortem was a leak of exactly this
kind.
