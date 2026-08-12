# Ordinary-operation learning regime

North has an operational regime orthogonal to dispatch mode, provider account,
Orchestration role, and delivery posture:

- **frozen** uses the best-known admitted route, prompt, authoring surface, and
  history strategy consistently. It still records the same telemetry and
  construction receipts.
- **learning** permits bounded exploration during ordinary dispatch. A
  deterministic assignment may change at most one eligible axis for one
  episode. Risk, hard quality floors, explicit eligibility, and maximum tier
  distance narrow the candidate set before assignment.

This is experimental learning from operations, not provider-model training.
The default is `frozen`.

## Configure the regime

```sh
north config learning
north config learning mode frozen|learning
north config learning intensity 0.10
north config learning axes all
north config learning axes model-tier effort prompt
north config learning max-tier-delta 1
north config learning risk-ceiling p1
north config learning seed ordinary-ops
north config learning epoch 2
north config learning evidence-mode discovery|evaluation
```

The versioned policy is atomically replaced at
`~/.config/north/learning-policy.json`. `NORTH_LEARNING_POLICY` selects an
isolated policy for a test or tool. A malformed document fails closed; North
does not silently restore defaults over invalid state.

Changing `mode` never changes a pinned dispatch surface. The dispatch setting is
`native` (provider-native pinned), `north` (North-managed pinned), or `auto`
(choose for each dispatch). Under `auto`, frozen versus learning says whether
that assignment is deterministic known-best or bounded experimental. Account
allocation policy remains a routing detail within `auto`, never a peer dispatch
mode.

## Assignment contract

The harness decides and durably publishes the complete assignment before it
selects or invokes a provider. If publication fails, provider execution does
not begin. The deterministic key commits the policy fingerprint, policy seed
and epoch, and stable episode identity. This makes replay exact without making
assignment depend on provider availability or timing.

The control arm is the frozen policy that would otherwise run. An exploratory
episode changes one of these axes:

- `model-tier`
- `effort`
- `prompt`
- `authoring`
- `history`

Model tier and effort cannot cross the run's hard floor or the policy's maximum
tier delta. Authoring and history have no implicit alternatives: the caller
must supply explicit eligible arms, and an unsupported arm resolves to control.
A pinned route axis is not eligible. Unknown risk resolves to control.

Facts such as `learning_policy_sha256`, `learning_task_signature_sha256`,
`learning_axis`, `learning_arm_id`, propensities, narrowed options digest, and
the complete assignment digest are immutable on the run. The terminal writer
must repeat the exact pre-provider projection; omission or movement is refused.

## Discovery is not evaluation

`discovery` is for changing prompts, arm definitions, instrumentation, and
eligibility rules while learning what is worth testing. Its observations are
useful diagnostics but never comparison evidence.

`evaluation` freezes those experiment semantics. Even then, a run enters an
offline comparison cohort only when all of the following are exact:

- the immutable assignment is complete and valid;
- the task signature coverage is exact;
- the prompt construction receipt is exact;
- the execution-environment receipt is exact;
- the run envelope ties those receipts to the assignment and admitted route;
- at least one done bar exists and every done bar has observed evidence.

Unknown evidence is never converted to zero. `north learning compare
<experiment-id> [--json]` retains excluded attempts and names every exclusion
reason. It deduplicates explicit retry chains, then groups eligible observations
by exact task-signature digest, changed axis, and arm. Its observed summaries
are descriptive only; the command does not produce a causal verdict.

## Content-addressed construction evidence

The prompt receipt commits the ordered module list, schema and dependency
versions, source and rendered-byte digests, privacy-bounded safe parameters,
branch decisions, and the exact provider wire bytes. It stores no raw prompt.

The environment receipt separately commits:

- the skill catalog available to the run;
- the resource closure actually activated;
- tools, hooks, configs, executables, and instruction sources.

Availability is not activation. If the harness cannot prove the activated
closure, environment coverage is `unknown`, and the run remains outside
evaluation cohorts. The run envelope finally commits the prompt receipt,
environment receipt, learning assignment, admitted semantic route, provider
adapter version, and provider runtime version.

Receipts contain digests, identifiers, counts, and deliberately bounded safe
parameters. Secret-shaped parameters and raw credentials are rejected.
