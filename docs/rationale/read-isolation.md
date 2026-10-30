# Worker read isolation

Measured 2026-10-30 for north#59 step 1 (north#361): which Claude Code settings stop a worker session from reading
session credentials (`~/.claude/sessions`) and reaching peer session sockets (`/run/user/1000/cc-socks`), whatever
command it runs. Codex is not measured: north plans no new Codex work, so a Codex row waits for Codex workers to return.

## Installed version and probe

Claude Code 2.1.314 on Linux, `bwrap` from the system profile, `socat` taken from the nix store because the profile
has none. `harness/scripts/read-isolation-probe.sh CONFIG` builds a scratch tree holding a fake key
(`FAKEKEY-361-SECRET`) and a fake peer socket (a `socat` listener that answers `PEER-361`), starts headless
`claude -p --model haiku --settings` sessions (one for the five Bash probes, one each for Read, Grep and Glob) with the config's settings, and prints `leaked`, `blocked` or
`no-result` (the model made no tool call, so rerun that config) for each tool from the tool results. Real key files are never read: the protected paths are the scratch
copies, and the settings are passed per session so a lead started without them keeps its access.

## Measured table

| Config (settings passed to the session) | Bash `cat` | Bash `cd dir && rg` | Bash `rg` over the parent | Bash `tar` | Bash socket connect | Read | Grep | Glob |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| none | leaked | leaked | leaked | leaked | leaked | leaked | leaked | leaked |
| `sandbox.enabled` + `sandbox.filesystem.denyRead` | blocked | blocked | blocked | blocked | blocked | leaked | leaked | leaked |
| `sandbox.enabled` + `permissions.deny` `Read(//path/**)` | blocked | blocked | blocked | blocked | blocked | blocked | blocked | blocked |
| both | blocked | blocked | blocked | blocked | blocked | blocked | blocked | blocked |

What the table shows:
- `sandbox.filesystem.denyRead` is enforced by the bwrap sandbox on Bash and everything it spawns, so it holds against
  any command string. The native Read, Grep and Glob tools run outside that sandbox and still read the path.
- A `Read(...)` deny rule covers the native tools, and with the sandbox enabled Claude Code merges it into the
  sandbox's read denials (the `denyRead` schema text says so), so one rule covers all eight probes.
- A `Read(/abs/path)` rule is relative to the settings root, not absolute (measured by hand with the first probe run: the same rule with a single leading
  slash left all eight probes leaked). An absolute path takes two slashes (`Read(//home/…/**)`).
- The sandbox needs `socat`. With `sandbox.failIfUnavailable` set, a session on a host without it refuses to start
  ("sandbox required but unavailable … socat not installed"), so the worker profile must carry `socat` before this
  config can be applied.

## Smallest configuration that holds

Per worker session: `sandbox.enabled`, `sandbox.failIfUnavailable`, `sandbox.allowUnsandboxedCommands: false`, and
`permissions.deny` `Read(//…/.claude/sessions/**)` and `Read(//run/user/1000/cc-socks/**)`. `denyRead` adds nothing
once the rule is present and is redundant. The later boxes of north#59 apply it through the firn north-profile and
check that ordinary worktree work is unaffected; this measurement does not build either.
