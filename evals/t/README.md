# Eval T: T3 Code's own measures, run on north

Runs the measures T3 Code publishes about itself against north, on the same footing (south#107; the registered
design and its pass lines are in the assessment's `EVALS.md`, section "Eval T"). This directory holds the parts
that are north's: the frozen case list with the rule that made it, the runner of a case in both harnesses with T3 Code's grader,
the reader of a session's first request, and north's side of the thread-transfer fixture. Nothing here sets or judges a pass line.

## The pin

T3 Code is read at its release `v0.1.9`, commit `24e77473ecf41bc1f7142529c7911c1c1aae1d76`
(https://github.com/pingdotgg/t3code, MIT). `eval-t fetch --into DIR` fetches that commit and refuses a checkout
at any other commit or with a changed tracked file. Its cases and graders are read in place and never copied into
this repository; `cases.json` records the SHA-256 and git blob of every case file and of the grader sources
(`evals/mcp-tool-use/run.ts`, `regrade.ts`, `device-grading.ts`, `file-link-grading.ts`, `secret-grading.ts`), so
a later reader can show the grader ran unchanged.

## The frozen case list

`cases.json` is the output of `eval-t classify` at the pin: all 195 cases of `evals/mcp-tool-use/cases`, keyed by
case id, each with the reason it is in or out of each list. The rule is quoted in the file's `rule` field:

- a case is **neutral** when none of its checks names a tool only T3 Code has. A tool is T3 Code's when its name is
  lower-case and is defined under `apps/server/src/mcp/toolkits` (Claude Code's own tools are capitalised; the
  classifier fails if a lower-case name is not defined there). Checks built on T3 Code's thread state
  (`newThreads`, `linked`, `linkedNumbers`, `device`, `secret`) count as naming one.
- a case is **runnable in both arms** when a real session can be given its world: files alone (no mocked tool or
  shell result, no earlier turn, thread context or side chat, no file outside the repository), the top role, not
  Codex-only, and a prompt of its own. T3 Code's own runner replays a case against the model API and feeds those
  mocks and turns in; a real session in either harness cannot take them without stub tools nobody has built.
- a case **asks for something without T3 Code's tools** when it requires no T3 Code tool, has no T3 Code state
  check, and at least one check asks the agent to produce something (a call of a tool both harnesses have, a
  pattern its text or shell command must match, or working file links). A case whose only checks forbid things is
  not in this list, because a harness passes it by doing nothing.

Per case: `neutral`, `runnable_in_both_arms`, `requires_t3_tools`, `forbids_t3_tools`, `t3_state_checks`,
`world_needs_t3`, `asks_without_t3_tools`, `bars_without_t3_tools`, and `world_beyond_files`: what the case sets up
besides files (mocked tool or shell results, earlier turns, thread context, a side chat, files outside the
repository), which the world builder here does not write. `lists` names the case ids of `neutral`,
`neutral_and_runnable_in_both_arms`, `asking_for_something_without_t3_tools` and
`asking_and_runnable_in_both_arms`, and `counts` gives each list's size by kind (bad, guard, control). At the pin:
7 neutral, 3 of them runnable; 16 asking, 2 of them runnable.

`eval-t classify --checkout DIR --check cases.json` derives the list again from the checkout and fails unless it is
byte-for-byte the frozen file, so the list cannot drift from the rule or the pin.

## First-request context bytes

Claude Code (2.1.322 here) writes the shape of its first model request into the session's own transcript: an
`api-request-shape` record with the system text and the tool definitions, an `api-request` record with the model
and effort, and `api-request-blob` records with the first message's text. Both harnesses run the same Claude Code,
so that record is the same measurement point in every arm, with no proxy between a session and its endpoint
(north removes `ANTHROPIC_BASE_URL` from a session's environment, so a recording proxy could not sit there).

`eval-t context --transcript FILE [--bare FILE]` prints it: system bytes, tool count, deferred tool count,
tool-definition JSON bytes in total and per MCP server, first-message bytes, model and effort; with `--bare`, also
what the harness adds over a bare Claude Code session (`evals/t/arm bare DIR`: `claude -p` on the same prompt with
a configuration directory holding only the effort level). Every graded row carries the same figures under
`first_request`, so a behaviour run is also a context-bytes run.

## Thread-transfer bytes

`eval-t transfer-fixture --state DIR` writes north's side of T3 Code's thread-transfer fixture: a session record
and stream of 10 history turns (each 5 command tools with 4,096-byte outputs and one MCP result of 900,000 bytes),
a measured turn to append (20 command tools and one MCP result of 1,100,000 bytes), and `fixture.json` with the
sizes and the two north commands whose output is measured (`north fleet show`, and `north fleet show --follow`
while the measured turn is appended). The sizes follow T3 Code's fixture; the text is generated here.

`evals/t/transfer STATE` measures north's side on that fixture: the bytes `north fleet show` prints for the
10-turn thread (as text and as `--json`), how long that takes, and the bytes and lines `north fleet show --follow`
adds while the measured turn is appended to the session's stream, beside the bytes stored for each. T3 Code's side
is its own integration test at the pin, `apps/server/integration/transferBudgetV2.integration.test.ts`.

## Behaviour cases in both harnesses

`evals/t/run --smoke` runs one case (`file-links-package-relative`) once in each arm and prints one graded row per
arm; `evals/t/run --list NAME --samples N [--label NAME] [--seed N]` runs every case of a list of `cases.json`,
N samples each, the arm order per sample drawn from the seed. For each sample and arm it:

1. writes the case's world (`eval-t world`): the case's files committed on the case's branch in a new repository,
   with the case's prompt beside it. Both arms get the same tree; the row's world is refused when the case sets
   up more than files.
2. runs one session on the prompt (`evals/t/arm`), claude-opus-5-5 at medium effort unless `EVAL_T_MODEL` or
   `EVAL_T_EFFORT` say otherwise. The `north` arm is `north fleet launch --prompt north --test` in scratch state on
   the subscription account `EVAL_T_ACCOUNT` names. The `t3` arm is the T3 Code release binary (`T3_RUNTIME`,
   else `~/.local/lib/t3/current/t3`) served headless on loopback with telemetry off, a scratch home and a
   configuration directory holding only the effort level, logged in with the subscription OAuth token in the file
   `EVAL_T_TOKEN_FILE` names. Every `ANTHROPIC_*` variable is unset first, so no arm holds an API key.
3. turns the session's Claude Code transcript into one row of T3 Code's results shape and grades it
   (`eval-t grade`) by running T3 Code's own `evals/mcp-tool-use/regrade.ts` on it, from a second checkout of the
   pinned commit that carries its installed dependencies. The command refuses a grader checkout that is not at the
   pin or has a changed file under `evals`, `apps` or `packages`, and stamps the row with the SHA-256 of `run.ts`.

A row: case, kind, arm, pass, the checks T3 Code's grader failed, calls, stop reason, sample, seed, model, effort,
the T3 Code version and binary hash, the Claude Code version, the north runtime commit, the host, the start time and one-minute load of the sample, the label `REAL-HARNESS`, and the grader's pin and hash. The script fetches the pinned commit with plain git, because a cargo routed through a build gate has no network. Rows are
appended to `runs/LABEL/results.jsonl` under `EVAL_T_WORK` (default `evals/t/work`, which git ignores; a lane's `target` is reclaimed by lane gc, so nothing is kept there), where each
sample's world, transcript, row and grader output stay. The run exits non-zero when a sample gave no row or when
the T3 Code binary is not `v0.1.9`. T3 Code's own runner replays cases against the model API with a simulated
shell; nothing here does, so these rows are real-harness rows and are not comparable with T3 Code's README tables.

The grader's dependencies are installed once by `bun install --ignore-scripts` in the grader checkout (no install
script runs; the installer rewrites that checkout's root `package.json`, which the grader does not read), and the
grader runs under the Node the caller names in `EVAL_T_NODE` (default `node`; T3 Code pins 26.12.1, and 24.21 loads it).

## Rerun

```
cd evals/t && cargo build --locked --offline
target/debug/eval-t fetch --into /tmp/eval-t/t3code
target/debug/eval-t classify --checkout /tmp/eval-t/t3code --check cases.json
target/debug/eval-t transfer-fixture --state /tmp/eval-t/transfer
EVAL_T_ACCOUNT=LABEL EVAL_T_TOKEN_FILE=FILE evals/t/run --smoke
```

## Outside code

T3 Code (T3 Tools Inc., MIT, `LICENSE` at the pinned commit) is used as data and as a program: its case files are
read and its grader is run, both from a checkout fetched at run time at the pinned commit. No T3 Code source or
case text is copied into north; `cases.json` holds case ids, kinds, areas, check names and file hashes. The crate
reaches the network only in `fetch` (one `git fetch` of the pinned commit from GitHub) and builds with `serde_json`
alone. The grader checkout's dependencies are T3 Code's own npm tree (about 6.5 GB), downloaded from the npm
registry by `bun install --ignore-scripts` into the work directory, never into this repository, and loaded only as
far as `regrade.ts` imports them; the grader reads the case files and one row file and calls no model. The record is in [docs/references.md](../../docs/references.md#t3code).
