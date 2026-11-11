# The mayday ladder: one command per level, and a bottom that needs no north

In the week before 2026-11-11 every north outage was repaired through the part that was broken: the hooks, the Bash sandbox prefix and the lander all run the north binary, so a bad runtime stopped the work the harness exists for (north#1130).
The ladder is a way down that does not need it. Its scripts are bash under `harness/mayday/`, each at most 80 code lines, and `harness/mayday/tier0-bundle` installs a copy in `~/.local/state/mayday/bin`, outside every checkout, because the tree may be the broken part.

## The levels

| Level | Command | What runs | Depends on |
| --- | --- | --- | --- |
| 0 | `mayday 0` | full north: starts every unit `mayday` recorded as stopped | everything |
| 1 | `mayday 1` | north without the fleet: stops `fleet-tick.timer` and `north-dispatch-mirror.timer` here and `north-dispatch.timer` on the dispatch host | systemd, ssh to the dispatch host |
| 2 | `mayday 2` | Claude Code on a profile of north with hooks, skills, policy and the Bash sandbox; `--relink-stable` first points `current` at the stable pin | a healthy north runtime |
| 3 | `mayday 3` | vanilla Claude Code with only the portable core: rules, skills and deny rules, no hooks | bash, jq, the Claude Code runtime |
| 4 | `mayday 4` | raw Claude Code with empty settings | the Claude Code runtime |

`mayday N --dry-run` prints what a level would do and changes nothing; `mayday-status` prints the level, what backs it and any drift.
Levels 3 and 4 are started by `mayday-claude`, and `--dry-run` prints what it would run.
The launcher is a file of its own because starting a session and switching units are separate jobs, and the bottom of the ladder must keep working when the unit half is what fails.

## The bundle

`tier0-bundle` reads `tier0.list` beside it: rules and skills under `harness/` come from the tree the script sits in, Tom's policy from the south checkout, and the deny rules from `tier0-settings.json`.
It builds both directories beside the old ones and swaps them in by rename, keeping session history.

## The guard

`mayday --self-test` builds the bundle from its tree into a scratch directory, then for that bundle and the installed one resolves levels 3 and 4 with the stripped PATH and prints the exact command each would exec.
It fails when the PATH still reaches `north`, when the config directory, the runtime or the account token is missing, or when the level 3 settings carry no deny list. It makes no model call and takes about 0.2 s.
It is the land check `mayday-self-test` on lanes that change `harness/mayday/`, and the guard of hold H5 in `harness/holdlist.toml`.

## When the script is broken

Run the command `mayday-claude 3 --dry-run` prints by hand; it needs only bash and the Claude Code runtime. The worked command and the machine-specific notes are kept in the private overlay.

## Climbing back

`mayday 0` restarts what `mayday` stopped, from `~/.local/state/mayday/stopped`. Work done at level 3 is pushed as `mayday/<topic>` branches (the bundle's preface refuses a push to main), and the lander takes them once north is back.

## The stable pin

`stable-pin show` prints each host's runtime, pin and units; `stable-pin hold SHA` writes the pin file `~/.local/state/north/channel/stable` on every host and starts `north-stable-hold.service` on the laptop; `stable-pin release` removes the pin and stops the hold.
The pin file gates `north-runtime-follow` and `agent-runtime-update` through their `ConditionPathExists` line, so `hold` stops or masks no timer; the hold unit keeps the promotion lock against the lander's after-land promotion.

## T3 Code as a second client

`mayday 2 --t3` serves T3 Code on the profile of north (hooks, skills, policy and the Bash sandbox prefix), `mayday 3 --t3` on the level 3 bundle with no north on PATH; both bind 127.0.0.1 and print the address to open. `mayday-t3` holds the server logic.
`mayday 2 --t3 --self-test` serves headless on a scratch configuration linked to the profile's settings, policy and skills, asks one Haiku thread in a scratch repository for `git add -A` and passes when the thread shows the guard's refusal (53 s on 2026-11-11); at level 3 it asks for `git stash list` and expects the deny rule (11 s).
Review of the runtime, per the external-code skill (2026-11-11): T3 Code v0.1.8, the `t3-0.1.8-linux-x64.tar.gz` release asset of https://github.com/pingdotgg/t3code, MIT (the `LICENSE` of the checkout, copyright T3 Tools Inc.); used as a program, nothing copied into north, so the only obligation is the notice the archive carries.
It is pinned by the release's `SHA256SUMS` (`b9c49dc0…50a9e5` for linux-x64), verified before unpacking into `~/.local/lib/t3/versions/0.1.8` with a `current` link; `T3_RUNTIME` names another binary.
Its telemetry is switched off in the launcher's environment (`T3CODE_TELEMETRY_ENABLED=false`, `OTEL_SDK_DISABLED=true`), and it is served on loopback only; the full run-time review is kept in the private overlay.
It beats what exists as a fallback because it is a second, independent client over the same Claude Code runtime and profile: a north TUI or fleet fault does not reach it.
