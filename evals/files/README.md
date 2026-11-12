# Eval: predicting the files a task will change

Scores how many of the files a task's reference commits edit each picker would put in a first prompt (north#1234, after the
graph and text-search measurement of north#1147). It reuses that measurement's tasks (`evals/a/tasks-19.json` of the
assessment: 19 tasks, 91 edited files), its truth (every file the reference commits edit, the other repository's files
included) and its launch caps (a file over 8 KB is skipped, and so is one that would take the total past 32 KB, applied in
rank order).

Pickers, all reading the base commit's tree only (no landed diff, no later commit):

- graph: `NORTH_GROUNDING_AS_OF=<base commit time> north graph ground query --query <brief>`, files in the order named;
- text search: `git grep -c -i -F` for up to 24 words of the brief (stopwords dropped), ranked by distinct words matched, then match count;
- classifier on the repository map: the brief and every text file of the base tree, each with up to six names it declares
  (the graph indexes no code symbols for these repositories, so the names are read from the base blobs; the map shrinks
  its symbols stepwise to stay under 240 KB);
- classifier on candidates: the brief and the top 30 text-search and top 15 graph files, with their names, to choose from;
- unions of those under the same caps.

The classifier is `north check oneshot`, the harness's own one-shot call (no tools, no settings, no session, API-billing environment unset,
fast mode off; the harness picks the account, so this code touches no login), on `claude-haiku-5-5` at low effort, asked for at most 10 paths. The summary gives wall time and the tokens the call reports.
It is given the brief and paths of the repository the task starts in, nothing else; the graph's session and ledger text is
never sent. Run it only on public repositories.

## Rerun

```
cd evals/files && cargo build --offline
target/debug/eval-files --tasks TASKS-19.json --work /tmp/eval-files [--north PROGRAM]
```

`--work` keeps the graph output and each classifier reply under `raw/`; a rerun reuses them, so delete `raw/` to ask again.
The summary (table, classifier time and tokens, per-task counts, every miss with its reason) is written to `summary.md`.

Columns: found is edited files among those kept under the caps; files named is what the picker ranked before the caps (text search
names every file that holds any word, so its figure is the repository's size); ceiling is edited files the picker's input
contained that are also under the 8 KB file cap, so no variant can pass it.

## Experiment G1 of north#1147: grounding a brief from its base commit

`eval-files boxes` replays the registered G1 line on north's own closed boxes. Set: the newest closed north issues whose
only mention in main's first-parent history is one commit with `(north#N)` in its subject. Base: that commit's parent.
Input: the issue's title and body as of the base commit's time, read from GitHub's edit history (the fleet's stored briefs
live in the ledger of the host that launched them). Truth: every file the commit changed. A picker scores a box when a
changed file is in its top 5 (and, separately, top 10).

Pickers, from the box text and the base commit only:

- (a) the full grounding query, `NORTH_GROUNDING_AS_OF=<base time> north graph ground query --query <text>`;
- (b) text search, as above;
- (c) the graph step: files the text names that exist at the base, then files that define a code-shaped name the text
  uses (a Rust item definition at the base), then files that changed with those in the base's history, then the files of
  the tests that the graph's coverage map says reach them;
- (d) the classifier on the brief and the repository map, as above; and, beside it, the classifier on the top text-search
  and step candidates;
- the pack as dispatch attaches it today: the paths of a `Files:` line.

```
cp ~/.local/state/north/graph.db /tmp/g1/graph.db
NORTH_GRAPH_DB=/tmp/g1/graph.db target/debug/eval-files boxes --repo NORTH_CHECKOUT --graph /tmp/g1/graph.db --work /tmp/g1/run --count 120
```

The box set, text-search ranks and classifier replies are kept under `--work` and reused on a rerun. The copy is needed
inside a worker sandbox, which opens the live graph read-only and gets no rows back.
