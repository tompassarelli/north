# Verdict reuse by recorded inputs: a test's pass carries to every tree where the files it read are the same

Plan of 2026-11-08 for the "Verdict reuse by recorded inputs" section of north#842. Tom asked for the most
ambitious, robust fix to the landing queue that day. The lander runs every check again for every probe,
so each lane added to the queue makes it longer. This note describes how to run a check only when
something it actually read has changed, while every commit that reaches main is still checked as its
exact merged tree.

## What is measured

- One check in 139 was reused in the hour to 6:36 AM on 2026-11-08 (north#842).
- North has two verdict stores, and both are keyed on more than a check reads:
  - The verdict cache (`north-base` `verdict_cache`, read by the slot runner's `reuse`) hashes every path
    that a check line's selector names. For clippy and nextest that is all of `src/`, `crates/` and
    `tests/`. For any check that runs north, it also hashes the base and head trees.
  - The shadow store (`north-lander` `land/reuse`) is keyed `LANE_HEAD:MAIN:INPUT`.

  A verdict from either store therefore carries only to an identical tree. Probes never repeat a tree,
  because main and each probe's set of lanes change every round.
- Of the 150 commits on main before this plan (5.9 hours), 132 (88%) touch a Rust input and 38 touch the root
  package under `src/`.
- Reusing a whole check by crate source hash was judged unsound on 2026-11-06
  ([landing-latency.md](landing-latency.md)): 27 test files read paths outside their crate through
  `CARGO_MANIFEST_DIR`.
- The sandbox needs Landlock, seccomp user notification with the continue flag, and a supervisor's access to
  `/proc/PID/mem` of its own descendants. The build hosts' readings of 2026-11-08 are in South
  (agents/decisions/harness/verdict-reuse-hosts-2026-11-08.md).

## Guarantee

A test's pass recorded on tree T1 is reused on tree T2 only when each of these holds:

- Every file-system observation the test made on T1 gives the same answer on T2. An observation is a
  file's content, a lookup that found nothing, or a directory's list of names.
- The command, toolchain, lockfile and environment are the same.

Argument: a deterministic process whose every observation answers the same takes the same steps.
By induction over its system calls, it reads the same files and ends with the same verdict. So
the merged tree is checked by equality of everything the check observed, and that holds for every commit
that lands.

Only a pass is reused. A failure always runs again.

Determinism beyond the file system is owned elsewhere: the clock, the network, scheduling, randomness and host
resources belong to the test-integrity land scan and the ambient-test-input rule (ec1231f242). This plan
does not add network or clock isolation (north#842 marks it not required). The shadow phase measures
whether the assumption holds.

## Prior art and what each one decides here

- **Bazel remote caching** ([references](../references.md#bazel-remote-cache)) keys an action on the hash of
  its declared inputs plus the tool identity. Its sandbox shows an action only those inputs, and it
  caches test results per test target. North cannot declare each test's inputs ahead of time (the 27
  files), so it takes Bazel's key shape and store protocol but not its declared-input model.
- **Buck2 dep files** ([references](../references.md#buck2-dep-files)): an action reports which of its
  declared inputs it actually used, and the next cache check hashes only those. North adopts the rule:
  key on what was used.
- **BuildXL** ([references](../references.md#buildxl)) caches on observed inputs with a two-phase lookup.
  - A weak fingerprint (command, declared inputs, environment) selects the path sets that earlier runs
    observed.
  - A strong fingerprint hashes the current answers at those paths, including absent paths and
    directory enumerations.
  - On Linux it observes accesses by interposing libc, and falls back to ptrace for static binaries.

  The store below uses this lookup.
- **tup** ([references](../references.md#tup)) records each command's file accesses through FUSE. It fails
  a command that reads a generated file it did not declare, and keeps "ghost" nodes for files that a
  command probed and did not find. North adopts the rule that a lookup which finds nothing is an input,
  and keeps FUSE as the fallback recorder.
- **build2** ([references](../references.md#build2)) discovers a compile's dependencies from the
  compiler's own output into a per-target database. North takes a compiled binary's inputs from
  rustc's dep-info, not from a trace.
- **Ekstazi** ([references](../references.md#ekstazi)) selects regression tests by dynamic file
  dependencies: a test runs again when any file it accessed has changed. Two points apply:
  - It shows that file-level dynamic dependencies per test are safe and cheap.
  - The JVM loads classes lazily, which gives Ekstazi type-level precision. A statically linked Rust
    binary has no equivalent, so north keys compiled code by the binary's whole source closure.
- **nextest** ([references](../references.md#nextest)) runs one process per test and calls a target
  runner as `BINARY --exact TEST --nocapture`; `north test coverage exec` already uses that hook. This
  decides the unit (one test) and the hook (the runner).
- **Landlock** ([references](../references.md#landlock)): an unprivileged, inherited path-beneath
  sandbox for reads and executes.
  - It restricts neither `stat` nor path walks, and it reports denials only through audit.
  - It can therefore fence the roots a test may read, but it cannot record what was read.
- **seccomp user notification** ([references](../references.md#seccomp-unotify)) shows a supervisor every
  listed system call made by the whole process tree. The supervisor reads the path from `/proc/PID/mem`
  and lets the call continue.
  - It covers static and dynamic binaries alike.
  - `io_uring` bypasses it, so the filter refuses `io_uring_setup`.

**Why not coverage** ([coverage-test-selection.md](coverage-test-selection.md)): changing a constant or a
type outside the executed functions changes the executed code without changing its source.
Selection can accept that gap because the full suite still runs after the push. Reuse replaces the run,
so it needs the file-level key.

**Why record rather than enforce a declared set:** enforcing a set recorded on an earlier run would fail
every lane that legitimately adds a read, such as a new fixture, and that costs a first try.
Recording every access means no read goes undeclared: each one becomes part of the key. A read that a
key cannot cover makes its unit unreusable, so that unit runs for real on every probe and never passes
silently on a reused verdict. It is not refused by default: a test that passes today would then fail
because of the harness, and lose a first try. Landlock enforcement is available as `--enforce` for the
replay test and for checking a path set. It becomes the default once the report shows no unit with an
ambient read, the way `ambient-test-inputs.tsv` only shrinks.

## Design

### Unit

- Each nextest test (binary id and test name) is one unit, and every other check line is one unit.
  Clippy is split per crate in a later lane.
- Scans over a range (`${SAFE_PUSH_LAND_BASE}..${SAFE_PUSH_LAND_HEAD}`) read history, so they stay keyed on
  the exact range. Each takes 0.1 to 5 s and runs beside the tests, not after them.

### Sandbox: `north land sandbox exec`

`north land sandbox exec [--enforce PATHSET] --record FILE -- CMD` is the nextest target runner for land
checks. It works in three layers.

**Process shape.** The workspace forbids unsafe code, so the sandbox never uses `pre_exec`:

- `exec` is the supervisor. It spawns `north land sandbox child` as a fresh process and waits for it.
- The child installs its seccomp filter, hands the notification descriptor to the supervisor over an
  inherited socket, applies Landlock when `--enforce` is set, and execs CMD.
- The supervisor returns CMD's exit status (128 + N for signal N).

**Fence (roots).**
- The roots are the checked tree, its target directory, `/nix/store`, `/dev/null`, `/dev/zero`,
  `/dev/urandom` and `/proc/self`, plus a short list of libc files under `/etc`, named in the code.
- Each unit also gets a fresh scratch directory that becomes `TMPDIR`; Bazel gives each test its own
  `TEST_TMPDIR` the same way.
- A path under scratch, or one the unit's own processes created, is scratch: a create or write by the unit
  comes before its read.
- Any other path is ambient: `$HOME` outside the tree, other repositories, sibling slots, the rest of
  `/etc` and `/proc`. The recorder checks every observed path against the roots, and an ambient path
  makes the unit unreusable.

**Recorder (seccomp user notification).**
- It traces `open`, `openat`, `openat2`, `creat`, `execve`, `execveat`, the stat family (`stat`, `lstat`,
  `newfstatat`, `statx`), `access`, `faccessat`, `faccessat2`, `readlink`, `readlinkat`, `getdents64` and
  `inotify_add_watch`.
- It resolves relative paths through `/proc/PID/cwd` and `/proc/PID/fd/N` at each call.
- It traces creates and writes (`mkdir`, `mkdirat`, `rename`, `renameat2`, `link`, `symlink`, `unlink`, and
  `open` with a write or create flag) to classify paths as scratch.
- It also traces `statfs`, `fstatfs`, `sysinfo` and `sched_getaffinity` as ambient reads.
- It refuses `io_uring_setup` with ENOSYS, because io_uring opens files without a system call the filter
  sees.
- The supervisor is an ancestor of every process it watches, so a ptrace scope limited to descendants
  still allows the `/proc/PID/mem` reads.

**Enforce (`--enforce PATHSET`).** Landlock narrows tree access to a recorded path set and refuses every
ambient root. A test may swallow the EACCES (`unwrap_or_default`), so the supervisor fails the unit with
`sandbox refused read of PATH` whatever the test returned. The replay test plants a read outside the set
and a read of an ambient root.

**Fallback.** If the recorder adds more than 10% to the nextest check's wall time on gex45, the recorder
becomes a FUSE view of the tree inside a user and mount namespace (tup), and the reason is written here.

### Path set

The recorder classifies each observed path relative to the tree root:

- **Tree** (not under `target/` or `.git/`):
  - A file is identified by its git blob id at the checked tree (one `git ls-tree -r` per tree).
  - An absent path is recorded by name.
  - A directory listing is identified by its sorted names and modes, not by its tree id, so changing a
    child's content does not invalidate the listing.
- **`target/`:** a cargo artifact (the test binary or a `CARGO_BIN_EXE_*`) stands for its compile inputs:
  - cargo's dep-info for the artifact, which lists every source rustc read across path dependencies,
    including `include_str!` and `include_bytes!`;
  - the values of its `# env-dep:` lines;
  - the artifact's metadata hash (features, profile, rustc).

  Any other read under `target/` makes the unit unreusable.
- **`/nix/store`:** the store hash in the path names the content, and `flake.lock` in the weak fingerprint
  covers it.
- **Scratch, `/dev/null` and `/proc/self`:** not inputs.
- **Unreusable:** reading the checked tree's `.git`, writing into the tree, an ambient path or an ambient
  system call each make the unit unreusable, with the reason recorded. The report names the files that
  keep tests unreusable, the way `ambient-test-inputs.tsv` does.

### Key and store

The lookup is BuildXL's two-phase lookup:

- **weak** = sha256 of:
  - the unit id;
  - the check line with its selection variables removed;
  - the toolchain identity (`rust-toolchain.toml`, `flake.lock`, `rustc -vV`);
  - `Cargo.lock` and `.cargo/config.toml`;
  - the test process's environment, minus an ignore list that gives a reason per name the way
    `NORTH_IGNORES` does.
- **Path sets** observed under weak, stored content-addressed.
- **strong** = sha256 of weak plus each path-set entry's answer on the tree under check.
- **Pass record** at (weak, strong): time, tree sha and host.

To look up a unit, compute strong on the current tree for each path set under weak. A pass at any of them
means the unit is reused. After each run, record the path set and, if the run passed, the pass.

There is one store: the verdict cache, grown to two phases. The shadow store's `LANE_HEAD:MAIN:INPUT` key
is folded into it and deleted. Today `land shadow tick` writes its lane verdicts only to that store, and the
only thing that reads the store is the tick itself, so the slot runner never reuses them. Once the shadow
store is folded, the tick writes under the verdict-cache key the slot runner computes.

Records are immutable, content-addressed files, so the lander host holds the union of every host's records:

- An offload job pulls the store's new files before it runs.
- The job's results carry back the records it wrote.
- Copying with ignore-existing in both directions is a safe merge.

Lookup asks only whether any run has seen these answers, so verdicts carry with no lane bookkeeping. A
lane's own pre-land check, every probe and each `land shadow tick` lane run all feed every later probe.

### Shadow, then on

**Shadow.** The runner computes would-reuse for every unit, runs the unit anyway, and logs: the landing, the
unit, whether it would have been reused, the real verdict, and whether the real path set equals the reused
one. A disagreement is either of these:

- reuse would have carried a pass, but the real run failed;
- every answer matched, but the real run's path set differs from the reused set. That is a read outside the
  recorded set, from a determinism bug or a recorder bug.

`north land reuse report --since D` prints:

- the landings and units counted;
- the share that would have been reused, by count and by test-seconds;
- each disagreement, named by unit and by the first path that differed.

**On.** After at least 100 landings with 0 disagreements, the runner exits 0 for a reused unit and prints
the existing reused-verdict line. Nextest then reports the unit passed and the span reader counts it as
reused.

### Expected reuse, and the lever if it falls short

**tests/it and binary spawns.** A test that spawns the north binary carries the whole Rust tree in its key,
and so does every tests/it test, whose binary links the root package. Such a test is reused only when no
other lane in the probe changed Rust. That is rare in a probe of several lanes. It is common in a retry
round and in the bypass candidates of north#842's design.

**Crate unit tests.** A crate's unit tests carry only that crate's dependency closure. In a probe of lanes
in different crates, most of them reuse the lane's own verdict.

**Measurement and lever.** The shadow report measures the reused share by unit and by test-second. If it
stays under the 50% box, the lever is to turn tests/it tests that spawn the binary into in-process calls.
That shrinks each one's key to its own crates. The `# narrow:` lines in `tests/it/crates.txt` already
declare that direction; this would make it sound.

**Selection.** Today's selection by crate closure (`SAFE_PUSH_LAND_PACKAGES` and `SAFE_PUSH_LAND_IT` from
`tests/it/crates.txt`) amounts to reusing main's verdict keyed on declared crates. That is the key
judged unsound on 2026-11-06. Once path sets exist, selection can read them: a test runs when an entry
of its path set has changed against main's recorded pass. That closes the gap too, but it is not part of
these lanes.

## Lanes

Each lane lands through safe-push. None of them touches the files that the landing-queue quick wins, the
shell-test span and the seventh-lane flake lanes are changing:

- `crates/north-land-checks/src/land/slots/runner.rs`
- `runner/build_host.rs`
- `runner/selection.rs`
- `crates/north-landing/src/landing/land_test_runner.rs`
- `land_test_selection.rs`
- `tests/it/land_serve/landing.rs`

The lanes:

1. **Sandbox enforce.** `north land sandbox exec` with the supervisor and child process shape, Landlock
   under `--enforce`, and replay tests for a planted ambient read and a planted read outside the recorded
   set. Closes the out-of-set read box.
2. **Recorder.** The seccomp recorder and the classified path-set file. It measures overhead on gex45 for
   north-base's lib tests, one tests/it module and one test that spawns the binary. It also runs the whole
   nextest suite on main under the runner and checks it gives the same pass and fail set as without it.
   Starts after lane 1.
3. **Store.** The two-phase verdict cache and the host merge. Runs in parallel with lanes 1 and 2.
4. **Shadow-tick fold.** The tick records under the verdict-cache key, and the `LANE_HEAD:MAIN:INPUT` store is
   deleted. Runs in parallel with every other lane.
5. **Runner and shadow.** The target runner wired into the nextest land check, path-set hashing (tree, git
   blob ids, dep-info closure), the shadow log and `north land reuse report`. Starts after lanes 2 and 3.
6. **On.** Starts after the report shows 100 landings with 0 disagreements. The runner reuses, and the 1 h
   scoreboard is read for the reused share and queue p90. Closes the shadow box and the turned-on box.
7. **Later.** Clippy per crate keyed on dep-info, scans that read no range, and test selection by path set.
