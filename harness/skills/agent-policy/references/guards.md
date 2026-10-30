# Guards: implement or change a command or write guard

Read this when you implement or change a refusing guard, its identity,
activity gate, provider wiring or deny/pass fixtures. The live list of guards
is in [enforcers.md](enforcers.md).

## Enforcement is a separate layer

A skill explains the sanctioned workflow; the guard enforces a deterministic
boundary even when that skill is not loaded. A provider wrapper transports the
real event and payload. None of them invents identity, permission, module
closure or source authority, because a second authority drifts from the first.

A refusing guard has one `PreToolUse` identity shared by its implementation,
catalog, provider wiring, worker chains and fixtures, so a fixture proves the
same unit that runs. Telemetry and lifecycle hooks keep their actual events,
because a wrapper that pretends to be `PreToolUse` sees the wrong payload.

## Contract

- Edit the resolved owner in a worktree, never a projection, because a
  projection is regenerated over your edit.
- Drain stdin, bound input near 1 MiB, and fail open on malformed or oversized
  input, missing optional runtime, unresolved paths or internal errors, because
  a broken guard must not stop every session. Never silently redefine a
  security enforcement contract that the owner declared fail-closed.
- Honor `AGENT_NO_AUTHORING_HOOKS`. Otherwise require one exact active
  `kind=hook` row in the valid generation; missing, duplicate, invalid or
  inactive state disables the guard, because North already resolved
  permission, claimants, module closure and provenance.
- Deny only a decoded dangerous operation and name a sanctioned alternative,
  because an agent can only retry from a legal next move. Guards allow or deny;
  they never ask the owner.
- A PreToolUse allow does not skip Claude Code's own dangerous-rm dialog;
  the `north-hook` tripwire guard answers it on `PermissionRequest:Bash`.


## Skill by file

`agent-machinery/file-skills.toml` maps edited file kinds to the skill that
governs them (`.rs`, `Cargo.toml`, `rust-toolchain.toml` to
`rust-development`; `.ts` where the nearest `package.json` or `node_modules`
has `effect` to `effect-development`; `.nix` to `nix-development`). The
`file_skill` check in `north-hook` advises the skill body on a session's first
matching `PreToolUse` edit and never again that session, and flags a Cargo
workspace root without `[profile.dev]` once with the compile-times reference's
dev-profile lines. It never refuses an edit, because the skill is context, not
a boundary. Add a language by adding a row; the embedded table is the only
place file kinds are mapped.

## Identity and activity inputs

Resolve the owner with `agents inspect <id>` and `agents path <id>`; do not
copy a historical hooks-directory path. A Firn-owned native policy points at
its NixOS source and installed executable; `firn-system-policy` shows that
shape, and a provider adapter is a distribution, not a second identity.

The current generation defaults to
`${NORTH_AGENT_STATE_ROOT:-~/.local/state/north/agents}/current/activation.active`,
one `kind id` line per active unit; `north_agent_unit_active` reads it with
bash builtins.

The session switch shape is:

```bash
case "${AGENT_NO_AUTHORING_HOOKS:-}" in
  ''|0|false) ;;
  *) exit 0 ;;
esac
```

## Deny envelope

Prefer stdout JSON with exit zero:

```json
{"hookSpecificOutput":{"hookEventName":"PreToolUse","permissionDecision":"deny","permissionDecisionReason":"<legal next action>"}}
```

Exit 2 with one stderr reason is the supported alternate.

## Decoded entrances

| Tool family | Payload to decode |
| --- | --- |
| `Edit`, `Write`, `MultiEdit` | `tool_input.file_path`, with `filePath` where the provider uses it |
| Bash | `tool_input.command`, including indirect writes through scripts, Git, redirects or text tools |
| Codex `apply_patch` | patch envelope plus relative-path resolution against `cwd` |

Wire only events and tool payloads the implementation actually decodes,
because wiring an undecoded entrance gives false coverage. Include
`apply_patch` only when the implementation deliberately parses that envelope;
the launch-critical worktree guard does, while `firn-system-policy`
deliberately covers the edit family and Bash instead.

## Provider wiring

Guard events per provider are declared in `harness/policy-owners.toml`
(`guard[]`); keep Claude and Codex at parity or record the `*_absent` reason,
because both providers are first-class.

Codex uses managed hooks from `/etc/codex/requirements.toml`. A promoted shell
hook command has this shape:

```text
/etc/codex/hooks/runtime/env -u BASH_ENV -u ENV /etc/codex/hooks/runtime/bash /etc/codex/hooks/<id>.sh
```

Declare promoted sources in `nixos-config:modules/codex/default.gal`, include
them in North's sealed enforcement-source contract, and regenerate through
`firn repo build`. Stable Firn native policy binds directly as
`/run/current-system/sw/bin/firn-system-policy`. Provider lifecycle wrappers
bind to their actual spawn, tool-use, delegated, stop or terminal events and
preserve stdin.

For North workers, add the exact catalog ID and source to each chain whose
payload is decoded. Exercise the composed callback with real
`{tool_name, tool_input, cwd}` input, because a file in a source list is not
proof it ran.

## Fixture matrix

Pair every supported dangerous entrance with its named sanctioned
alternative, plus nearby text, quoted mentions and scoped passing forms,
because a refusal alone cannot show the sanctioned route still works. Include
malformed, oversized and unavailable-state input; permission or activity off;
and exact session force-live/off behavior. Exercise Codex and North paths
against the same identity.

Firn native-policy fixtures also cover hosted-runtime independence, the
edit-family and Bash decoders, and the intentional `apply_patch` omission.
Lifecycle-wrapper fixtures exercise all five adapters and show that inactive,
missing or invalid generations drain stdin and delegate nowhere.

Run the nearest affected checks once under the verification skill. Missing owner or
decoder evidence blocks that wiring; prose or a second identity cannot fill
the gap. After publication and any required promotion, confirm exact owner,
support provenance, resolved activity and activation paths with
`agents inspect <id>`.
