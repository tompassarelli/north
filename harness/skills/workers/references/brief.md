# Writing a worker brief

Read this when you write a brief in full, queue a box behind another lane, or brief a bug, native item or tooling box. The header lines are in `SKILL.md`.

- Spawn in-process `Agent` workers from the main checkout (`~/code/<repo>/main`), never from your own lane: they inherit your cwd and commit onto your branch; the pre-agent hook denies it unless the brief names the lane.
- Brief: Item, Category, goal, files, Sources (the decisive authoritative plan, spec, data or code; a summary only with a pointer to what it digests), Methods (only when the choice changes correctness), Done when, ETA, facets `Spec: exact|measured|judged`, `Scope: one-file|module|cross-module|cross-repo`, `Surface: ts|rust|lua|nix|shell|workflow|docs|assets`, `Verify: none|local-test|farm|native|visual`, and the state checked at spawn (main SHA and CI, lane SHAs, what landed).
- Category: mechanical, docs-policy, tooling, feature, bug-known-cause, debugging-unknown-cause, netcode-determinism, performance, balance-tuning, native-check or research. `the Codex brief check in crates/north-core/src/router/brief.rs` reads only the lines before the first blank line, so put Category, the four facets, `Done when:` with a backticked command, `ETA:`, `Files:` and any `Follows:` there.
- The first Done when line of every brief is an oracle: an observation outside the agent naming the artifact or state and the pass condition, and the agent attaches the observed result, because a claimed pass is not evidence. For each Done when line, the box's writer names the existing property, replay or golden test that covers it; a new example test needs a one-line reason.
- A bug brief's Done when covers every instance of its `Class:`, not the one symptom seen.
- A bug-fix brief's Done when names the test that fails on the old code and asks the report to paste that failure and the passing run after the fix; a refactor brief asks for `Behavior: unchanged` in the commit message and its changed tests passing on the old code. Which kind of test to ask for is in the [verification skill](../../verification/SKILL.md).
- Queue each box that waits on another lane by piping its brief, carrying `Follows: <commit subject, SHA or Item>`, to `north threads queue follow <repo#N> --tier <tier> --parent <your Fleet session name>`, so the predecessor's landing starts it instead of someone noticing.
- Pick ready work with `north threads ready` and read its Done when with `north threads show repo#N`. Claim shared-resource work through `north threads`; a running worker's brief is its claim.
- Workers on native items (life, accounts, travel, ops) report with `north threads note REF TEXT`, never issue comments, because those items are private.
- Research Tom asks for inline is timeboxed at 5 minutes and ends by offering a deeper pass with its own ETA.
- Brief a tooling or infrastructure box with its Payback line per [payback.md](payback.md).
