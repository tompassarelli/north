# Harness architecture

This document describes what enters an interactive Claude Code or Codex session on Tom’s NixOS system, where each surface is owned, and how each controllable part is disabled.

It distinguishes three kinds of truth:

- **Installed truth:** files and links present on the current machine.
- **Configured truth:** what North and Firn source says should be installed.
- **Effective truth:** what a provider demonstrably loads at runtime. Where runtime execution has not been performed, this document says so.

Snapshot date: 2026-08-03.

## The pipeline

```text
Firn durable profile      Active module instructions and skills
dotfiles/agents/AGENTS.md orchestration · coordination · …
              │                         │
              └──────────┬──────────────┘
                         ▼
               agents switchboard composition
               ~/.config/agents/CLAUDE.md
               ~/.config/agents/AGENTS.md
               ~/.config/agents/skills
               ~/.config/agents/activity.conf
                         │
       ┌─────────────────┼────────────────────┐
       ▼                 ▼                    ▼
Claude Code            Codex            managed workers
~/.claude/CLAUDE.md    ~/.codex/AGENTS.md ~/.agents/AGENTS.md
~/.claude/settings.json                     per-account CODEX_HOME
~/.claude/hooks                             /etc/codex/requirements.toml
~/.claude/skills                            /etc/codex/hooks
~/.claude.json MCP                          account/plugin state
plugins

North profile support surfaces
  north:profiles/tom/docs  ──► ~/.agents/docs
  north:profiles/tom/hooks ──► ~/.agents/hooks
```

Firn owns durable global policy and the switchboard. Optional consumer behavior
belongs to its North module or skill. Home Manager projects the composed target
and North-owned support surfaces into provider discovery locations.

## How `~/.agents` is composed

[Observed] `nixos-config:dotfiles/agents/AGENTS.md` is the durable global policy
source. `agents apply` appends instructions only for active sets and
writes the provider-neutral targets under `~/.config/agents/`. Static provider
hook adapters consume `activity.conf`, a derived projection rather than a
second policy source.

[Observed] `nixos-config:modules/north-profile/default.bnix` declares these Home Manager projections:

| Live path | Source |
|---|---|
| `~/.agents/AGENTS.md` | `~/.config/agents/AGENTS.md` |
| `~/.agents/docs` | `north:agent-profile/docs` |
| `~/.agents/hooks` | `north:agent-profile/hooks` |
| `~/.agents/skills` | `~/.local/state/north/skills` |
| `~/code/AGENTS.md` | the Firn-projected repository-root profile |

These are out-of-store links. A content change at an existing North source path is visible immediately. A change to the wiring itself requires a Firn/Home Manager activation.

[Observed] Claude’s settings are different: `~/.claude/settings.json` is a regular writable file seeded atomically on every Home Manager activation. Claude or the user may change it between activations; a later activation seeds it again.

[Observed] Claude MCP declarations in `~/.claude.json` are structurally reconciled during activation.

[Observed] Codex’s global `~/.codex/config.toml`, `~/.codex/hooks.json`, and `/etc/codex/requirements.toml` are Nix-managed. Direct interactive Codex normally changes `CODEX_HOME` to a subscription-account directory, so the account config replaces the global user config. `/etc/codex/requirements.toml` remains machine-wide.

[Unverified] Exact symlink metadata was not inspected with `readlink`; the link relationships above are derived from the live file contents and Nix declarations.

## Recomposition events

There is no single “recompose everything” operation.

- `agents apply` atomically recomposes global instructions, provider skill
  links, hook activity, and role-profile links from switch state.
- A landed edit under `north:agent-profile/` changes linked docs and hooks
  immediately; it does not bypass the composed instruction target.
- A Firn/Home Manager activation creates or restores the provider discovery links, seeds Claude settings, installs Codex requirements, and reconciles Claude MCP declarations.
- `north config context apply` materializes the selected Claude constitution into `~/.claude/CLAUDE.md`.
- `north config skills sync`, or a skills on/off mutation, stages an immutable skills generation and atomically repoints `~/.local/state/north/skills`.
- Codex enforcement promotion changes `/var/lib/north-enforcement/active/current`, which is referenced by root-managed hook commands without waiting for a system rebuild.

A later Home Manager activation restores every global instruction link to its
switchboard-composed target. It cannot reactivate a module by pointing a
provider at North's personal profile.

## Canonical harness state

[Observed] The canonical state file is:

```text
~/.local/state/north/harness.conf
```

Current contents:

```text
rebuild-coordination=on
rebuild-window=3600s
dispatch=auto
guards=on
```

Missing keys use code defaults:

- `context=full`
- hooks: on
- skills: on
- comms base: `db`
- native and managed comms: inherit
- comms enforcement: forced
- coordination: North
- learning: frozen/default policy
- routing: balanced when no routing state exists

[Observed] `~/.claude/my-config.state` still exists and says:

```text
dispatch=warn
guards=on
```

`north:cli/harness-state.clj` reads the canonical file first and consults the legacy file only when the canonical file is absent. Any consumer that reads the legacy file directly is wrong.

State writes are locked, private, and atomic. The first canonical write can seed values from the legacy file.

## A fresh Claude Code session

A plain `claude` launch passes through the Firn-owned wrapper.

### Account and executable

[Observed] The wrapper selects a Claude subscription account and sets `CLAUDE_CONFIG_DIR` to that account’s configuration root. Under `dispatch=auto`, account selection may query North, but the wrapper has a fallback to an already authenticated account.

[Inferred] The exact account chosen cannot be predicted from static files because it depends on current account availability.

The wrapper ultimately invokes the system Claude Code package.

### Constitution and project memory

Claude receives:

1. The provider’s built-in system prompt.
2. The selected account’s settings and provider state.
3. `~/.claude/CLAUDE.md`, linked to the switchboard-composed
   `~/.config/agents/CLAUDE.md`.
4. Provider-native `CLAUDE.md` files found from the working directory and its ancestors.
5. Any additional context emitted by lifecycle hooks.
6. Any enabled plugin’s session-start context.

The `agents` switchboard controls global instructions, optional module
instructions, skill links, and governed hooks. It does not control repository
`CLAUDE.md` files or provider-native state.

### Settings

[Observed] The live `~/.claude/settings.json` contains:

- model `claude-fable-5[1m]`
- effort `xhigh`
- North/Fram/Firn authoring and lifecycle hooks
- North status-line integration
- enabled Rust and TypeScript language plugins
- no Orchestration plugin or marketplace; that surface is switchboard-owned

Account settings mirror this surface through the wrapper’s account bootstrap.

### Hooks

The profile registry currently contains these classes:

- Authoring deny hooks: Firn, launch-critical, blind staging, and tripwire guards.
- Dispatch deny hook: agent-spawn guard.
- Advisory hooks: comment bloat, Racket build, and log compression.
- Context hook: Beagle SessionStart.
- Coordination hooks: North session end and hook detachment.

The `north-session-lifecycle` hook module contains `north-on-spawn`,
`north-on-stop`, and `north-mark-delegated`; the `assignments` skill claims it,
so the coordination and orchestration set gates apply to it too.

Consequences:

- `north config hooks all off` does not mean no hooks.
- Coordination hooks are intentionally excluded from the global hook sweep.
- Provider/plugin lifecycle hooks not registered in `registry.tsv` are outside the hook dial.
- The authoring launch escape affects only hooks categorized as `authoring`.

### SessionStart context

A fresh session can receive context from:

- `beagle-session-start`, when the project matches its Beagle conditions.
- `north-on-spawn`, which emits North coordination context and attempts presence registration.
- the switchboard-composed Orchestration doctrine, only when its outer module
  set derives active.
- other provider plugins.

### MCP

[Observed] Live Claude MCP declarations name:

- `fram`
- `north`
- `linear-mcp-msa-new`

The activation source configures Fram and North as local command transports and Linear as `https://mcp.linear.app/mcp`.

Presence in `~/.claude.json` proves declaration, not successful authentication or server health.

### Skills

Claude’s shared skills link is:

```text
~/.claude/skills -> ~/.agents/skills -> ~/.local/state/north/skills
```

[Observed critical drift] `~/.local/state/north/skills` is currently absent. The source inventory contains four North skills, but the live farm is not published. Consequently the shared North skill links are presently dangling or empty.

`north config skills` computes its summary from the source inventory. It can therefore report four enabled skills while no farm exists for providers to discover.

Provider/plugin-bundled skills are separate and are not controlled by the North skills dial.

## A fresh Codex session

A plain `codex` launch also passes through a Firn-owned wrapper.

### Account and executable

[Observed] The wrapper selects a Codex subscription account and sets `CODEX_HOME` to that account directory. It invokes the exact Nix-installed Codex package and supplies model, effort, permission, and thread-related overrides.

The account directories contain their own `config.toml`, authentication state, `AGENTS.md`, plugin state, and history. They do not inherit the global `~/.codex/config.toml` merely because that file exists.

### Constitution and project instructions

A direct Codex session receives:

1. Codex’s built-in system/developer instructions.
2. `$CODEX_HOME/AGENTS.md`, linked by account bootstrap to the full North constitution.
3. `AGENTS.md` files discovered from the working directory and its ancestors.
4. Provider or account plugin/skill metadata.
5. Hook-produced context and advisory output.

`project_doc_max_bytes=0` disables project instruction discovery, but does not suppress `$CODEX_HOME/AGENTS.md`. Local North provider code explicitly records that Codex has no supported switch that suppresses only global `AGENTS.md`.

### Hooks and machine policy

[Observed] `/etc/codex/requirements.toml` requires:

```toml
allow_managed_hooks_only = true
allow_remote_control = false
managed_hook_failure_mode = "block"

[features]
hooks = true
```

It also points Codex at the managed hook closure under `/etc/codex/hooks`.

This policy is root-managed and remains effective when `CODEX_HOME` changes. Redirecting `CODEX_HOME`, setting `AGENT_NO_AUTHORING_HOOKS`, or using the existing `codex-native` profile does not remove it.

The intended managed lifecycle includes Beagle and North SessionStart hooks, authoring and dispatch guards, post-tool hooks, and North stop handling.

[Unverified] The provider’s treatment of the separate user `~/.codex/hooks.json` was not runtime-observed. The Nix and SDK contract says the root-managed requirements surface is authoritative.

### MCP

[Observed] The global store-managed `~/.codex/config.toml` declares North, Fram, and Linear MCP servers.

[Observed] Direct interactive launches replace `CODEX_HOME` with an account root whose inspected config did not contain those MCP declarations. Therefore the global declarations are not sufficient evidence that a plain account-selected Codex session receives them.

Managed North-dispatched Codex lanes are different: the SDK constructs a temporary `CODEX_HOME` and injects an explicit North/Fram/Linear MCP set into the launch configuration.

### Managed lanes are not interactive sessions

North-managed lanes add a separate authority envelope:

- a temporary provider home
- a North-authored system/developer orchestration contract
- managed topology, role, posture, and capability facts
- explicit MCP servers and tool restrictions
- `project_doc_max_bytes=0`
- root-managed Codex hooks
- exact provider model/effort routing

The interactive provider profile and the managed-lane profile must not be treated as the same harness.

## Controls and their real scope

| Surface | Readout | Disable command | Important limit |
|---|---|---|---|
| Dispatch | `north config dispatch` | `north config dispatch native` | Selects native dispatch; does not remove hooks or context. |
| Authoring guards | `north config guards` | `north config guards off` | Only the authoring guard category. |
| One registered hook | `north config hooks` | `north config hooks off <id>` | Does not affect unregistered provider/plugin hooks. |
| Registered hook sweep | `north config hooks` | `north config hooks all off` | Coordination hooks are excluded. |
| Claude context section | `north config context show` | `north config context off <section>` followed by `north config context apply` | Claude only; no global zero command. |
| Shared North skills | `north config skills` | `north config skills all off` | Does not affect provider/plugin skills. |
| Communications | `north config comms` | normalize native/managed overrides to inherit, then `north config comms off` | A surface-specific override can mask base `off`. |
| Beagle per-file adoption | `north config beagle list` | `north config beagle unadopt <file>` | Per-file only. |
| Nix-installed provider integration | `firn tag status` | module/tag disable plus a coordinated system activation | Not immediate and can disable unrelated development facilities. |
| MCP declarations | provider files/UI | no unified North command | Claude activation can restore declarations. |
| Plugins | provider settings/registry | no unified North command | Outside hook/context/skills dials. |
| Whole harness | none | no existing command | This is the zero-switch gap. |

## Kill switches and launch escapes

### `north config guards off`

Disables the registered authoring guard category. It is not a global harness kill switch.

### `AGENT_NO_AUTHORING_HOOKS=1`

Canonical launch escape for authoring hooks.

### `CLAUDE_NO_AUTHORING_HOOKS=1`

Compatibility alias for the same authoring-only escape.

Nonempty values other than `0` and `false` mean off. `0` and `false` mean live.

These variables do not disable dispatch, context, coordination, MCP, skills, plugins, or Codex’s root requirement that the hook feature be enabled.

[Observed] The authoring escape does not disable the Beagle SessionStart hook. The registry categorizes that hook as `context`, and the hook resolver clears the authoring escape for non-authoring registered callers.

## What “fresh session” does not mean

A fresh provider conversation is not a fresh harness profile. Persistent account directories can retain:

- authentication
- history and session state
- provider settings
- plugin installation records and caches
- trust records
- disabled-hook coordinates
- MCP state
- skills or commands

The existing `claude-native` and `codex-native` profiles are persistent profiles. They are useful for stable subscription access, but they are not proof of a zero session.

## Current installed drift

As of this snapshot:

- Canonical state says `dispatch=auto`, while the legacy Claude state says `dispatch=warn`.
- The shared North skills farm is absent even though the source inventory contains four skills.
- The historical Orchestration plugin files remain as archive material but no
  provider settings enable them.
- Claude context state has no freshness or checksum proof against the materialized `~/.claude/CLAUDE.md`.
- Direct Codex account configs and the global `~/.codex/config.toml` expose different MCP surfaces.
- A worker-topology guard treats several semantically read-only `north config` subcommands as mutations.

These are control-plane visibility defects, not merely documentation omissions.

## Zero-session boundary

“Zero” should mean:

Excluded:

- `~/.agents` constitution, docs, hooks, and skills
- provider account settings other than subscription authentication
- user and project instruction files
- North/Firn/Fram/Beagle hooks
- `/etc/codex/requirements.toml` and `/etc/codex/hooks`
- provider-installed plugins and marketplaces
- North and provider MCP declarations
- North session-start, stop, coordination, logging, and dispatch integration
- North-related environment variables and wrappers

Included:

- the provider’s own built-in system behavior
- built-in provider tools
- the selected provider’s normal model defaults
- subscription authentication
- ordinary terminal and network access supplied by the provider
- explicit arguments typed after the zero command

A zero session must not require the North binary, socket, daemon, state parser, or account-selection service.

`north zero claude` and `north zero codex` launch this boundary. The command
branches before the normal `north` wrapper initializes, starts in an empty
temporary directory, and prints its disablement manifest before provider start.
For Codex it hides `/etc/codex` in a Bubblewrap namespace when available; if
that namespace cannot be created, the manifest names the still-visible
machine-wide requirements and hooks rather than calling the session zero.

---
