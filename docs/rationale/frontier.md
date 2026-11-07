# Frontier scoreboard: north against plain Claude Code on the same boxes

The owner asked on 2026-11-07 for a scoreboard that shows whether north is at the frontier of agent harnesses, by how much and where it is not (north#730). Every other number in `north ledger scoreboard` is set against north's own targets or its own earlier week ([scoreboard](scoreboard.md)); this note sets north beside the same model run without north, on north's own recent work, and keeps the run rows that `north ledger scoreboard --frontier` prints.

## Method

`north frontier` replays landed boxes from their base commit and grades each arm's tree by the landing's own tests.

**Tasks.** `north frontier tasks` drafts candidates from the worker ledger: first attempts (no `Follows:`) in north that reported Done and landed, with a recorded brief, whose landing changed a test file. The frozen set is `docs/rationale/frontier/tasks.json`: each task names its issue, the ledger run, the base (the landing's parent), the landed sha, the original run's model and effort, the hash of its brief, the landing's test files and the test slice that grades it. Briefs stay in the ledger, since they can quote private messages.

**Screens, before any arm runs.**
- A brief that reads the issue on GitHub or depends on live fleet, landing or log state is dropped: no arm can reach either.
- The oracle must exist: `north frontier oracle` runs the slice at the landing twice and at the base with the landing's test files laid over it. Tests passing both times at the landing and failing at the base are the fail-to-pass set; a task with none is dropped.
- A task whose hidden tests call names that neither the brief nor the base gives is dropped after reading them, since a correct change under other names would fail.

**Arms.** Both get the brief as their first message, the same Claude Code binary, and the model and effort the original run had, the router's pick at launch.
- north: the argv `north fleet launch` builds for a worker (`Claude::host`), its fleet prompt and standing orders, the profile's instructions, skills, agents and hooks, and the worker session variables.
- plain: the same argv without the appended prompt, an empty config directory, and none of north's session variables.

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

**Rows.** `north frontier report --rows FILE` writes one JSON line per arm and task (`docs/rationale/frontier/results.jsonl`): the task, arm, model and effort, correct on first pass, correct after continuations, cost, wall seconds, interventions and whether the transcript reached past the sandbox.

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

None yet: `results.jsonl` is empty until the pilot runs. The pilot runs three tasks on both arms to measure the cost per task, which sets how many tasks the paid first pass can afford.
