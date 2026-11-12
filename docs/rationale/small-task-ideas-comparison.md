# Small-task speed-up ideas: two sessions compared (2026-11-12)

Two sessions were given the same request anatomy for north#1239 and asked for ranked ideas to cut a small task's launch-to-landed time, each without reading the other. One ran on the largest model at its highest effort (claude-fable-5-2, xhigh), the other on the model a tier below at high effort (Opus 5.5, high). This note lists every idea from both, matched by mechanism, for the staffing record to cite. Status was read from `origin/main` at 71100d8c8.

| # | Mechanism | Smaller model (S): idea, seconds at one / five workers | Larger model (L): idea, seconds at one / five workers | Found by | Status on origin/main |
|---|---|---|---|---|---|
| 1 | A worker already running, handed the task | S1, 3.0 / 4.5 (also pays its own cache write in a warm-up turn) | L1, 2.1 / 2.9 to 4.2 | both | landed 32a91e9ce and 1cad099c0, then reverted by d8d65b1ab (held from promotion by review); box open |
| 2 | Judge a predicted commit message at launch | S3, 1.0 / 1.2 | L3, 1.2 / 1.2 | both | landed ae46d6a55, 7d4e7f03e |
| 3 | Finish-call round trips: start together, or fetch each fact once (destination, archived, visibility) | S2, 0.5 to 0.8 / 0.6 to 0.9 | L4 first part, 0.7 (1.1 to 0.4) | both; unsure they are the same (S runs the trips side by side, L fetches each once and earlier) | no commit; box "One round trip per fact" open |
| 4 | Kept connection to the git host for the deny-list fetch (S also the push) | S6, 0.9 to 1.1 (0.6 fetch, 0.3 to 0.5 push) | L4 second part, inside L4's 1.2 | both | landed b103d2a79 (fetch part); the push part has no box |
| 5 | Shared prompt prefix across workers | S4, 0.4 / 0.6 (session name, lane path, date lines) | L5, 0 to 0.4 (session name) | both | session name landed 98ba19cfa; lane path and date lines unstaffed |
| 6 | Finish call omits fields the harness knows (check, message, paths) | S5, 0.4 to 0.6 | L2 second part, about 0.2 | both | message and paths optional on main (ae46d6a55 and the finish schema); `check` still required; unstaffed |
| 7 | Line tags instead of old text | S11, 0.3 to 0.5 (ranked last, replay first) | L2 first part, 1.1 beyond exact old text | both | box open; exact-old-text form landed 3a030a407 |
| 8 | Apply and check while the call streams (partial messages) | S7, 0.4 to 0.7 / 0.5 to 0.9 | L7 first part, 0.4 / 0.6 | both | unstaffed; the verdict box later dropped stream reading |
| 9 | Hedge the first byte on two workers | S10, 0.2 median at five, about 2 at p90 | L6, 0.5 median at one, 2 to 3 at p95 | both | unstaffed |
| 10 | Start the task before the launch (unfair on the ladder) | S14, 0 under a fair clock | L10, 0 under a fair clock (1 to 2 on a real queue hit) | both | unstaffed; both sessions say it is no ladder win |
| 11 | A lane warm for its check | S13, 0.1 to 0.3 | L8 second part, no seconds given | both; unsure they are the same (S installs dependencies and warms caches, L runs the check once at launch) | unstaffed |
| 12 | Lander wakes on a check's exit instead of its 250 ms tick | none | L4 third part, no separate figure (0.5 is L4's 1.2 less its first part's 0.7) | larger only | no commit; box open, `TICK` still 250 ms |
| 13 | Provisional ticket so the lander builds its candidate while the check runs | none | L7 second part, not estimated | larger only | unstaffed |
| 14 | A judge client already started and waiting | none | L8 first part, 0.3 / 0.4 when row 2 misses | larger only | unstaffed |
| 15 | A lean first request (only the finish tool and short instructions) | none | L9, 0.2 to 0.6 | larger only | unstaffed; L9 calls it the owner's call |
| 16 | Judge slots at five workers | S8, 0 / 0.3 median, 1.5 slowest | none | smaller only | unstaffed |
| 17 | Land a batch in one push at five | S9, 0 / 0.5 to 1.0 | none | smaller only | unstaffed |
| 18 | End the turn at the queued line | S12, 0 landed seconds (one request less, a worker free about 3 s sooner) | none | smaller only | unstaffed |

How the rows were cut: ideas were matched by mechanism. Where one idea bundled two mechanisms (L2, L4, L7, L8) it was split into rows and each part marked; seconds were split only where the session gave the split. Seconds are as each session stated them, against its own anatomy (12.4 s larger, 12 s smaller at one worker).

## Counts for the staffing record

- Only the larger model: 4 ideas (rows 12 to 15); expected seconds 1.0 to 1.4 at one worker and 1.1 to 1.5 at five (row 12's 0.5 is derived; row 13 was not estimated).
- Only the smaller model: 3 ideas (rows 16 to 18); expected seconds 0 at one worker and 0.8 to 1.3 at five.
- Found by both: 11 rows.
