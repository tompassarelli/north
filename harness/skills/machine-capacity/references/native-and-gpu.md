# Native clients, GPU and gaming

Rules for Warcraft clients, private desktops, native evidence runs, GPU work and Tom's gaming holds. Thresholds and line order are in [admission details](admission-details.md).

## Native sessions

- Use `session --class native --new-clients N --owner OWNER -- COMMAND ARG...` for Warcraft clients and private desktops (one scope per client, two per pair), because they must outlive a batch deadline. `native-launch-guard` refuses wine, proton, steam-run or a game .exe started outside one.
- Keep native sessions foreground until command exit, Ctrl-C or explicit stop, because a detached tree escapes its scope.
- Declare `--new-clients N` on a native `run`, `session` or `probe`: 1 for a clone or one client (the default), 2 for a pair start, 0 for a run on clients already up, because admission defers only when live Warcraft clients plus N exceed the host's client cap: its measured `warcraft_clients` (else four when attended) as a ceiling, lowered to the live clients plus those free BAR1 (`bar1_client_mib` each after `bar1_margin_mib`) and free VRAM (`vram_client_mib` each after `vram_margin_mib`) from the same `nvidia-smi -q -d MEMORY` still fit (north#259).
- Declare `--new-desktops N` beside it for the private desktops the request starts (2 for a pair whose clients each get a desktop), because admission defers while live BAR1 Free is below the declared desktops and clients at their measured MiB plus a margin.
- Pass `--wait-seconds S` on a native `run` or `session` that starts clients to wait for admission in arrival order for up to S seconds, then exit 75 with the last DEFER, because a caller retrying a DEFER on a timer loses the freed client slot to whoever asks during its gap (wisp#76).
- Request native-only `--memory-gib 1.5` for measured smaller offline clients, because it charges 1536 MiB instead of 4096 MiB for that client only.
- Scale native pairs one at a time and stop adding above protectedCpuSomeAvg10 20, because Warcraft clients are the heaviest steady load.

## Evidence runs

- Start native evidence runs (accept, gate runs, capture batches, native soaks, pad scripts) with `session --class native --new-clients 0 --quiet --quiet-seconds N` for the run's duration only, because a declared length is what makes the lease hold heavy tests and builds, which pass a quiet lease that declared none (north#335), and because I/O, copies and memory pressure from batch work stall both clients even at `nice -n 19`; moderate, heavy, exclusive and gpu work then queues until the run ends and is refused when its own timeout runs out first.
- Never mark idle pair pools, clones or private desktops quiet, because they are live most of the day while nothing is measured.
- Pass `--beside-native NATIVE-OWNER` only for that evidence run's own build steps.
- Declare `--latency` on a native `session` or `run` whose clients serve or measure frame-timed play (Tom's playtest or signed-in match, a run measuring rollback), because only those sessions' own scopes count as CPU pressure that holds heavy and exclusive work; capture, accept, pool and desktop clients tolerate contention, so their pressure never holds batch (north#460).

## GPU

- Expect any request except agent and tom (the build Tom is waiting to play, which never waits on the GPU) that starts GPU work or names a native, private or gpu need to defer with DEFER_GPU_BUSY while Tom is present and the laptop GPU is at least 85% busy with no gpu lease running, naming `offload` when another host serves the need, because admission counts CPU and memory but an agent window admitted as `--class heavy` once held the 890M at 100% and made his desktop choppy.
- gpu is admitted on GPU occupancy, not behind CPU-heavy tickets.

## Gaming and game reserve

- Expect the laptop-auto tick to set the mode each 30 s (`north capacity mode status`): play while Tom's game (Warcraft III or a Wisp native window) runs and for `capacity_mode_play_release_sec` (10 min) after it exits, or while a quiet hold from his own prompt lasts (the UserPromptSubmit phrase hook; it expires after `capacity_mode_hold_sec`, 2 h, or when he says he is done, which also closes Warcraft III, Battle.net and native test clients); away when the seat is idle; desktop otherwise. Play reserves the game CPUs (8 at first, sized by protectedCpuSomeAvg10), pauses heavy batch and holds the rest, and away and desktop reserve nothing. While play is on, native, heavy, exclusive and gpu admissions defer with DEFER_GAMING and each 30-second laptop-auto tick stops every native lease, agent Warcraft client and private desktop and freezes heavy, exclusive and gpu scopes, because agent GPU and memory load beside his game hard-froze the laptop; the same tick stops the scope of any run lease whose wrapper died, so no client outlives its runner, and thaws the frozen scopes once the hold is off.
- Expect play mode's game reserve to admit moderate batch on the agent CPUs it pins (the batch CPU limit while it is on) while heavy batch defers and its scopes freeze, because the reserve confines every slice of the user manager except app, session and game (agent, background, the podman user slice, native, control and any slice added later) to the agent CPUs, and `game-reserve status` exits 1 with NOT ENFORCED while one of them still reaches a game CPU; native, exclusive and gpu still defer with DEFER_GAMING, native scopes are still stopped and exclusive and gpu scopes frozen, and any non-agent request that would leave less than `game_reserve_headroom_mib` (24 GiB) of measured available memory defers with DEFER_GAME_MEMORY, so the game and desktop keep their memory.
- The reserve never outlives the game: the laptop-auto tick (every 30 s) runs the mode tick, which runs `game-reserve auto` while a game runs or no reserve is wanted, and that turns it on within one tick of Warcraft III or a Wisp native window starting and turns it off, however it was switched on, once no game has run for 10 minutes (`GAME_RESERVE_IDLE_SECONDS`).
