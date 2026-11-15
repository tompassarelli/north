# The laptop runs the screen; the control plane runs on a build host (north#1572)

## Why

On the night of 2026-11-14 to 15 every desktop freeze on the operator's laptop (whiterabbit) traced back to fleet machinery running there. Each one was fixed alone under north#1553: broker-run compiles, the lander sheriff's reruns, runtime and closure copies passing through the laptop, the disclosure judge opening browser sign-ins, and `north health` reading about 3 GB from disk every 2 minutes (836 MB after 12acacbfa5, and only new transcript lines since). Fixing them one at a time keeps finding the next one. They have one cause: the lander, the health and tempo timers, the brokers, the runtime follower and the fleet's own sessions all run on the laptop.

This plan fixes that cause. The laptop runs the operator's screen, his own sessions and what those need to reach the fleet. The control plane runs on a build host by default, and the screen talks to it.

Tom's design call (2026-11-15, relayed by the coordinator): the control plane runs on the build host by default. One break-glass command moves it to the laptop and back. Only one place runs it at a time, handed over through one exclusive lease so two landers never land at once, and the queue, logs and state move with it.

## Inventory: the user units on whiterabbit

Read from `~/.config/systemd/user` on 2026-11-15 (home-manager links into the Nix store). The units are defined in nixos-config `modules/<name>/default.nix` and switched on for this host in nixos-config `hosts/whiterabbit/configuration.nix` and south `hosts/whiterabbit.nix`. The build hosts' units are defined in south `hosts/<host>/`. A twin is a copy of the same routine that already runs on another host, per its south `agents/routines/<name>@<host>.md`.

### Control plane: moves to the control-plane holder (gex45 by default)

- One `north-land@REPO.service` for each of eleven served repositories and `north-land-rest.service`, the landers (`north land serve`).
  - Reads: `~/code/<repo>/main` and its lander worktrees, `~/.local/state/north/land-queue/<key>/` (tickets, the outbox of each remote host over ssh, holds), the offload slots on gex45 and cpx62b over ssh, and the lander's push identity.
  - Writes: origin main (one push per candidate), `land-queue/<key>/{done,logs}` with `logs/spans.jsonl`, `pushes.tsv`, `stages.tsv`, fleet messages to each lane's author, and `north land promote` of the live runtime.
- `north-land-shadow.timer`, every 2 minutes: the lander doctor, then a check of the next queued candidate on a host with room. Reads the land queue. Writes the shadow verdicts the lander reads.
- `north-land-drill.timer`, nightly: injects stuck states into a scratch lander. Reads and writes only scratch.
- `north-tempo.timer`, hourly: `north ledger tempo --record`. Reads `land-queue/<key>/done/`. Writes `land_tempo` in the ledger, and fails when a learning loop the lander holds is stale.
- `harness-health.timer`, every 2 minutes: `north health`. Reads the land queue, the local units through `systemctl` and `journalctl`, the GitHub mirror and the ledger. It also reads the typed turns of the laptop's transcripts through `typed-said.db` beside the convo index (the 836 MB). Writes alarms to the coordinator.
- `agents-flow-stall.timer`, every 5 minutes: `north ledger flow --stall-alarm`. Reads the land queue, origin main and the same typed turns. Writes the stall alarm and the landing-is-stalled issue.
- `north-watch.timer`, every 30 seconds: `north ops watch --once`. Reads GitHub events and the land queue. Writes hiccup messages to the coordinator and to the lane's lead. A gex45 twin already runs, and an event alarms only from the lander host.
- `verify-north.timer`, daily: an end-to-end worker, enqueue and landing run. It reads the promoted runtime and needs tmux and a Claude sign-in on its host.
- `north-runtime-follow.timer`, every 20 seconds, and `north-stable-hold.service`: promote north's origin/main to this host's runtime, held by `runtime-promote.lock` while the stable pin exists. Reads origin/main. Writes `~/code/north-data/runtimes/` and `~/.local/state/north/current`.
- `north-credentials.timer`, daily: renews machine credentials and batches the renewals that need Tom.
- `firn-host-follow.timer`, every 20 seconds: pins the overlay to firn's main and deploys main to this host and to gex45, cpx62, cpx62b and nexus. cpx62 already builds and signs the toplevel. The laptop still pins, evaluates and pushes.
- `firn-delivery-liveness.timer`, every 30 minutes: the delivery floor of that follower.
- GitHub-only timers with nothing local to read: `north-github-mirror` (a Nexus twin exists), `outside-watch`, `supply-chain-scan`, `watch-review`, `eta-calibration`, `mutant-sweep` (its work already runs on a build host), `coverage-map-refresh` (records on a check host), `cross-project-scan` and `north-land-drill`.
- Pacing: `pace-enforcement.timer` (a gex45 twin exists). `usage-sample.timer` stays wherever a Claude login lives; it already runs on gex45 and cpx62b.
- The fleet's own sessions: the commit-reviewer and second-reviewer, the disclosure judge, leads and their `north fleet watchdog` processes, and the outside broker socket `$XDG_RUNTIME_DIR/north-broker/broker.sock` they call. On 2026-11-15 all of them ran on whiterabbit. gex45 is already an agent host (south `modules/agent-host.nix`, `fleet-tick`, `laptop-runner`, `lease-reaper`).

### The laptop keeps

- The screen and input: the compositor, its shell, idle and clipboard helpers, dictation, the editor, the terminal and the game controller bridge.
- Tom's own sessions, such as the coordinator and any session he opens himself, and the runtimes they run on.
- What those sessions need to reach the fleet: the shared ssh connections the laptop opens to each host, the fleet spool flush that publishes its sessions, the mirror that pushes claims and reports to Nexus, the gh broker for Tom's own calls, and a runtime follower that fetches the built closure from the binary cache and never builds (step 5).
- Housekeeping over the laptop's own worktrees, checkouts, transcripts and terminals. Each piece follows the sessions it reads, so it stays only while sessions run here.
- The machine itself: its OS update, hardware and load watchers, and the capacity units that take offload runs only while the laptop is away.
- Tom's personal mail, backups and game installs. These are his own data and use, not fleet work.

The kept units are listed by name in south `agents/decisions/harness/laptop-kept-units-2026-11-15.md`.

## Design

### One lease, one holder

`north control-plane` is a new `north-ops` module with two commands.

- `north control-plane where` prints the holder host and the lease's epoch.
- `north control-plane move --to HOST` is the break-glass command. It runs from the laptop.

The lease is one row in the Nexus ledger that both hosts already read and write: holder, epoch and renewed-at. It changes only by compare-and-swap on the epoch. Every control-plane unit is installed on both whiterabbit and gex45. Each starts through `north control-plane gate UNIT`, which exits 0 at once and runs nothing when this host does not hold the lease. The lander renews the lease on its tick and checks the epoch before each main push, so a lander that has lost the lease pushes nothing. The compare-and-swap push on main already keeps two landers from landing two different trees on one base. The lease adds what that push lacks: two landers would still take the same ticket twice, and would each write `done/`.

`move --to HOST` runs in this order:

1. Take the lease into a moving state (epoch+1, holder `moving:OLD->NEW`). Every lander stops at its next tick and finishes or abandons its running candidate. Abandoned tickets stay queued.
2. Stop the control-plane units on the old host.
3. Copy the state over the laptop's ssh paths with rsync. The copy carries `land-queue/` (tickets, `done/`, `logs/`, holds and freezes), `pushes.tsv`, `stages.tsv`, the shadow verdict store, `alarms-told.json` and the stable pin. The lander worktrees and `target/` do not move, because the new holder rebuilds them from `~/code/<repo>/main`.
4. Set the holder to NEW (epoch+2) and start the units there.

`landing.lander-host = control-plane` in a repository's `.safe-push` resolves to the lease holder. The flip therefore needs no landing, which matters most when the lander is the thing that is down. A host name keeps working as it does today.

The lease takes no automatic takeover and no timeout. Tom's call names a break-glass command, not failover, and no incident so far needed one. `move --to HOST --from-dead` skips step 2 when the old holder cannot be reached.

### Outbox direction

Today the lander pulls each remote host's outbox (crates/north-landing/src/land/outbox.rs). With the lander on a build host, a laptop enqueue pushes its ticket and lane bundle into the holder's queue instead. The pull stays for build hosts. Why the direction is reversed is recorded in south `agents/decisions/infra/control-plane-host-access-2026-11-15.md`.

### Health's laptop-only facts

`north health` and the flow stall alarm read the typed turns of the laptop's transcripts. Once sessions launch elsewhere (step 6), only Tom's own sessions are left on the laptop. The laptop keeps its incremental reader (12acacbfa5 reads only lines it has not seen yet) and pushes the typed-turn rows to Nexus with `north-dispatch-mirror`, which already pushes claims and reports every minute. Health on the holder reads those rows from Nexus. The laptop's unit checks reach the holder through `fleet-tick`'s published host state, so health on gex45 still reports the laptop's failed units.

### Credentials

Who signs (agents as the North Fleet App, the lander as the North Lander App) is unchanged. What the holder needs to push, and how it gets it, is recorded in south `agents/decisions/infra/control-plane-host-access-2026-11-15.md`.

## Order, each step with its revert

1. **Lease and break-glass command** (north). Adds `north control-plane where|move|gate`, `landing.lander-host = control-plane`, and the epoch check before the lander's push. The holder starts as whiterabbit, so behaviour is unchanged at landing. This step changes the lander, so each patch id needs both the commit-reviewer's and the second-reviewer's accept.
   - Revert: `git revert` the commit, and north's `.safe-push` returns to `landing.lander-host = whiterabbit`.
2. **Outbox pushed to the holder** (north). The laptop enqueue writes into the holder's queue when the holder is another host. It needs both reviewers.
   - Revert: `git revert` the commit, and the lander pulls outboxes again.
3. **Units on both hosts** (nixos-config modules, south `hosts/gex45`).
   - Enable `north-land` for every repository, plus `north-land-shadow`, `north-land-drill`, `north-tempo`, `harness-health`, `agents-flow-stall` and `verify-north` on gex45, each started through `north control-plane gate`.
   - Wrap whiterabbit's copies the same way.
   - Clone `~/code/<repo>/main` on gex45 (south `modules/north-clean-install.nix`) and give gex45 what the south record names.
   - Inert until a move.
   - Revert: revert the south and nixos-config commits, and the gex45 units disappear on the next deploy.
4. **First move: `north control-plane move --to gex45`.** It moves the landers together with the units that read their queue: `north health` (the heaviest disk reader), `north-tempo`, the flow stall alarm, `north-watch`, the shadow, the drill and `verify-north`. Health and tempo cannot go first on their own, because they read `land-queue/`, which lives with the lander. Health's typed-turn rows arrive through `north-dispatch-mirror`, added in this step.
   - Check: `! systemctl --user is-active --quiet north-tempo.service` exits 0 on the laptop while `north health --dry-run` reports from gex45 (north#1572, second box).
   - Revert: `north control-plane move --to whiterabbit`.
5. **Runtime promotion and the follower.** `north land promote`, `north-stable-hold`, `north-credentials`, `firn-host-follow`'s remote deploy and `firn-delivery-liveness` move to the holder. The laptop keeps a follower that substitutes the promoted closure from the binary cache (south's cache on wg-build) and never compiles. This changes the promotion path, so it needs both reviewers.
   - Revert: re-enable the laptop's units in nixos-config `hosts/whiterabbit/configuration.nix` and south `hosts/whiterabbit.nix`, then `north control-plane move --to whiterabbit` for the promote lock.
6. **The fleet's sessions.** The commit-reviewer, second-reviewer, disclosure judge, leads and their watchdogs launch on gex45 by default, with the outside broker beside them. Browser sign-ins stay a session on the laptop only when a step needs Tom's hands. The coordinator's interactive session stays on the laptop as Tom's screen.
   - Revert: set the launch placement back to whiterabbit.
7. **The rest of the GitHub-only timers.** Retire `north-github-mirror` and `pace-enforcement` on the laptop, because their twins run on Nexus and gex45. Move `outside-watch`, `supply-chain-scan`, `watch-review`, `eta-calibration`, `mutant-sweep`, `coverage-map-refresh` and `cross-project-scan` to gex45. These need no lease, since none of them lands anything.
   - Revert: re-enable each unit in whiterabbit's host file.

Every check and alarm keeps running through the move. At each step the holder runs the same units, the lease decides which copy acts, and `north control-plane where` names it.

## Where this changes the recorded decisions

- docs/rationale/landing-train.md
  - **D4** (2026-11-04) put the lander on the laptop as a user service. This plan supersedes D4 for the lander host: the lease holder runs it, gex45 by default. The reasons D4 gave against Nexus (3 cores) do not apply to gex45, which already runs the warm offload slots the checks use.
  - Design step 2 ("runs as a user service on the laptop") becomes "runs on the control-plane holder".
  - The rejected "Laptop as lander" line now holds without the D4 exception.
  - Wave 2's goal ("landing no longer depends on the laptop being awake") is met by gex45 rather than Nexus. B17's unit sandboxing is not part of this plan.
- docs/rationale/agent-push-identity.md
  - The identities are unchanged.
  - What the holder needs to push is in south `agents/decisions/infra/control-plane-host-access-2026-11-15.md`.
- South `agents/decisions/infra/nexus-first-placement-plan.md` (2026-10-13) put the sessions on Nexus and the laptop as signalled capacity. The direction is the same: the laptop is not where fleet work runs. The host changes to gex45, because Nexus has 4 vCPU and the lander's checks already run on gex45's slots.
- South `agents/decisions/harness/rebuild-craftsmanship-audit.md` places health, watchers and infra placement in the ops modules. `control_plane` joins them there, and no new crate is added.
- South `agents/rules.toml` additions-point-to-evidence:
  - Each unit moved here points to the freezes of 2026-11-14 to 15 under north#1553, or to Tom's call of 2026-11-15.
  - The lease has no timeout and no automatic failover, because nothing points to needing them.
  - The `--from-dead` path points to the laptop's sleep, which D4 records as pausing the queue.
