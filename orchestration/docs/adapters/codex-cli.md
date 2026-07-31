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
| `filesystem.write` + `shell` | `-s workspace-write -C <workdir>` | writes confined to the lane's workdir (worktree or clone); `--add-dir` for named extra surfaces only |
| read-only set (`shell.readonly`, no write) | `-s read-only` | codex's read-only sandbox is an OS-enforced write denial, so `shell.readonly` roles KEEP live shell probes on this adapter (unlike the Claude plugin adapter, which must withhold Bash) |
| `coordination` (orchestrator topology) | not dispatchable | this adapter has no per-child admission/settlement surface; orchestrator seats run on the session or North side and fan WORKER lanes out here |

Always pass `-s` explicitly — the user-level codex config is permissive
(filesystem unrestricted) and must never be relied on as a boundary.

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

## Verification

Worker lanes return evidence against their brief's done-bars in the `-o`
capture. Law 7 verifier siblings are separate composed lanes (typically
senior/`gpt-5.6-sol` or a native verifier), never the same session. The
orchestrator reconciles; a bare exit 0 is not evidence.

## Known failure modes

- `Selected model is at capacity` — transient; retry with backoff, cap
  concurrent lanes per account.
- Silent config inheritance — always pin `-m`, `model_reasoning_effort`, `-s`.
- Raw-prompt dispatch (skipping the composer) reproduces exactly the
  role-drift this adapter exists to prevent; treat it as a defect, not a
  shortcut.
