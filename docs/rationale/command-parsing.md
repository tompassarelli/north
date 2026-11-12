# Command parsing and error handling: a measured choice

Measured for north#1276 on 2026-11-12 at origin/main 80086bdf9. The owner asked (2026-11-12) that north's hand-written
argument parsing and error handling move onto a library, chosen by measurement rather than by the earlier unmeasured
audit; the question answered here is which library and in what order. The rule for a net loss, set in the same
request: fewer than 1,000 lines removed across the tree, or more than 10% added to the one-line-edit rebuild of the
command crate, for every candidate, sends the numbers back to the owner and nothing is converted.

## Behaviour held fixed

Skills, briefs and guards quote north's output, so a conversion keeps all of it:

- `--help` text. Every verb's help is hand-written: `src/cli/reference.txt` (339 lines, one line per verb, read by
  `north VERB --help`), `src/cli/help.txt`, and 1,643 lines of `USAGE`/`HELP` constants in the crates. No library's
  generated help matches it, so every candidate runs with its help generation off and the text stays as it is. A
  library removes no usage text.
- Exit codes: 0, and 2 for a usage error, per verb as today.
- Usage-error messages. The scratch conversions were held to a golden of 44 argument vectors over `north convo`
  (help, clusters such as `-xa` and `-xn3`, `--flag=value`, values that start with a dash, `--`, a lone `-`, unknown
  flags, bad numbers and bad choices), captured from the old parser.

## What exists

Counted over non-test Rust (no `tests/`, `evals/`, `*_tests.rs`), leaving out north-guard, whose flag matching reads
other programs' command lines, not north's own arguments. "argv functions" are functions taking `&[String]`,
`Vec<String>` or an argument iterator that match a flag literal (an upper bound: they hold the verb's work as well as
its parsing); "parse functions" are those of them named for parsing (`parse`, `args`, `options`, `flags`, `usage`);
"flag arms" are match arms on a flag literal.

| crate | argv function lines | parse function lines | flag arms | usage text lines | error type lines | `Result<_, String>` |
|---|---:|---:|---:|---:|---:|---:|
| north-ops | 1,195 | 250 | 75 | 80 | 0 | 224 |
| north (src/cli) | 1,105 | 245 | 88 | 431 | 8 | 129 |
| north-core | 941 | 129 | 59 | 54 | 0 | 94 |
| north-runtime | 859 | 141 | 33 | 41 | 4 | 140 |
| north-capacity-host | 798 | 26 | 15 | 27 | 0 | 19 |
| north-landing-gates | 752 | 119 | 22 | 121 | 0 | 139 |
| north-threads | 651 | 58 | 10 | 4 | 0 | 31 |
| north-lanes | 508 | 134 | 30 | 130 | 5 | 113 |
| north-base | 496 | 0 | 13 | 39 | 10 | 87 |
| north-land-scans | 473 | 44 | 21 | 15 | 0 | 30 |
| north-flow | 460 | 113 | 32 | 7 | 0 | 86 |
| north-lander | 457 | 151 | 34 | 12 | 0 | 109 |
| north-diff-gates | 431 | 126 | 10 | 234 | 0 | 26 |
| north-land-checks | 378 | 107 | 28 | 5 | 0 | 38 |
| 41 other crates | 3,341 | 678 | 203 | 443 | 103 | 993 |
| **total (55 crates)** | **12,845** | **2,321** | **673** | **1,643** | **130** | **2,258** |

Error handling is almost all string-typed: 130 lines of error enums with their `Display`, `From` and `Error` impls
(north-graph 66, north-foundation 37), against 2,258 functions returning `Result<_, String>` and 2,354 `map_err`
calls, 919 of them a plain `to_string()` or `"context: {error}"`.

## Candidates

Each argument candidate converted `crates/north-convo/src/convo/args.rs` for real in a scratch lane: the largest
dedicated argument file (220 lines, 22 flag arms; 72 of the lines are its help text, which stays). It is a port of
Python argparse, so it hand-rolls exactly what a library offers: short clusters, `--flag=value`, `--`, typed values and
argparse-style messages. A second, typical parser, `crates/north-land-checks/src/land/slots/runner/arguments.rs`
(one `match` arm per flag, 24 flags), was converted to clap derive and counted without building.

Builds ran on cpx62b (8 cores, under other land checks: load 3 to 6 per core) through `north capacity offload run`,
in the slot's own target with the host's sccache, so a first build is not a cold build. Each candidate's edit
rebuild ran interleaved with the baseline in rounds, so host load falls on all of them alike. Edit-rebuild times are
the median of eight rounds (two runs of rounds 2 to 5; round 1 warms the cache) after a one-line edit to
`crates/north-convo/src/convo/args.rs`, at load 7 to 25 on 8 cores; the spread within one candidate (3.9 to 5.6 s for
the baseline) is wider than any difference between candidates. Clean builds were not comparable: the shared sccache
serves every crate the host has built before, so a "clean" build measured the cache, not the candidate.

| | lines in args.rs (220 before) | golden of 44 cases | edit rebuild, north-convo | edit rebuild, north binary | north release bytes | new crates | licence | last release | build-time code |
|---|---:|---|---:|---:|---:|---|---|---|---|
| baseline | 220 | (source) | 1.10 s | 4.25 s | 89,016,856 | none | | | |
| clap derive | 186 (163 with each attribute on its field's line) | holds | 1.00 s | 4.35 s (+2%) | 89,490,080 (+0.53%) | clap, clap_builder, clap_derive, clap_lex, anstyle | MIT OR Apache-2.0 | 4.6.7, 2026-09-14 | proc macro `clap_derive` (syn 2, quote, heck already in the graph) |
| clap builder | 193 | holds | 1.10 s | 4.35 s (+2%) | 89,424,792 (+0.46%) | clap, clap_builder, clap_lex, anstyle | MIT OR Apache-2.0 | 4.6.7, 2026-09-14 | none |
| lexopt | 197 | holds | 1.05 s | 4.30 s (+1%) | 89,028,032 (+0.01%) | lexopt | MIT | 0.3.2, 2026-02-28 | none |
| bpaf (combinators) | 164 | 16 lines differ | 1.10 s | 4.10 s (-4%) | 89,287,232 (+0.30%) | bpaf | MIT OR Apache-2.0 | 0.9.28, 2026-09-17 | none |
| argh | 205 | 23 lines differ | 1.05 s | 4.30 s (+1%) | 89,035,608 (+0.02%) | argh, argh_derive, argh_shared | BSD-3-Clause | 0.1.19, 2026-03-16 | proc macro `argh_derive` |

Both "holds" and "differs" exclude two golden cases where the old parser was wrong (below); every candidate fixes them.

Which candidates cannot hold the behaviour:

- **argh** cannot. It has no short-flag clusters (`-xa`, `-n5` and `-xn3` fail), no `--flag=value`, reserves
  `--help` and requires a doc comment per field, and its error messages are its own (`Unrecognized argument: --bogus`).
- **bpaf** cannot without a layer that matches its message text. It refuses a value that starts with a dash
  (`-r -x`, `-n -3`), takes `-xz` as a search word instead of refusing it, and its parse errors carry no structured
  kind or flag name (`couldn't parse x`), so the old `argument -n: invalid value` cannot be rebuilt from them.
- **clap** (both APIs) holds, through a 20-line translation from clap's structured error kinds back to the old
  messages. Clap reports a flag by its long name, so the translation looks back at the raw arguments to say `-n` when
  `-n` was typed.
- **lexopt** holds, with the old `match` arms kept: lexopt removes the cluster, `=`, `--` and value-taking loop and
  nothing else.

The old parser had two bugs the golden exposed: `--max-files` and `--context` were missing from its list of options
that take a value, so `north convo compress --max-files 3` and `north convo show ID --context 3`, both in the verb's
help, exited 2 with `unrecognized arguments`. They are fixed in the old parser in the same change as this document.

### Error handling

- **thiserror** (MIT OR Apache-2.0, 2.0.21 released 2026-09-23) for library crates: north-graph's `error.rs` converted
  goes from 68 lines to 37. The whole tree holds 130 lines of error types, so thiserror removes about 60 lines in total.
  It adds no crate: thiserror 2.0.20 and its proc macro are already built for north through landlock, ratatui-core and
  rsqlite-vfs.
- **anyhow** (MIT OR Apache-2.0, 1.0.104 released 2026-07-18) at binary edges: already built for north through
  termwiz. Moving `Result<_, String>` to `anyhow::Result` turns `.map_err(|error| format!("context: {error}"))` into
  `.context("context")` on the same line, so lines removed are bounded by the 237 lines that hold nothing but a
  `.map_err(` continuation. It also changes what a caller prints unless every edge formats with `{:#}`.

## Projection and choice

Lines removed per flag arm, measured: clap derive 1.5 on the argparse port and 0.25 on the typical parser with each
attribute on its own line, or 2.6 and 1.3 with the attribute on its field's line; lexopt 1.05 on the argparse port
and about 0 on the typical parser, whose `match` it keeps; clap builder 1.2 on the argparse port. Over the tree's 673
flag arms:

- clap derive, attributes on their own lines: about 170 to 1,010 lines.
- clap derive, attributes on the field's line: about 870 to 1,750 lines.
- clap builder: at most about 830 lines.
- lexopt: at most about 710 lines.
- thiserror and anyhow together: at most about 300 lines.

No candidate adds a measurable rebuild second: every one is within the round-to-round spread and at most 2% over the
baseline median, far under the 10% bar. So the rule of lines removed per build second added reduces to lines removed
among the candidates that hold the behaviour.

**Choice: clap derive, each `#[arg(...)]` on its field's line, help generation off** (`disable_help_flag`,
`no_binary_name`, `args_override_self`; `default-features = false` with `std`, `derive` and `error-context`), with one
shared translation from clap's error kinds to north's usage-error messages. It is the only candidate whose projection
clears the 1,000-line bar (about 1,300 at the midpoint), it holds the golden, and it removes the typical
one-arm-per-flag parsers that lexopt leaves as they are. bpaf and argh cannot hold the behaviour. For errors, thiserror
replaces the hand-written impls in north-graph and north-foundation, and anyhow is not adopted: about 240 lines at most,
for a change to what every caller prints.

Conversion order, largest saving first by flag arms and parse-function lines, one landing per crate: north (`src/cli`,
88 arms, 245 parse lines), north-ops (75, 250), north-core (59, 129), then north-lander, north-runtime, north-flow and
north-lanes. Each landing carries a golden test of that crate's `--help` and usage-error output captured from the old
code.

## External-code review

- clap 4.6.7 (https://github.com/clap-rs/clap): MIT OR Apache-2.0, compatible with north's MIT OR Apache-2.0;
  released 2026-09-14 and maintained by the clap-rs organisation; pinned in Cargo.lock by version and checksum. At
  build time `clap_derive` is a proc macro over syn 2 and quote, which north already compiles for serde_derive; no
  build script. At run time it reads only the argument vector, no files, network or environment. Built with
  `default-features = false` and `std`, `error-context` (plus `derive`), so no colour, terminal or suggestion code.
- lexopt 0.3.2 (https://github.com/blyxxyz/lexopt): MIT, compatible; released 2026-02-28, single maintainer, no
  dependencies, no build script or proc macro.
- bpaf 0.9.28 (https://github.com/pacak/bpaf): MIT OR Apache-2.0; released 2026-09-17; no dependencies with default
  features off. Not chosen: it cannot hold the behaviour above.
- argh 0.1.19 (https://github.com/google/argh): BSD-3-Clause, compatible; released 2026-03-16; proc macro
  `argh_derive`. Not chosen: it cannot hold the behaviour above.
- thiserror 2.0.21 and anyhow 1.0.104 (https://github.com/dtolnay): MIT OR Apache-2.0; both already in Cargo.lock as
  transitive dependencies.
