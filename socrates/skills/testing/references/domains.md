# Testing: domain profiles

Read when a project picks or revisits its test posture. The five kinds and the
burden rule hold everywhere; what changes per domain is which kind carries the
weight, what a failure costs, and which suites to measure against. Apply what
fits the project; copy no reference's habits whose cost its failures don't
justify. Tags name entries in `sources.md`.

## Ledgers, databases, consensus (money or data loss)
- Failure cost: permanent and silent. Correctness outranks speed of change.
- Weight: a deterministic simulator with injected faults (clock, network,
  disk) checking invariants at every step, e.g. conservation of money and
  linearizable histories [E2][E3]; seeded, replayable from seed plus commit.
- Exhaustive coverage tiers are justified here and only here [E1].
- Reference: TigerBeetle [E2], FoundationDB [E3], SQLite as the coverage end
  [E1].

## Deterministic games and simulations (Smashcraft, Wisp)
- Failure cost: a broken match or desync is visible and annoying, rarely
  permanent; feel and readability matter as much as correctness.
- Weight: seeded headless matches as the simulator [E2], with invariants per
  frame (no NaN, state hash agreement between Bun and 32-bit Lua, frame and
  allocation budgets in deterministic units); recorded tapes for every desync
  [E1][P9][P10]; same seed twice must give the same hash [E3].
- Content as data properties: every move has a visible effect, despawns, is
  mirror-symmetric, and the body drives it, checked over the whole roster,
  not per move by hand.
- Look and feel: native capture judged by a cold viewer and by Tom; never ask
  a human to confirm what a property can.
- Few hand-written unit tests; no per-move example matrices.
- Reference: Factorio [P9], GGPO [P10], Lua's own suite for the Lua side [E8];
  measured: Lua 5.4.7 is 0.68 test lines per code line in 33 scripts.

## Command-line and developer tools (north, firn, agent tooling)
- Failure cost: a broken workflow, quickly seen and quickly fixed.
- Weight: many small expected-output tests, each one behaviour against its
  exact output [E4][E5][E9]; one integration check per real boundary (git,
  GitHub, systemd) [G1]; pure cores with properties for parsing and planning.
- Reference: ripgrep [E9] (measured 22 tests per KLOC, about 7 test lines
  each), Jane Street expect tests [E4], esbuild [E5].

## Language implementations (Galileo)
- Failure cost: wrong programs accepted or wrong output, often silent.
- Weight: test programs written in the language itself with their expected
  output [E8][E7]; differential runs against a reference implementation;
  properties and fuzzing over the parser and evaluator; expect tests for
  diagnostics [E7].
- Reference: Lua 5.4 `testes/` [E8], rust-analyzer [E7], Go [E6].

## Desktop and UI apps (smashcraft-client)
- Failure cost: a confusing or broken screen; the user sees it at once.
- Weight: the page logic as a pure state core with properties; one end-to-end
  smoke per platform; screenshots judged by a viewer for layout; render from
  the same data the backend acts on, never a copy.
- No fakes of our own backend API [G3]; no snapshot of markup structure.

## Infrastructure configuration (nixos-config, south)
- Failure cost: a machine that won't boot or a leaked secret.
- Weight: evaluation and build of every host in CI; policy assertions in the
  module system rather than tests; one integration check per secret or
  service boundary; a secret scan.

## Measuring against a reference
Use the same counter for both sides (the runner's own listing, `scc` for
lines). Record production and test lines, tests per KLOC, lines per test and
CI wall; a gap of 2x or more needs a written reason. Smells to look for when a
number is off: the van Deursen catalogue [P11] and its later studies [P12].
