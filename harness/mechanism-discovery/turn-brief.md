# Mechanism-discovery turn

Tier: worker
Surface: docs
Needs: local-state

Goal: one turn of north's question-taking loop (rung three of `docs/rationale/self-improvement-loop.md`). A turn takes the largest measured loss that no open issue owns, states two explanations whose predictions differ, separates them on recorded cases, and writes the result to the turn log. A turn decides and records. It builds nothing.

The detector files this turn once a UTC day as an issue of its own, titled "Run the mechanism-discovery turn", holding one box that names the loss key it was filed for; the turn's landing ticks that box and closes the issue. Take that key first. The same procedure runs by hand.

Method.

1. Pick the question. Read the losses the box lists, in order: `north dash losses --since 7d` (first tries lost to the harness, by class) and the `north dash --weekly` rows that carry a k/n figure in both weeks, counted as n times the move for the worse. Skip a key that a `[[turn]]` of the last 28 days in `docs/rationale/discovery-turns.toml` already holds. For the first key left, check whether an open issue owns it: `north work search TEXT`, or `gh issue list -R tompassarelli/north --state open --search TEXT` when that command cannot read its store. An owned loss is recorded with `owned` and the turn moves to the next key. An unowned one is the question.
2. State two explanations, A and B, that predict different things. Write each prediction as what the separating command should print under A and under B before running it.
3. Separate them with the sources this turn may read, and nothing else: the decision log (`north ops strategy replay --since 24h`), the ledger's run rows (`north ledger runs --json`, counted with the scoreboard's worker rule, `ROLE_SQL` in `crates/north-base/src/platform/run_roles.rs`), git history on origin/main, and the lander's records (`north ledger flow --json`). Until graded outcomes (rung zero of `docs/rationale/self-improvement-loop.md`) hold, these are all a turn reads. Replay first. A bounded live arm is allowed only for a survivor that replay does not settle, and it is named in the turn with its size before it runs.
4. Record the turn. Append one `[[turn]]` to `docs/rationale/discovery-turns.toml` with these keys: `date`, `loss` (the key), `figure` (the number the loss was read at), `explanation_a`, `explanation_b`, `separating` (the command or query), `survivor` (A, B, or `none`, with what it shows), `owned` (the open issue that owns the loss, or `none`), `mechanism` (the issue filed for the mechanism, or `none`) and `registry` (the `discovery-` entry id, or `none`).
5. Register a survivor that has a mechanism. Add `discovery-<date>-<key>` to `docs/rationale/experiments.toml` with a bar measured on a class or repository the mechanism was not built from, and a review day. A survivor with no mechanism to test is recorded in the turn only, with `registry = "none"`.
6. Mechanism code is never written in the turn's lane. It is filed as a box on its own issue, and the turn names that issue. Ideas the turn cannot fit are one line each in its report, naming the open issue they belong to.

Edit nothing else. The turn's lane changes `docs/rationale/discovery-turns.toml`, adds lines to `docs/rationale/experiments.toml`, and nothing more. No replay, checker, ledger count, bar or review day moves, and no gate or holdlist entry changes. The `turn-scope` land scan refuses a lane that does otherwise.

Stop rule. The loop is kept or deleted on 2026-12-17, by the check its experiment entry runs on that day. It is kept when, in the 28 days before, it finished at least 20 turns and at least one mechanism it registered was kept on a class or repository it was not built from. A miss reopens north#1496, the issue that carries the check's box, whose work is to delete the detector, this brief, the scan and the loop entry, and to keep the turn log as the record.

Done when: this turn's `[[turn]]` is on origin/main, so `git show origin/main:docs/rationale/discovery-turns.toml | rg -c '^date = '` prints a count that includes the turn's date, and the turn's lane changed no file other than the two named in step 4 and step 5.

Not required: fixing the loss in this turn, restoring the climber, reopening a closed issue, a second fact store, and any paid or hosted-provider call.
