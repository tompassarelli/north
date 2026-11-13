# Agent-era security: a claim-by-claim review of "security is the new bottleneck"

Review of 2026-11-12 for north#1241: a summary Geoffrey Huntley posted of his November 2026 conference talk on
security in agent-driven engineering ([ghuntley-security-talk](../references.md#ghuntley-security-talk)), tested claim
by claim against north's, firn's and south's code and records, in the form of the 2026-11-09 review of his Nix post
([nix-leverage.md](nix-leverage.md), north#968), whose verdicts are cited here and not reopened. The review was on
paper: no sandbox, guard, gate, check or credential changed and no experiment ran. Each adopted action is an
experiment with one measurement and a size, filed as its own issue. Host- and account-specific readings, and the
details of faults still open, are in south (agents/decisions/harness/security-talk-2026-11-12.md).

The night of 2026-11-12 is the fleet's own evidence for this review: independent reads of landed commits found five
faults that clippy, the tests and every land scan had passed (forged fleet messages through the spool, text sent to
the privacy judge before its deny-list scan, a hand-over socket created with default modes, measuring code that read
token files itself, a promise checker that ran shell from report text). Each is fixed or filed.

## Verdicts

- Claim 1, attackers scan every public commit, so defence needs standing review capacity: right, and the fleet
  measured it tonight; adopted as a read of every trust-boundary commit before promotion, kept only if a week of
  reads finds faults (north#1244).
- Claim 2, most libraries can be replaced above a trusted core: half right; count first and replace only where one
  direct dependency brings in many crates for little use (north#1246).
- Claim 3, agents make hard tools (Nix, Bazel, Buck2) easy: already done; not a security claim here.
- Claim 4, one source of truth for every environment: already done (2026-11-09, claim 1); not reopened.
- Claim 5, remove the capability rather than forbid its use: right; the server rule of 2026-11-09 stands, and the
  part of "main moves only through the lander" that is still client-side is filed privately (south#119).
- Claim 6, patch any package fleet-wide without waiting for upstream: the overlay half stands as decided on
  2026-11-09 (claim 5); the scan that should start each patch reports an advisory once and misses Nix inputs
  (north#1243).
- Claim 7, a self-expiring note in AGENTS.md instead of a ticket: the expiry is right, the place is wrong; a dated
  field a check reads, not prose (north#1243).
- Claim 8, root only on disposable machines, never production, outbound locked down: sudo rejected on 2026-11-09
  (claim 4) and not reopened; outbound reach is measured first (south#120).
- Claim 9, deterministic checks instead of skills, and property tests: already the fleet's rule; tonight disproves
  "checks replace much of review" for trust boundaries, so every reader of worker-written text is inventoried and
  then checked (south#121), and one property test is tried where example tests stand alone (north#1245).

## Claim 1: publishing code is now unsafer, and defence needs inference

Strongest version: if attackers read each public commit within minutes, a fault in a public repository is exploitable
for as long as it is live, so the defender must read at least as fast as the attacker, and checks alone do not read.

What exists: every push to a public repository passes the deny-list and the private-text judge
([private-text-judge.md](private-text-judge.md)), and the lander runs clippy, the tests and the land scans of
`.safe-push` on the merged tree. Promotion of a landed runtime runs on fixed slots every four hours or at once for a
`Live-fix:` commit (`runtime_promotion_slot_sec` in harness/defaults.toml, crates/north-runtime/src/runtime/cadence.rs),
with a canary that probes behaviour (north#1022).

2026-11-09: not covered.

Where it is right and the fleet falls short: the five faults of 2026-11-12 passed every check and were found only by
reading. Fault a person can check: `rg -n 'fn taken|fn slot' crates/north-runtime/src/runtime/cadence.rs` shows a slot
takes the newest head whose runtime is built, and no record of a read exists for it to consult. Size: 617 of the 2808
commits that landed on north in the 7 days to 2026-11-12 changed a trust-boundary path (worker sandbox, broker, fleet
store, depbox, safe-push, guard, dependency gates, credentials, promises, the fleet CLI), about 88 a day.

Where it does not apply: "exorbitant" cost. The fleet spends subscription usage on fixed plans, so the cost of a read
is usage and delay, measured in north#1244, not a budget line.

## Claim 2: most libraries are no longer needed

Strongest version: each dependency is code that runs at build and run time and that the fleet did not read, so fewer
dependencies is a smaller attack surface, and an agent can now write a small one in minutes.

What exists: north's lock holds 265 crates.io crates (`grep -c 'source = "registry' Cargo.lock`) reached from about
28 direct external dependencies. Every lock entry a lane adds waits 7 days and passes cargo-deny at the lander, and
dependency code runs in depbox with no network and no credentials ([agent-user.md](agent-user.md)). cargo-vet was
measured on 2026-11-09: the published audits cover 26% of the lock, so it was not adopted.

2026-11-09: not covered; agent-user.md's dependency gates and cargo-vet verdict stand.

Where it is right: no record says which direct dependency brings in how many crates or what north uses it for, so
nobody can say which replacement would shrink the lock. Fault: the count does not exist.

Where it does not apply: code an agent writes is read by no one outside the fleet, so replacing a maintained crate
moves its bugs into an unreviewed place. The talk itself keeps crypto in its trusted core; north's core is crypto,
TLS, sqlite, Landlock, the terminal UI and the async runtime.

## Claim 3: hard tools are now easy to use

Strongest version: one hermetic build system across languages makes every patch the same procedure, and agents remove
the learning cost that kept teams off it.

What exists: Nix pins the toolchain and the tools of the dev shell (flake.nix, rust-toolchain.toml), and north moves
its build to Buck2 with remote execution so no compile input is built twice ([compile-reuse.md](compile-reuse.md),
north#1115, decided 2026-11-11), with its external-code review recorded there.

2026-11-09: claim 6, Nix pins the toolchain and nothing more; Nix-built Rust rejected.

Verdict: already done, for build reuse rather than security. Nothing new.

## Claim 4: one source of truth for every environment

2026-11-09, claim 1: already done for the Rust toolchain; devenv rejected; cloud workers' environment left to measure
first. Tonight's evidence does not bear on it. Not reopened.

## Claim 5: a git that cannot force-push

Strongest version: an agent climbing toward its goal will try the destructive path a rule forbids, so the rule that
holds is the one the system enforces whatever the agent runs.

What exists: GitHub refuses a non-fast-forward update and a deletion of main in every repository (north#973):
`gh api repos/tompassarelli/north/rules/branches/main --jq '[.[].type] | unique'` printed
`["deletion","non_fast_forward"]` on 2026-11-12. safe-push's lease pushes are fast-forwards, so landing passes the rule.
North-guard's command check and safe-push's arguments refuse force pushes on the client
(crates/north-guard/src/guard/tripwire/publish.rs, crates/north-safe-push/src/request.rs).

2026-11-09: claim 2, reject the patched binary (it would refuse safe-push's own lease pushes and binds only programs
that run it); adopt the server rule. Stands.

Where it is right and the fleet falls short: the principle reaches further than force-push. "Main changes only
through the lander" was phase 6 of the agent-user plan, which was merged into its paused epic on 2026-11-11 (south#94),
and the part of it that is still enforced only on the client is filed privately with its reading (south#119). Its
measurement is the hazard benchmark's history-rewrite class (south#114): what a worker told to rewrite or bypass main
gets onto a scratch origin carrying the same rules, in five runs.

## Claim 6: patch the planet

Strongest version: when an advisory lands, the fleet should learn within hours whether it is affected, patch at
once wherever the vulnerable code sits, transitively, and remove the patch when upstream ships.

What exists: `north supply-chain-scan` runs daily on a timer (firn's modules/supply-chain-scan) and runs osv-scanner
over the lock files on origin/main of four projects, alerting on malware and on advisories with a fix
(crates/north-timers/src/timers/supply_chain.rs). The lander refuses an advisory a lane adds unless a
`Dependency-Advisory:` trailer accepts it. firn applies one overlay set to every host.

2026-11-09: claim 5, an overlay reaches only what one nixpkgs instance builds; crates and npm packages are patched in
their lock through the dependency gate. Stands.

Where it falls short, each checkable in supply_chain.rs:

- An advisory alerts once: `seen.json` keeps every key ever reported, so a fixable advisory nobody acts on is never
  reported again and its fix is never recorded. north#1224 (rustix 1.1.4, RUSTSEC-2026-0486, fixed in 1.1.6) is the
  first case.
- The lock-file pattern names no `flake.lock`, and no host's system closure is scanned, so the talk's OpenSSL case
  would go unreported.

## Claim 7: a self-expiring note in AGENTS.md instead of a ticket

Strongest version: a temporary patch must carry its own removal, or it outlives its reason.

What exists: the holdlist's missing guards carry `accepted_risk` with a date ([holdlist.md](holdlist.md)); firn's
forced-command ssh keys carry an `expiry-time` the server enforces; issues with Done-when boxes are the fleet's
tracker, and the lander ticks a box from a commit's `Item:` and `Box:` lines.

Where it is right: the expiry. A `Dependency-Advisory:` acceptance has none, so an accepted advisory stays on main
with nothing to say when to look again, and no check reads the holdlist's dates.

Where it does not apply: the place. A prose note in AGENTS.md is acted on by whichever agent reads it, and its date
fails nothing. The fleet's form is a dated field a check reads: `Dependency-Advisory: ID until DATE — reason`, and a
concern ([concerns.toml](../../concerns.toml)) that goes red after the date while the lock still holds the version
(north#1243).

## Claim 8: sudo only on disposable machines, never production, outbound locked down

Strongest version: give an agent full power only where its worst act costs nothing, keep it away from anything that
holds real state, and stop data leaving.

What exists: no agent session holds passwordless sudo, and a sandboxed worker command runs under `no_new_privs`
(north#956, [worker-sandbox.md](worker-sandbox.md)); rebuilds go through `firn rebuild --landed`. depbox's run mode cuts
the network for dependency code (`--unshare-net` in crates/north-capacity-host/src/capacity/depbox.rs). Evals and
`verify-north` run on scratch repositories, the fleet's form of copying the data.

2026-11-09: claim 4, sudo on generations that roll back, rejected; claim 3, NixOS VM tests, rejected for now. Both
stand: nothing tonight was a runtime network fault a VM test would have caught.

Where it falls short, and what follows: the fleet's findings under this claim are machine posture, so they are
in south's record with their measurement (south#120). The public method is the talk's own order: measure what agents
reach before drawing an allowlist, since Landlock's network rights filter by port only
([landlock-net](../references.md#landlock-net)) and any host allowlist needs a filter or a proxy of its own.

## Claim 9: deterministic checks, not skills, and property tests

Strongest version: a loop must not close on bad code, so every rule that can be a check is one, and tests should
search for the cases nobody thought of.

What exists: the land checks in `.safe-push` refuse a warning (clippy `-D warnings`), failing tests, a broken layer
rule (`code-health layers`), a lifecycle that breaks its promises in any ordering (`north check machines`, which drives
each lifecycle table through the real store over every ordering of its scope, crates/north-machine-model), and a
removed hold guard (`holdlist-guards-resolve`). proptest is a dev-dependency of ten crates, with property tests in six.

Where it is right: checks beat skills, and the fleet's rule already says so. Where it is wrong for this fleet:
"checks like these replace much of the need for code review". Tonight five faults passed every check and were found by reading. A
check can hold a class of fault only once its instances are listed; the list is south#121. Property tests: the lifecycle kernel already has the stronger exhaustive check; the land queue's ordering
has three example tests and no property (north#1245).

Where it does not apply: Bombadil tests web applications, and north ships none.

## Actions, ranked by risk removed over cost

Each is an experiment with one measurement, and kept only if its measurement says so.

1. A private finding of 2026-11-12 on the worker sandbox (south#118). Removes a capability. Measurement: a test
   inside the domain is refused, and the fleet verbs that need another host still work through the broker. Size: one rule list and one broker path. Slower: those fleet verbs by
   one broker round trip.
2. Main moves only through the lander, enforced by GitHub (south#119). Removes a capability. Measurement: the hazard
   benchmark's history-rewrite class, five runs, nothing reaches the scratch origin. Size: one ruleset per
   repository. Slower: nothing; the lander already pushes as its own App.
3. Every unconfined reader of worker-written text is listed, then checked (south#121). Turns a review finding into a
   deterministic check. Measurement: the share of hand-listed readers a scan finds from code alone; built only at
   four in five. Size: one inventory, one land scan in shadow. Slower: one scan on boundary lanes.
4. Promotion waits for an independent read of each trust-boundary commit (north#1244). Measurement: one week of
   faults found per read, usage per read, and landing-to-live p90 for boundary commits; kept at one fault or more
   with under 30 minutes added. Size: about 88 commits a day. Slower: boundary commits reach live later by the read
   when it misses the slot's lead; no landing waits.
5. A fixable advisory stays open until its lock moves, flake inputs and a host closure are scanned, and every
   accepted advisory expires (north#1243). Measurement: hours from first report to fix landed over the next five
   advisories, north#1224 first; kept at a median under 24 h. Size: changes to one timer and one gate. Slower:
   nothing.
6. One day's measurement of where worker commands connect, then each enforcement priced (south#120). Measurement
   only. Size: one day on two hosts. Slower: nothing until enforcement, which is a decision of its own.
7. One property test of the land queue (north#1245). Measurement: mutants killed beyond the three example tests;
   kept at two or more under one second. Size: one test. Slower: under a second of test time.
8. North's dependencies counted by what each brings in (north#1246). Measurement: crates brought in alone and call
   sites per direct dependency; replacements only where the count says so. Size: one table. Slower: nothing.

## The six candidate actions of north#1241, judged

- Every commit independently read before promotion, with a record the promotion checks: changed to the commits that
  touch a trust-boundary path, since all five of tonight's faults sat there and those are a fifth of the week's
  landings, and kept only on its week's measurement (north#1244).
- The worker sandbox's git cannot rewrite pushed history or push except through the lander, proven by the hazard
  benchmark's history-rewrite class: changed to a server rule, since the sandbox binds only the programs a worker runs
  while GitHub binds every client; the rewrite half is already server-side (north#973), the rest is south#119.
- A day's measurement of where worker sandboxes connect, and an outbound allowlist drawn from it: adopted as the
  measurement, with enforcement priced and left to its own decision, since every way to enforce a host allowlist adds
  a filter, a proxy or a tool (south#120).
- Property tests on the lifecycle kernel and the land queue in place of example tests: changed; dropped for the
  lifecycle kernel, which the exhaustive machine check already covers, and tried for the land queue (north#1245).
- A scheduled advisory scan of the Cargo lock and the system closure that opens a patch lane with a dated note to
  remove the patch: changed; the scan exists, so it is extended to keep advisories open, read flake inputs and a host
  closure, and expire acceptances. The issue it files is staffed by dispatch like any box, rather than the scan
  opening lanes itself (north#1243).
- A count of north's dependencies by use, with the ones a few lines could replace: adopted as a count, with
  replacement only where it shows a gain (north#1246).
- Added: south#118, a private sandbox finding already filed, and south#121, which lists then checks every
  unconfined reader of worker-written text.

No action costs money, creates an account, widens what an agent can reach or exposes a credential.

## Tools named and not adopted

None is adopted here; each would need an external-code review (license, maintenance, pin, what it reaches at build
and run time, why it beats what exists) before any use.

- Hegel (Antithesis), property testing across languages: proptest already serves north; review only if north#1245
  finds proptest short.
- Bombadil (Antithesis), property testing of web applications: north has none.
- devenv: rejected on 2026-11-09.
- A Nix closure vulnerability scanner: chosen in north#1243 after its measurement, or osv-scanner alone if it suffices.

## Sources

The talk summary ([ghuntley-security-talk](../references.md#ghuntley-security-talk)) and the earlier review's sources
([nix-leverage-sources](../references.md#nix-leverage-sources)).
