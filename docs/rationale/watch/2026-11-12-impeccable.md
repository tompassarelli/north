# Impeccable, second reading (2026-11-12)

Record for [Impeccable](../../references.md#impeccable) (Paul Bakaus, Apache-2.0), one watching cycle from
`c3bb418` (read 2026-11-06, [screen](2026-11-06-screen.md)) to `b4bff16` (2026-11-11). It informs screen and design
work for Mak'gora and Wisp, not the harness core. Nothing was built or run: this is a reading of the diff, in a
fresh clone treated as data. The box on north#1012 said 106 commits at head `f2baa414`; the source moved on, and
the range read here is 195 commits, 143 of them non-merge (116 by the maintainer, 14 by the sync bot, 13 by other authors).

License at `b4bff16`: `LICENSE` is Apache-2.0, `NOTICE.md` is unchanged and still credits the MIT-derived
platform references (`skill/reference/ios.md`, `android.md`). `crates/account` declares `license.workspace`
(Apache-2.0). The paid rule pack is linked in from outside the repository (`docs/ENGINE.md:152`), so none of
it is in the tree. Copying the open Rust rules stays allowed with the notice, a change statement and `NOTICE.md`;
nothing was copied.

## Where the 143 commits went

Counted by the paths each commit touches (a commit can touch several):

- Account and cloud, 27 commits in `crates/account`: `impeccable login`, `logout`, `whoami`; opt-in critique
  history upload; `DESIGN.md` viewer sync; design-health counts after each full `detect`; stale-sign-in handling;
  proxy and `NO_PROXY` handling. About 8,950 changed lines. This is the service's customer path, off by default
  and not something a design check for our pages needs.
- Agent installer engine, 16 commits in `crates/agents`: owner-scoped hooks, MCP entries and rule blocks written into
  Codex and other harness configs, with a journal, undo and drift reports (`impeccable agents list`). Install
  plumbing for many harnesses.
- Engine releases and CI, 29 commits in `.github` and 36 touching `tests`: engine 0.1.19 to 0.1.22, checksum
  records, Windows launcher fixes, runner changes. Test and release hygiene, with 14 sync-bot commits regenerating
  per-harness copies (about 20 directories of near-identical output).
- Skill text, 8 commits in `skill/reference`, read below.
- Hook, 5 commits in `crates/hook`: Stop pass now also scans UI files modified since the session started.
- Detection rules: `crates/core` and `crates/detect/src/regex_matchers.rs` have 0 commits in the range, so the 62 rules
  we screened in are byte-identical to what was read on 2026-11-06. `crates/detect` has 2 commits, `cli.rs` and `lib.rs`:
  composable rule packs (a closed pack can add rules to the open engine) and a `FullScan` result the design-health
  upload consumes.

## What the behaviour changes are

- `skill/reference/critique.md`: project personas now come from PRODUCT.md `## Users` and `## Brand Commitments`,
  not the old `## Design Context` block. Same mechanism, tied more firmly to the per-project brief.
- `skill/reference/audit.md`: adds RTL checks scoped to pages that declare `dir="rtl"`, each phrased as a signal to
  verify the rendering, and a "Save the Report" step that writes the audit through `impeccable audit-storage write`
  (`Feat: audit saves its report`).
- `skill/reference/new-work.md`: no visual direction may be proposed before the seeded direction roll (`concept-seed`),
  and a pinned direction records `seed: none (pinned)`. This extends their anti-slop approach from rules after the fact
  to randomness before the first draft.
- `skill/reference/onboard.md`: localStorage keys namespaced by account and version; empty states scoped by type.
- `crates/hook/src/stop_discovery.rs`: the author found that edit-tool hooks never see files written by a shell
  heredoc, which in about half of Codex runs left the Stop scan with nothing, and added a bounded walk (depth 8,
  5,000 entries, 40 files) keyed to the session start from a UUIDv7 session id or the transcript's birth time.

Reverted or flip-flopped in the range: the OpenAI plugin shipped a bundled hook and had it removed the same day
(`Fix: ship the OpenAI plugin without a bundled hook`); CI Windows jobs moved to Namespace runners and back to
`windows-latest`; a temporary Namespace image probe was added and removed. None of these touches the rules.

## Last cycle's mechanisms

The screen took nothing and filed no issue; its entry says "nothing taken yet", and that record has no "Next review:
start here" section, only "Full cycle now", which names ponytail and not this source. So there is no kept, reverted
or unfinished mechanism to report. A search of `makgora/main`, `wisp/main` and north's own tree finds no use of
impeccable beyond the follow list and these records.

## Ranking and decision

Measured basis for every line is the diff above (commit counts per path); effect on north's landing speed, quality
and usage is reasoned, labelled as such.

- The 62 slop rules as a client-UI check. Stable across 143 commits, so no re-reading is needed. Effect on north's own
  work: none, since north ships no rendered pages. Effect on Mak'gora or Wisp: reasoned only, and there is no
  consuming page under review yet. Dropped for now: below the bar until a UI is waiting on review.
- Critique, audit with P0 to P3 ranking and PRODUCT.md personas. The changes are small wording and an RTL check.
  Reasoned effect on north: none; on the pages: moderate when a page exists. Same drop.
- Pre-draft direction roll. A distinctive idea but it needs the whole `concept-seed` CLI; effect unmeasured and
  tied to their own benchmark. Dropped.
- Stop-pass discovery of shell-written files. Real for Codex, where edits arrive through `apply_patch`; north's
  sessions are Claude, whose Edit and Write tools reach post-edit hooks. Reasoned effect small. Dropped.
- Account, uploads, agents installer, releases: product plumbing for the service, about 70 of the 143 by path (counts overlap), with no
  effect on north. Dropped.

No mechanism clears the bar, so there are no experiments and no new issues. The slop rules remain the one copyable
asset (Apache-2.0, Rust, unchanged), and the right trigger to take them is a Mak'gora or Wisp page being built.

## Next review: start here

- Review again at the next due date set by `south:agents/watch.toml`, or sooner when a Mak'gora or Wisp page is built.
- First check whether `crates/core` or `crates/detect/src/regex_matchers.rs` changed since `b4bff16`; if the rules moved,
  re-read what moved, and if a page is waiting, run `impeccable detect` on it before copying anything.
- Check whether the open engine still builds without the closed pack, since composable packs make the account
  and Pro path the direction of travel for the source.
- The `hallmark` recheck (2026-12-21) asks whether its eval harness adds anything this source's does not. This cycle did not read
  impeccable's measurement path closely enough to answer; read `tests/oracle` then.
