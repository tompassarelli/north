# Saturation of 2026-11-12: live compiles, work that had lost its caller, and a queue that admitted nothing

Record for north#1247, 2026-11-13. The saturation day is read from the three build hosts' own
records: the owner's read of gex45, cpx62b, cpx62 and the laptop at about 09:20 UTC, the admission
records for 02:15 to 02:45 UTC, and the CPU-minute readings of gex45 and cpx62b. The two commits
this reading found are f178f6047c (north#1252) and 397bb5f628 (north#1305).

## What the load was made of

- **gex45 (20 threads)**, 1-minute load 20.7 then 33.6. Every busy tree was younger than 5 minutes and
  owned by a live land check, offload run, worker or spare session unit; the only long-lived CPU users
  were two idle farm containers and the disk-keep timer. Admission had 1 to 3 waiting while 3 of the
  checkout's 6 slots were held, and leased ceilings reached 27 against a 20-core limit.
- **cpx62b (16 threads)**, load 17.9 then 10.7: rustc compiles of live land checks and one
  `fold-learning` worker unit, nothing older than 10 minutes with CPU time, queue 0.
- **cpx62 (16 threads)**, load 6.1 then 12.1: probes and the buck2 lead scopes of live sessions,
  queue 0.
- CPU minutes on the day: gex45 7,762.7 over 332 landed commits (23.4 per landing; 17.2 for the
  landing stages alone). cpx62b 8,278 under capacity leases (20.0 per landing) inside 12,566
  whole-host CPU minutes (30.4 per landing), so 4,288 CPU minutes, 34%, ran outside every lease.

The peak was therefore live landing work: land checks compiling and testing, offload runs, worker
gates and the sessions that started them. Nothing at that moment was an idle tree holding a slot.

## What had no live owner, and why it was still running

Two kinds of work whose owner had ended were found from these records, one on 2026-11-12 and the
other the next morning, and the 4:20 PM read could not see either: in both the process that should
have ended them was itself alive, so a tree scan and the lease reaper both read it as owned.

- **A nested sweep outlived its check.** The mutant-kill gate runs cargo-mutants in place as a nested
  job in its caller's slot. It took no lock of its own, so when the caller's land check hit its
  deadline and freed the slot, the sweep kept writing the lane's files into it, and main's
  after-landing run, holding that slot, compiled the mixed tree (2026-11-12, twice). f178f6047c
  gives a mutant sweep the checkout's one `.background` slot instead of a land check's, and a nested
  job holds a shared lock on the slot's `.nested` file for its whole run (`script.rs:210`) while a
  watcher in its own session waits on the holder and stops the job's process group, TERM then KILL,
  once that holder ends (`script.rs:536`, `:549`).
- **An offload outlived its client.** The caller holds a job's standard input open for as long as it
  lives, but nothing read it then, so a killed client or a closed ssh left the remote script running. Found
  on 2026-11-13 at 11:29 AM: 2 of cpx62b's 6 slot holders were nextest runs whose laptop client was
  gone (14 and 8.5 minutes old), and 4 tickets waited on gex45 for callers that no longer existed.
  397bb5f628 has the slot script watch that input: a job still in line ends its process group at once,
  a running job after 18 s with no client reattached while the shared ssh connection lives, or after
  the client's reattach window plus 30 s once that connection has ended
  (`TETHER_MARGIN_SECONDS` and `DROPPED_MARGIN_SECONDS`,
  `crates/north-runtime/src/land_offload/ship/script.rs`).

The rest of the unleased share was not ownerless: it belonged to sessions, the nix daemon, the build
runner and the sampler. Those have an owner but no lease, which is why admission's CPU share could
not charge for them.

## Why the queue admitted nothing

The rule at the time: moderate and heavy tickets waited in arrival order, and a ticket ahead lost its
place only if the host held its class, if the free CPU share could not fit it and it had waited under
`backfill_seconds` (300 s), or if it was a heavy held by `DEFER_RUN_QUEUE`, which a moderate passed
with no time limit.

- The head of the line was a heavy request, first deferred at 9:28:40 AM (`DEFER_CPU_PRESSURE`) and
  admitted at 9:42:06: 13 minutes 26 seconds in front of everything. No other moderate or heavy lease
  was granted in that span; only land checks, which go ahead by class, ran.
- Behind it waited three ticketed requests and about one heavy probe a minute: 233 `DEFER_QUEUED`
  decisions together.
- How many would have fitted: none. Until the second the head ran, the host's CPU stall read 47.5 to
  77% against the build host's 40% bound (`offload_job_cpu_psi`), which defers moderate and heavy
  work on its own, and load ran 17 to 71 on 20 threads while one 6-CPU lease ran. The only
  `DEFER_QUEUED` under the bound came at 9:42:06, as the head was admitted.

So the line was held by the load, not by its order: unleased and ownerless work kept the pressure
band above the bound, and admission therefore refused every request that would otherwise have fitted
behind the blocked head. The order rule made the wait worse in the other direction: a heavy held by
`DEFER_RUN_QUEUE` could be passed by a moderate with no bound at all, so it could also starve. A
capacity request that fits now passes a queued ticket the host would not admit on its own, with
`backfill_seconds` as the bound and `passedOverSeconds` in `status` (814670b510).

## What stops it now

- **No live owner.** A background command run by `north capacity bounded` sits in a guard scope bound
  to the fleet session scope (`--property=BindsTo={scope}`,
  `crates/north-ops/src/ops/run_bounded/mod.rs:64`), and its service is bound to that guard
  (`:89`); a session's own tools are bound to their session unit
  (`crates/north-base/src/platform/session_scope.rs:33`). A nested offload job holds its caller's slot
  lock and a watcher ends its process group when that holder ends (f178f6047c;
  `crates/north-runtime/src/land_offload/ship/script.rs:210`, `:536`). A remote offload ends when its
  caller's input closes (`:159`). Each of these ends the process group from the unit that holds it,
  so nothing has to find the work afterwards.
- **A restarted lander.** It adopts the after-landing runs and the alone check it finds
  (`crates/north-lander/src/land/watch.rs:210`,
  `crates/north-landing/src/land/alone_flight.rs:79`), hands its runs back when it stops, and reruns
  from the journal what it cannot adopt (4b6bf5de46, 595d993e2b). The adopted-run path is shown by
  `cargo nextest run -p north-lander --test it -E 'test(chaos_lander_stopped_by_sigterm_mid_after_run_adopts_its_live_runner_and_reverts_the_red_lane)'`.

## What this reading leaves open

The 4:20 PM read named no ownerless tree, so it did not name either kind above; they were found from
the same hosts' records afterwards. How much of cpx62b's 34% unleased CPU could be charged is not
measured here.
