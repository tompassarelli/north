# Frontier scoreboard: north against plain Claude Code on the same boxes

The owner asked on 2026-11-07 for a scoreboard that shows whether north is at the frontier of agent harnesses, by how much and where it is not (north#730). Every other number in `north ledger scoreboard` is set against north's own targets or its own earlier week ([scoreboard](scoreboard.md)); this note sets north beside the same model run without north, on north's own recent work, and keeps the run rows that `north ledger scoreboard --frontier` prints.

## Method

`north test frontier` replays landed boxes from their base commit and grades each arm's tree by the landing's own tests.

**Tasks.** `north test frontier tasks` drafts candidates from the worker ledger: first attempts (no `Follows:`) in north that reported Done and landed, with a recorded brief, whose landing changed a test file. The frozen set is `docs/rationale/frontier/tasks.json`: each task names its issue, the ledger run, the base (the landing's parent), the landed sha, the original run's model and effort, the hash of its brief, the landing's test files and the test slice that grades it. Briefs stay in the ledger, since they can quote private messages.

**Frozen set (2026-11-07).** 32 candidates passed the brief screen. `north test frontier oracle` found a fail-to-pass test for 23; the other 9 had no test passing at the landing, mostly because the landing's slice does not run in the sandbox. Of the 23, 13 have tests that do not build at the base (they use names the landing added, as the first pass's two zero-score boxes showed), and 10 build there: those 10 are `tasks.json` (3 tooling, 3 bug with a known cause, 3 feature, 1 performance). The set is far below the 30 the design aimed for, because the ledger held no more first attempts with landed tests that a different implementation can pass; the first pass runs all 10, so its intervals are wide.

**Screens, before any arm runs.**
- A brief that reads the issue on GitHub or depends on live fleet, landing or log state is dropped: no arm can reach either.
- The oracle must exist: `north test frontier oracle` runs the slice at the landing twice and at the base with the landing's test files laid over it. Tests passing both times at the landing and failing at the base are the fail-to-pass set; a task with none is dropped.
- A task whose hidden tests call names that neither the brief nor the base gives is dropped, since a correct change under other names would fail. The oracle decides it mechanically: the landing's test files are laid over the base and built, and a task whose slice does not compile there has no oracle (`north test frontier oracle` refuses it). The first pass showed why: on two boxes both arms scored 0 of 73 and 0 of 46 because the tests did not compile against either arm (`struct Landed has no field named heads`, `no field verdict on type Verdict`), names the original landing chose.

**Arms.** Both get the brief as their first message, the same Claude Code binary, and the model and effort the original run had, the router's pick at launch.
- north: the argv `north fleet launch` builds for a worker (`Claude::host`), its fleet prompt and standing orders, the profile's instructions, skills, agents and hooks, and the worker session variables.
- plain: the same argv without the appended prompt, an empty config directory, and none of north's session variables.
- t3code (`--arm t3code --t3 DIR --t3-node PATH`): T3 Code's own server (T3 Tools Inc., MIT, github.com/pingdotgg/t3code at 32a0531748c44b25072d7495ce0b9bd7e9450a87, built from source) runs headless inside the same sandbox, and the brief is a thread started through its own CLI on the Claude provider, so Claude Code gets the instructions, the `t3-code` MCP tools and the adapter options T3 Code sends. The one seam is a `claude` shim that runs the same Claude Code binary, copies its stream to `stream.jsonl` for cost, wall time and the leak screen, and applies the same cost cap and web-tool ban. T3 Code's CLI cannot set an effort, so its default stands. The server's 20 tools that need a desktop host are dropped from the model's view and never stubbed: `preview_dialog`, `preview_status`, `preview_open`, `preview_navigate`, `preview_resize`, `preview_set_appearance`, `preview_act`, `preview_evaluate`, `preview_wait_for`, `preview_recording_start`, `preview_recording_stop`, `preview_snapshot`, `t3_preview_list`, `t3_preview_close` (a Playwright browser), `device_list`, `device_open`, `device_close`, `device_screenshot` (an agent-device shim) and `html_render`, `html_preview` (a headless Chrome); the sandbox holds none of them. The run record's harness line lists them.

**Sandbox, the same for both.**
- Each arm runs under bubblewrap. `$HOME` is empty except the harness files, the toolchain state, the cargo registry and the arm's own directory.
- The clone appears as `~/code/north/main` and is the only checkout. It is fed by pushing only the base into a fresh bare origin, so it holds no later object; the run refuses to start if the landed commit resolves inside it.
- `gh` refuses every call. `safe-push` pushes to the sandbox origin with no land checks. In the north arm, north's verbs that reach live fleet, landing, ledger or retrieval state refuse; the rest run.
- Both arms start in the repository's dev shell environment with no host tools on PATH. The dev shell's compiler cache wrapper picks its server from the checkout path, which inside the sandbox would name the real checkout, so it runs the compiler directly instead.
- WebFetch and WebSearch are disallowed in both arms. The transcript is screened for the landed sha and the public repository, and a pair with a hit leaves the headline.
- Arm sessions hold no capacity lease, as fleet workers hold none; the oracle's and the grader's test runs each hold a heavy lease, so they wait behind land checks.

**Run rules.**
- A run ends when the session has been idle for ten minutes after a result.
- If it then stopped without changing anything, or its last line asks a question, or it reports Not done or Blocked, it gets one fixed continuation ("No one will answer questions during this run. Make any decision yourself and finish the task."), at most twice. Each counts as an intervention.
- Both arms have the same per-run cap on API cost and wall time; a run that hits either is graded as it stands.
- Both arms of a task run back to back in a seeded order at fixed concurrency.

**Grading.** The grader reads the tree, never the report: what the arm pushed to its sandbox origin, else the lane it changed, else the main clone with its uncommitted files. It lays the landing's test files over it and runs the slice. A run is correct when every fail-to-pass test passes and no test that passed at the landing fails; a test the arm deleted is absent, not failed.

**Rows.** `north test frontier report --rows FILE` writes one JSON line per arm and task (`docs/rationale/frontier/results.jsonl`): the task, arm, model and effort, correct on first pass, correct after continuations, cost, wall seconds, interventions and whether the transcript reached past the sandbox.

**Metrics.** Only tasks both arms ran, with neither transcript touching the landed commit or the public repository, enter the headline; the line says how many were left out.
- Correct on first pass, and correct after the continuations, each apart: a Wilson interval per arm; the difference north minus plain with Newcombe's paired interval (method 10) and the exact two-sided McNemar test on the discordant tasks.
- Dollars per task, and dollars per correct task at list price (Claude Code's own `total_cost_usd`): the difference per correct task with a percentile bootstrap over tasks (10,000 resamples, fixed seed).
- Wall time to a correct result: the median over correct runs with a bootstrap interval, and the median paired difference over tasks both arms passed.
- Interventions per task: the mean with a bootstrap interval.

## What this does not measure

- Routing: both arms run the model and effort north chose, so north's choice of model is outside the comparison.
- Landing: neither arm gets production land checks, which leans against north.
- Retrieval over north's own records: hidden from the north arm, since those records hold the answers.
- The population is north's own code tasks whose landing has tests, not north's whole workload, and one model family.
- The brief is held fixed, and it was written by north, so north's briefing is outside the comparison too.

The design, its review round and each disposition are in north#730 and its first comment.

## Results

**T3 Code arm, one task (2026-11-07).** `north-685-4082956297` (claude-opus-5-5) through T3 Code's server: 26 turns, 128 s of wall time, 0 interventions; graded wrong (2 of 3 fail-to-pass pass, 1 existing test broken), leak screen clean; its cost at list price is in `results.jsonl`. The north and plain pilot rows for this task are on gex45 and are not in `results.jsonl`: this one row is. The grader had first been checked on prepared directories for the same task, a known-fail tree (fail-to-pass 0 of 3, 3 broken, wrong) and a known-pass tree (3 of 3, none broken, correct).

Pilot: three tasks on both arms measure the cost per task, which sets how many tasks the paid first pass can afford.
