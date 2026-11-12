# Close-by-rule: the share of coordinator turns a report alone triggers

Measured 2026-11-12 for north#1266 (idea 3 of the tenets examination, north#1248), on north origin/main `d8d65b1ab7`.

Verdict: **dropped.** Over the frozen day 2026-11-11 (UTC) the coordinator ran 499 turns, and 69 of them (13.8%) were triggered only by a `Done:` report or a landing notice. The rule fixed in the issue ends the experiment as dropped below 25%. Close-by-rule is not built and no build issue is filed.

## Measurement

Input: the coordinator's two Claude transcripts that hold turns started in that UTC day (the proxy window before and after its adoption on 2026-11-11), 74 and 425 turns. A turn is one prompt cycle, a distinct `promptId` whose first entry is a delivered message and not a tool result; its trigger is that first message, a peer message, a service alarm, a background-task notification or the owner's own text. Messages that arrive while a turn runs are injected into it and trigger nothing.

Kinds of trigger, one per turn, by the sender and the body's first matching rule: owner message (no sender), alarm (sender is a service), revert (body mentions a revert), completion report (body holds `Done:`), landing notice (body holds `landed` and a commit hash), other report (any other peer message), other (background-task notification).

- Other report: 189
- Other (background-task notification): 89
- Alarm: 70
- Owner message: 56
- Landing notice: 48
- Revert: 26
- Completion report: 21
- Gate answer: 0

Turns triggered only by completion reports and landing notices: 21 + 48 = 69 of 499, 13.8%. A looser reading that counts any message holding `Done:` or `landed` beside a hash, including the 8 revert turns that also name one and the relays that carry other findings, gives 121 of 499, 24.2%. Both are under the 25% stop bar, and the join (box 2 of the issue) was not run because box 1 ends the experiment.

## Limits

- Transcript turns are the only per-turn record. The ledger holds one coordinator run for the day, because the proxy is an adopted interactive window and not a headless run, so north#1235's turns-per-landing count and the ledger run count cannot cross-check this total.
- From 01:41 to 09:10 UTC the coordinator was a different provider and left no Claude transcript; its turns are not in the 499.
- Commit-reviewer sessions relay events to the coordinator, so many turns open on a relay that bundles a report with other findings; the strict reading counts those as other reports.

## Consequence

A closable report is at most a seventh of the coordinator's turns, so closing them by rule cannot move the coordinator's tokens per landed change enough to justify the build. The larger shares are other peer reports (38%), background notifications (18%) and alarms (14%); sorting those at the send path is north#1235's lane.
