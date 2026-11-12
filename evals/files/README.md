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
