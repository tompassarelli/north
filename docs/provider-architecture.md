# Provider accounts, routing, and usage truth

North owns durable coordination, account selection, and run evidence. Provider
surfaces execute a prepared run. Gaffer remains account-blind: it chooses the
role, composition, semantic tier, reasoning, and posture; North chooses an
eligible provider target and resolves that semantic tier through the selected
provider's model catalog.

## Subscription accounts

An account is a named routing target backed by provider-owned subscription login
state. The provider CLI owns the login flow, credential format, refresh, and
revocation. North invokes that flow inside an isolated CLI home; it never asks
for or copies credential values.

```sh
north account add claude-personal anthropic
north account login claude-personal

north account add codex-personal openai
north account login codex-personal

north account status
north account status claude-personal
north account list
```

`add` creates the isolated home and appends the target to
`~/.config/north/routing-policy.json`. Claude homes live under
`~/.local/state/north/accounts/anthropic/<id>`; Codex homes live under
`~/.local/state/north/accounts/openai/<id>` with a separate `sqlite/` child.
North links the safe shared instructions, skills, hooks, and configuration from
`~/.claude` or `~/.codex`, while provider login state remains isolated. `status`
checks the provider CLI in each target's own home; `list` shows the configured
provider, profile, and root.

Without named accounts, the existing `~/.claude` and `~/.codex` homes are the
ambient `anthropic` and `openai` targets. Named and ambient targets use the same
routing contract.

## Selection and pins

Provider selection is target-first. A target is one authenticated account; a
provider may therefore have multiple sibling targets.

```sh
north spawn implementer "Add the parser regression" --tier standard
north spawn integrator "Review the migration boundary" --provider anthropic
north spawn integrator "Review the migration boundary" --target claude-personal
```

- No pin, or `--provider auto`: select from every eligible target under the
  configured allocation mode.
- `--provider anthropic` or `--provider openai`: restrict selection and any
  fallback to sibling targets owned by that provider.
- `--target <id>`: select exactly that account. An exact target pin never falls
  back to a sibling account or another provider; unavailable or exhausted means
  the spawn fails before execution.

The policy is visible and editable through one surface:

```sh
north config routing
north config routing mode preferential
north config routing order claude-personal claude-work codex-personal

north config routing mode balanced
north config routing weight claude-personal 2
north config routing weight codex-personal 1

north config routing mode reserved
north config routing reserve claude-personal
```

The allocation modes are:

- **preferential** — choose the first eligible target in configured order.
- **balanced** — deterministically distribute stable run keys using target
  weights adjusted by each target's current subscription pressure.
- **reserved** — preserve the configured target for `frontier` work when an
  alternative can handle lower tiers; use the reserve for frontier work when it
  is eligible.

Pressure is per target, not merely per provider. The states are `plenty`,
`normal`, `low`, `exhausted`, and `unknown`. Automatic observations from each
provider's subscription-usage surface are primary. An `exhausted` target is
ineligible; balanced routing reduces the effective weight of `low` targets.
Temporary manual observations are available when the automatic view is missing
context:

```sh
north config routing pressure claude-work low
north config routing pressure codex-personal exhausted --until 2026-07-20T00:00:00Z
```

A manual observation expires after 24 hours unless `--until` is supplied.
Automatic observations are stored at
`~/.local/state/north/provider-usage-observations.json`. Resource envelopes are
separate admission limits on runs, frontier runs, retries, and parallelism; they
do not replace per-target subscription pressure.

## Proof-carrying fallback

Automatic fallback is allowed only before side effects. The provider adapter
must raise the typed `ProviderRetrySafeError`, proving that the request was not
accepted and no externally observable model or tool action occurred. North also
requires that no provider event has been emitted. It never infers replay safety
from exception text or an empty event stream.

The candidate set still obeys the caller's pin:

- auto selection may advance to another eligible account or provider;
- a provider pin may advance only to a sibling account of that provider;
- an exact target pin has no fallback candidates.

On a cross-provider fallback, North resolves the same semantic tier again through
the new provider's catalog before invocation. Once execution may have produced a
side effect, the failure is surfaced instead of replaying the task.

## Identity and routing evidence

The active route is visible in both live agent identity and immutable run facts.
Agent identity carries `provider`, `provider_target`, `model`, and `effort`; the
display label includes the target, such as `anthropic:claude-personal`. Run
telemetry records the requested and resolved route separately:

- `requested_provider`, `requested_target`, and `requested_tier` describe intent;
- `provider`, `provider_target`, `model`, and `effort` describe execution;
- `provider_reason`, `allocation_mode`, and `entitlement_pressure` explain the
  initial choice;
- `fallback_count`, `fallback_path`, and `fallback_target_path` preserve every
  proof-authorized route change.

This division keeps Gaffer reusable across account layouts. Gaffer's canonical
staffing catalog at `~/code/gaffer/staffing/catalog.json` names roles and semantic
tiers; it contains no personal account IDs or subscription state.

## Token truth

Token totals are provider-authoritative observations, not reconstructed
estimates. Every run records how many terminal usage records were observed, the
provider scope of that observation, and whether a total is exact:

- `usage_terminal_count`
- `usage_scope`
- `usage_total_status`

The aggregate `tokens` fact exists only when the provider adapter can prove an
exact total. With no terminal record, repeated terminals, incomplete terminal
components, or an unknown adapter scope, the total stays unknown rather than
becoming zero. Exact components such as `input_tokens` or `output_tokens` may
still be retained when an aggregate is unknown. Cached-input and reasoning-output
counters are subsets of their provider totals and are never added a second time.

Reports preserve that distinction: an all-unknown set displays `unknown`; a mix
of exact and unknown runs displays the exact known lower bound with incomplete
coverage; only fully covered sets display an exact total. Historical rows that
already contain an exact aggregate remain readable.

## Adapter boundary

Provider imports remain confined to `~/code/north/sdk/src/providers`. Anthropic
uses the Claude Agent SDK; OpenAI uses the authenticated Codex CLI and its ChatGPT
subscription. Both receive the target-specific environment and shared North
supervision. Live mid-run steering and model escalation remain capability-checked:
unsupported escalation fails visibly rather than pretending it succeeded.
