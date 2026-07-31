# Graph-native versus text authoring experiment

The experiment measures authoring workflows for real tasks; it does not use the
assignment result to decide whether a task enters the sample.

## Eligibility and assignment

A task is eligible when it has a stable task id/thread before authoring starts,
touches at least one already graph-upstream file, has the same acceptance bar in
either arm, and has a working Fram graph stack. Decide eligibility first, then
run `~/code/north/main/bin/north-arm-assign --record --thread <thread> <stable-id>`
exactly once.

Exclude emergency fixes, tasks whose required authoring style is externally
fixed, graph adoption/guard implementation itself, generated-output-only edits,
and runs with an unavailable graph or text-token seam. An operator may use
`--force graph|text --why <reason> --record --thread <thread>`, but
`forced-graph` and `forced-text` are audit rows, never A/B samples.

The assignment hashes the exact UTF-8 identifier bytes, without a trailing
newline, using SHA-256. An even low bit is `graph`; an odd low bit is `text`.
The function has no salt or mutable state, so the same identifier always selects
the same arm.

## Run-recorder contract

Record one experiment row on the exact run entity. The task thread's `run_arm`
fact joins the row to its assignment.

| Fact | Contract |
| --- | --- |
| `run_arm` | `graph`, `text`, `forced-graph`, or `forced-text` |
| `run_outcome` | `success`, `failure`, or `blocked` |
| `run_retries` | Failed authoring/acceptance cycles before the accepted result; nonnegative integer |
| `run_tokens` | Exact provider token total, or `unknown`; unknown is never recorded as zero |
| `run_wall_ms` | Monotonic elapsed time from recorded assignment through accepted result and text-arm reconciliation |
| `run_size_files` | Count of changed tracked files in the accepted Git diff |
| `run_size_loc` | Added plus deleted text lines from `git diff --numstat`; binary changes are flagged separately |
| `run_size_bucket` | `small` (1–20 LOC), `medium` (21–100), or `large` (>100) |

First-try success is derived as `run_outcome=success` and `run_retries=0`.
Text-arm token minting, reconciliation, normalization, and any follow-up commit
are inside the timer and retry count. Infrastructure failures remain `blocked`;
they are reported separately and are not silently converted to authoring
failures.

## Sample and decision guidance

Do not call a winner with fewer than 30 eligible completed runs per arm in a
size bucket. Report missing-token coverage and keep token-bearing analysis
separate until at least 80% of both arms have exact token totals. Inspect
failure/blocked reasons before combining them; a systemic missing-token or guard
failure is an experiment defect, not evidence for an authoring arm.

Within each size bucket, define the inefficiency score as:

```text
median(run_tokens) × median(run_wall_ms) / first_try_success_rate
```

Lower is better. Tokens are the subscription-capacity cost proxy, not invented
API-credit dollars. Compare the graph/text score ratio with a bootstrap interval
and publish the three inputs beside the product so one improvement cannot hide a
large regression in another. “Graph is better” means the interval is below 1.0
in at least two populated size buckets, the point estimate improves by at least
15%, and graph is not more than 5% worse in the remaining populated bucket.
The symmetric rule selects text. Otherwise the result is inconclusive and data
collection continues.
