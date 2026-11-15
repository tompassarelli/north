# Cycle of addyosmani/agent-skills, 2026-11-15

One cycle of [watching](../../../harness/skills/agent-policy/references/watching.md) for the source
reviewed at `301b02b` on 2026-11-06 (north#636, [screen](2026-11-06-screen.md)), run from the
detector box of north#1585. Nothing was copied: every file was read as data, and only ideas were
taken, if any.

## What we looked at

- **Source and range.** https://github.com/addyosmani/agent-skills, fetched on 2026-11-15 into a new
  empty directory and checked out at head `39b723ceebf81474d0557c1fecd01eddab585563`. The range
  `301b02b12bff98e4688e6984af6556802322bbde..39b723c` has 41 commits, 22 of them non-merge, authored
  2026-07-10 to 2026-11-14. Most of the 22 came in by merge in November.
- **License at that revision.** MIT, from `LICENSE` at `39b723c` (`Copyright (c) 2025 Addy Osmani`).
  Reused scope: none. This record and the [references entry](../../references.md#addyosmani-agent-skills)
  are the external-code record.
- **Read.** The commit messages and bodies of all 22 non-merge commits, `evals/README.md` in full,
  the ledger `evals/skill-impact.md`, and the file lists of `evals/cases/` (25 case files) and
  `evals/plugin/` (13 case directories).
- **Not read in depth.** The text diffs of the skills and of `scripts/run-evals.js`, the release and
  plugin-manifest bumps, and the CI workflow. Their commit subjects were read instead.

## Reversals and flip-flops in the range (step 2)

- **Documentation drift section.** Added 2026-08-25 in `7be23cf` with three mechanisms: a freshness
  stamp, an append-only change log and a code-to-document routing map. Cut on 2026-11-02 in `551c59c`
  to the two rules that change behaviour (the commit-order check and the change log), after review.
  The author undid half of its own addition, and kept the check that reports "cannot tell" on
  unordered history.
- **Review clause.** `5a52638` (2026-10-29) added to the review description a sentence saying the
  skill gives a verdict and a severity on every finding. The later measurement in `evals/README.md`
  scored the clause without that sentence at 12 of 12 as well, so the sentence was dropped. The
  author kept the trigger and removed the explanation.
- **Test-driven-development clause.** `41a16af` (2026-10-29) re-tuned the pasted-code clause for the
  2.1.312 router. A wording that repeated "fix" three times fired more often but cost a rank-1 route
  in the offline ranker, so it was not kept (`evals/README.md`).
- **Merge loss.** `53bd30d` (2026-11-13) restored a crash-and-rollback line that the merge `a3d5857`
  had dropped. That is an accident of merging, not the author's undo, and it is recorded as such.

## The screen's kept mechanisms: where they stand at `39b723c` (step 3)

The screen filed no issue for them, so there was no verdict box to read. Read from the tree:

1. **Fire and stay-quiet pairs.** Live and grown. `evals/cases/*.json` has 25 case files, each with
   positive and negative trigger prompts that name an owner for the negative. `evals/plugin/` has 13
   case directories for five skills (code review, debugging, git workflow, simplify, test-driven
   development), each a fires case or a stays-quiet case, graded on the reply as well as on the
   skill's load. Verdict: kept by the source, not reverted.
2. **Grader hygiene.** Live. `6852bc2` (2026-08-26) removed a code comment that restated the expected
   answer of a case and gave one fixture three entries that state why. `evals/README.md` says only
   completed executor streams reach the grader. Verdict: kept by the source.
3. **Rejected-change ledger, `evals/skill-impact.md`.** Created 2026-09-02 in `125b1f8`, and at `39b723c`
   it has no data rows (only its header and separator). In the same period the README records at
   least six rejected or narrowed wordings in prose. Verdict: unfinished at the source, past a verdict
   date that no one set. A finding, not a footnote: the ledger was not where the author wrote its
   rejections down.

## How we ranked the source's mechanisms (step 4)

Expected effect on landing speed, quality or usage per closed box.

| Mechanism | Basis | Decision |
|---|---|---|
| Fire and stay-quiet pairs measured through the agent's router, with the pinned model and CLI version | Measured in north: a search of the tree for stays-quiet, fires-on, should-fire, trigger-rate and skill-fire measurements finds none for the 22 skills under `harness/skills/`. `crates/north-flow/src/detectors/skill_review.rs` reads file changes, corrections, tool-version moves and cited-source drift, and never whether a skill loads. Measured at the source (claude-opus-5-5 pinned, three to six runs per cell): review requests with the diff pasted fired the skill 7 of 27 times, and one clause naming the inline case took them to 21 of 27 with nine neighbouring requests at 0 of 27; test-driven-development went from 6 of 24 to 19 of 24 (`evals/README.md`). Reasoned for north: a skill that does not load contributes none of its rules. North's own rate is not yet measured. | **Keep.** Becomes the box below. |
| Rejected-change ledger | Measured at the source: 0 rows after 2.5 months, while the README holds at least six rejected wordings. Measured in north: 7 commits with a Revert subject touch `harness/skills` or `agent-machinery`, and every one reverts a code change or a lander revert of a red main (`e67844c8b6`, `4bb4329edd`, `6490ace24c`, `0483729409`, `ea1f433b25`, `0c7d738861`, `e9613370d5`); none reverts a skill wording on eval evidence. The git history already holds the rejected attempts in searchable form. | **Drop.** Its purpose is served by git history, and no north rejection exists to record. |
| Grader hygiene: no answer-leaking fixtures, only completed executor streams graded | Reasoned, no north basis found: north's behaviour evals of harness text fork real transcripts in policy replay rather than grading fixtures (`docs/rationale/watch/2026-11-12-t3code.md`), so there is no fixture for a skill's answer to leak into. | **Drop** until a fixture-based eval of skill text exists. |
| Clause-level description tuning | This is the lever the first mechanism measures, and north's `harness/skills/agent-policy/references/guides/agentskills-optimizing-descriptions.md` already says wording matters for queries where the connection is not obvious (lines 52 and 19). | **Convergent**, folded into the first mechanism. |
| Git-ancestry documentation staleness, reporting "cannot tell" on unordered history | North took a docs-vs-tree check from pstack on 2026-10-27, and the source itself cut its freshness stamps and routing map to these two rules. | **Drop**, convergent. |
| A gate that exits 0 when its command is malformed (`depcruise --validate src`) | North already has the able-to-fail rule from pstack (2026-10-27): `README.md` and `docs/references.md` carry it. | **Drop**, convergent. |
| Say in the report what could not be run (frontend skill) | Conflicts with the owner's policy that a report never ends on a caveat line. | **Drop.** |
| Delta specs (`SPEC-<change>.md`) and plan source-spec stamps (`cee618c`, `5971d80`, `480bed1`) | North's unit of work is the box on an issue (`harness/skills/workers`), not a spec file or a reused plan, so the stale-plan hazard those guards answer does not arise. | **Drop.** |
| Product fixes in the source's own code: the session-secret startup check, the OWASP 2025 renumbering, the heading-slug fix, the hook matrix over bash versions | Fixes to the source's own code, with no mechanism north lacks. | **Drop.** |

## What we decided

- **One mechanism kept**: the fire and stay-quiet pairs, measured through the agent's router. Its
  experiment is a box on north#1585, not a run in this cycle. The box costs more than the 30-minute
  limit: 22 skills, two prompts each, three runs each, 132 pinned router runs. Its keep or revert rule
  is fixed in the box: keep the pairs on main if any should-fire prompt loads its skill in fewer than
  2 of 3 runs, or any stay-quiet prompt loads a skill in 2 of 3; otherwise revert them and record the
  zero.
- **Design of the frozen set.** One should-fire prompt per skill that it owns, one stay-quiet prompt
  drawn from a neighbouring skill, both with the input pasted inline, committed before the first run.
  Claude Code version and model are recorded with every number, because the source measured the same
  clause at 6 of 6 on Claude Code 2.1.283 and at 1 of 12 on 2.1.312 (`evals/README.md`).
- **No decision for the owner here.** Nothing in this cycle is a cost, account, public, or direction call.

## Next review: start here

- The source has no cadence in `south:agents/watch.toml`, so it is due 30 days after its
  `last_reviewed` of 2026-11-15, that is 2026-12-15, once `git log --no-merges 39b723c..HEAD` has any
  commits, or sooner at 10 of them.
- Check `evals/skill-impact.md` at the next head. If it still has no data rows, say so in the next
  record: the author has not used it.
- Check the verdict on the fire and stay-quiet box of north#1585, with
  `rg -n 'Verdict: (keep|revert)' /home/tom/code/north/main/docs/rationale/watch/2026-11-15-addyosmani-agent-skills.md`.
  A keep means the source's clause pattern held on north's skills, and a revert means north's
  skills already load on pasted inputs, which is also a finding.
