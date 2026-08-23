# codex-cli adapter — direct OpenAI lane dispatch

The `codex-cli` transport declared in `providers/openai.json`, documented as a
dispatch surface. Managed OpenAI lanes through North are currently ineligible
(fail pre-turn; see `docs/adapters/north.md`), so this adapter is today's only
OpenAI execution path: the orchestrating session drives `codex exec` directly.
Authentication rides the North launcher wrapper (`codex` on PATH), which
resolves a subscription account — never an API key.

## Dispatch contract

Every lane launches with a COMPOSED payload, never a bare task prompt:

```sh
node scripts/compose-payload.mjs <role> --provider openai \
  [--model <exact-or-alias>] [--reasoning <level>] --task <brief-file> \
  > payload.md
codex exec -m <resolved-model> -c model_reasoning_effort='"<resolved>"' \
  -s <sandbox> --ephemeral -C <workdir> -o <result-file> - < payload.md
```

The composer resolves tier→model/reasoning through `providers/openai.json`
(exact-model pins pass `resolvePinnedModelRoute` — Law 4), stacks
role + task-grade + topology + posture + comms + the OpenAI family block
(`docs/deltas/openai-common.md`) + the exact model's calibrated delta, and
prints the suggested invocation to stderr. Pin `-m` and
`model_reasoning_effort` on every spawn; never inherit codex config defaults.

## Capability → sandbox mapping

| Stock-template capabilities | Sandbox | Notes |
|---|---|---|
| `filesystem.write` + `shell` | `-s workspace-write -C <workdir>` | writes confined to the lane's workdir — which MUST be a self-contained local clone (see below); `--add-dir` for named extra surfaces only |
| read-only set (`shell.readonly`, no write) | `-s read-only` | codex's read-only sandbox is an OS-enforced write denial, so `shell.readonly` roles KEEP live shell probes on this adapter (unlike the Anthropic agent-file adapter, which must withhold shell) |
| `coordination` (orchestrator topology) | not dispatchable | this adapter has no per-child admission/settlement surface; orchestrator seats run on the session or North side and fan WORKER lanes out here |

Always pass `-s` explicitly — the user-level codex config is permissive
(filesystem unrestricted) and must never be relied on as a boundary.

## Authoring-lane workspace topology (mandatory)

An authoring lane's workdir is a SELF-CONTAINED LOCAL CLONE on real disk:

```sh
git clone --local /path/to/<repo>/main /path/to/lanes/<lane-name>
```

Never either of these, both of which produce `blocked-by-execution` on the
first `git add` by construction:

- **A git worktree of a guarded main.** A worktree's `.git` is a pointer
  file; its index, HEAD, locks, and all committed objects live under the
  MAIN checkout's `.git` — outside the lane's writable root. The lane can
  edit every file and commit none of them.
- **Anything under `/tmp`.** The codex sandbox virtualizes `/tmp`; a clone
  there reads normally but its `.git` writes land on a read-only path.

`--local` hardlinks objects, so clones are cheap. The lane commits freely
in its own universe; landing is the orchestrator's act — fetch from the
clone, review, fast-forward, `safe-push`. Read-only lanes need none of
this: point `-C` anywhere readable.

## Lane shapes

- **Fire-and-forget worker**: `--ephemeral`, payload via stdin, result via
  `-o <file>`; run lanes as background jobs with bounded parallelism.
- **Persistent, steerable lane**: drop `--ephemeral`; the session is durable
  and resumable non-interactively: `codex exec resume <session-id> "<follow-up>"`.
  `resume` accepts no sandbox flags (the session's policy is inherited) and
  must run from a trusted (git) directory.
- **Structured output**: `--output-schema <schema.json>`. OpenAI structured
  outputs are STRICT: every key in `properties` must appear in `required` at
  every nesting level, or the request 400s before the lane starts.

## Delivery checks

Worker lanes return the nearest existing relevant check and its observation in
the `-o` capture. The orchestrator reconciles child results and may run one
existing integrated aggregate check. It does not originate a verifier lane;
that role requires the user's current request to explicitly ask for assurance.

## Environment hygiene (from the hermes-agent study, 2026-07-31)

Findings adopted from the MIT-licensed hermes-agent codebase (study
archived in north-data with file:line citations):

- **Ambient codex config is load-bearing.** Even surfaces that compute a
  permission profile may not transmit it (observed in hermes's app-server
  transport); effective policy comes from `CODEX_HOME` config unless
  overridden per invocation. Therefore: every flag that matters is pinned
  on the command line, every time — never rely on ambient config.
- **Sanitize the child environment.** Lanes inherit the launcher's
  environment; infrastructure, daemon, and side-channel credentials do not
  belong in a coding lane. Prefer allowlist inheritance and a per-lane
  `CODEX_HOME` when lane trust matters.
- **The full-access pattern hermes recommends under sandbox friction
  (`danger-full-access` + convention-based review + deleting uncommitted
  work) is REJECTED on this host** — it assumes a disposable environment.
  This machine carries live daemons, canonical repos, and billing state.
  Full access becomes acceptable only inside an ephemeral container/VM
  with no host home, daemon sockets, or canonical repo mounts — that
  disposable-lane environment is tracked as its own work item.

## Known failure modes

- `Selected model is at capacity` — transient; retry with backoff, cap
  concurrent lanes per account.
- Silent config inheritance — always pin `-m`, `model_reasoning_effort`, `-s`.
- Raw-prompt dispatch (skipping the composer) reproduces exactly the
  role-drift this adapter exists to prevent; treat it as a defect, not a
  shortcut.
