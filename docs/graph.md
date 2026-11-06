# The fact graph

`north graph` is Beagle Store's triple kernel and five verbs (tell, retract, show, ask, validate) in Rust over SQLite
(`crates/north-graph`). Beagle is read for semantics only; no Clojure or Racket runs. SQLite holds rows, indexes,
durability and concurrent writers (WAL, 30 s busy wait, `BEGIN IMMEDIATE` per write); `ask` computes its fixpoint in
Rust, running each rule body as indexed lookups against SQLite. The database is `~/.local/state/north/graph.db`;
`--db PATH` or `NORTH_GRAPH_DB` names another, and `--writer NAME` or `NORTH_GRAPH_WRITER` names the writer of an occurrence.

## Kernel

- Atom: String, Int, Float, Bool, Keyword or Instant. Identity is the kind plus the canonical payload. A Float is its
  canonical bits: every NaN is one atom and `+0.0` is a different atom from `-0.0`, which fixes the gap Beagle's README
  states for host interning. An Instant needs nanoseconds below one second.
- Term: an Atom or a Triple. Triple: three Terms, nested in any position, with recursive structural identity. Terms are
  interned to integer ids; interning or nesting asserts nothing.
- Occurrence: coordinate, assert or retract, writer, proposition. The coordinate is
  `(("graph" :kernel/tx-sequence N) :kernel/op-ordinal K)`, an ordinary nested triple. Every assertion is a new
  occurrence even when an equal one is live. A retraction withdraws the newest live equal assertion, at most once; with
  none it is still recorded and reports `stateChanged false`. `apply` commits a batch of actions as one transaction.
- `validate` checks that every stored term decodes with children preceding it, every coordinate is well formed and
  increasing, every withdrawal binds a later retraction to an earlier assertion of the same proposition, liveness agrees
  with withdrawals, and the live-triple counts agree with the live assertions. It exits 1 on any disagreement.

## Terms on the command line

`"text"`, `42`, `1.5`, `true`, `:keyword`, `@SECONDS.NANOS9` and `(T1 T2 T3)`. Any other word is a string, so
`north graph tell north#669 :blocks north#670` needs no quotes. `show T1` prints each live triple with its live
occurrence count.

## ask

One JSON query (argument, `--file PATH` or `-` for stdin), Beagle's structured query with camelCase keys:

- `find`: a relation name, or `{rel, group, agg: [{op, arg}], having: [{op, agg, val}]}` with `op` in `count`,
  `count-distinct`, `sum`, `avg`, `min`, `max` (`sum` and `avg` need numbers; `min` and `max` use Term order).
- `rules` (one stratum) or `strata` (ordered). A rule is `{head: {rel, args}, body: [clause]}`; a clause is
  `{rel, args, neg?}`, `{pred: eq|ne|lt|le|gt|ge, args: [a, b]}` or `{fn: +|-|*|/|mod, args: [a, b], bind}`.
  Variables are `{"var": "x"}`. Constants are JSON strings, integers, floats and booleans, or tagged
  `["keyword","edge"]`, `["instant",secs,nanos]`, `["triple",a,b,c]`.
- `orderBy: [{column, direction: asc|desc}]`, `limit` (1 to 100000, applied after ordering) and `timeoutMs`.

Relations: `triple(t1,t2,t3)` has one row per distinct live proposition; `occurrence(coordinate, action, proposition)`
is the signed history with `:assert` and `:retract`; `assertion(writer, t1, t2, t3)` has a row per live assertion and the String that wrote it;
`quoted(term, t1, t2, t3)` has a row per triple term, asserted or only nested; `withdrawal(retraction, assertion)` has a row per successful
retraction. Clauses run in order; negation needs bound variables and a relation from an earlier stratum or SQLite;
arithmetic that overflows, divides by zero or yields a non-finite number drops the row and may not appear in a recursive
rule. Budgets are 10,000,000 steps and a 5 s timeout (60 s cap). Text relations and as-of or since views are not built.

Output is one tab-separated row per line in the term syntax above, or with `--json` an object with `columns`, `rows`
(tagged terms) and `steps`.

## Timing

`cargo run --release -p north-graph --example ask_scale` generates 1,000,000 occurrences (950,000
assertions and 50,000 retractions over 190,000 nodes), validates the store, checks a 3-hop `ask` anchored at one node against an independent
computation, and exits non-zero unless the median of 20 runs is under 50 ms.

## Ingest

`north graph ingest [--db PATH] [SOURCE...]` loads what north already records (`crates/north-graph-ingest`). Each run is one
short command for a timer or hook: it reads the local copies (never the GitHub API), compares them with the live assertions
and tells only the difference, so a rerun over unchanged sources asserts and retracts nothing. A source that cannot be read
is skipped with its reason and the exit code is 1; the others still load.

Facts are told by the writer that would know them. A worker's Done claim is a quoted triple asserted by that worker, never a
bare fact; a checker's observation is an assertion by `checker`. `assertion(writer, t1, t2, t3)` has a row per live
assertion with its writer and `quoted(term, t1, t2, t3)` gives the parts of any triple term, asserted or only nested, so a
query can join a claim to the checks of the commit it names.

- `mirror` (writer `mirror`): `(north#634 :title|:state|:label|:blocked-by|:has-box ...)` and, per task-list box,
  `(box:north#634:HASH :text|:ticked|:owner ...)`. A ticked box that names `owner: SESSION` is the session's claim
  `(SESSION :claims (box:... :done-at SHA))`, written by SESSION, one per commit named after `landed` (short ids expand
  to the full commit id when the ledger or push log knows it) or `:none` when the box names no commit. `:title`, `:state`,
  `:label`, `:blocked-by`, `:has-box`, `:text`, `:ticked`, `:owner` and `:claims` are replaced from the mirror: a box
  unticked or an issue closed retracts what no longer holds.
- `ledger` (writers `checker` and `ledger`): `(SHA :check-passed|:check-failed COMMAND)` and `(SHA :checked-for north#634)`
  by `checker`, from the ledger's checks; `(SESSION :works-on north#634)` and `(north#634 :landed-as SHA)` by `ledger`.
- `pushes` (writer `push-log`): per pushed commit `:repo`, `:branch`, `:pushed-at`, `(SESSION :pushed SHA)`, and for a
  commit that reached main `:landed-as`, `:landed-by` and `:landed-at`.
- `touches` (writer `git`): `(SHA :touched FILE)` for each commit the push log landed in the last 30 days, from `git log
  --name-only` of that repository's checkout under `~/code/REPO/main`. Facts are only added: a landed commit's files do not change.
- `decisions` (writer `decision-log`): per decision `(decision:ID :decision-kind|:at|:host|:follows|...)` with the branch,
  commit, lane, session, account and outcome the kind's case names. The log keeps 14 days; the graph keeps what it read.
- `corrections` (writer `corrections`): `(correction:ID :class|:at|:paraphrase|:in-session|:superseded-by ...)` from the
  grounding ledger and `(class:NAME :class-paraphrase|:class-owner|:class-guard|:class-guarded-at ...)` from
  harness/corrections.toml. The ledger holds Tom's corrections as neutral paraphrases and so does the graph.
- `coverage` (writer `coverage`): `(test:BINARY::NAME :reaches fn:FILE:START-END)`, `:in-binary`, `:in-file` and
  `(coverage:REPO :commit SHA)` from the coverage map, replaced whole by each map.

A predicate belongs to one source: a run refuses a source that replaces a predicate another source of the run replaces.

Which Done claims on north#634 have no checker observation, as one `ask` (`claim` joins each worker's quoted claim to the boxes
of the issue; `observed` is a commit `checker` passed):

```
{"find": "unobserved", "strata": [
  [{"head": {"rel": "claim", "args": [{"var": "who"}, {"var": "box"}, {"var": "sha"}]}, "body": [
     {"rel": "assertion", "args": [{"var": "who"}, {"var": "speaker"}, ["keyword", "claims"], {"var": "q"}]},
     {"pred": "eq", "args": [{"var": "who"}, {"var": "speaker"}]},
     {"rel": "quoted", "args": [{"var": "q"}, {"var": "box"}, ["keyword", "done-at"], {"var": "sha"}]},
     {"rel": "triple", "args": ["north#634", ["keyword", "has-box"], {"var": "box"}]}]},
   {"head": {"rel": "observed", "args": [{"var": "sha"}]}, "body": [
     {"rel": "assertion", "args": ["checker", {"var": "sha"}, ["keyword", "check-passed"], {"var": "command"}]}]}],
  [{"head": {"rel": "unobserved", "args": [{"var": "who"}, {"var": "box"}, {"var": "sha"}]}, "body": [
     {"rel": "claim", "args": [{"var": "who"}, {"var": "box"}, {"var": "sha"}]},
     {"rel": "observed", "args": [{"var": "sha"}], "neg": true}]}]]}
```

## Grounding

The UserPromptSubmit grounding hook and `north brief new --category C --item REPO#N` read this graph through
`crates/north-graph-ground`: for each issue (`north#634`) or file path a message names, up to four, they add the
neighborhood two hops out, and the hook shows it among its live lines inside its 600 ms retrieval bound.

- An issue: title, state, labels and blockers; its open boxes with owners; each ticked box with the Done claim of its owner,
  shown as a claim, or as observed when `checker` passed the commit the claim names, or as a failed check; recent landings;
  corrections made in the sessions that worked on it.
- A file: the functions the coverage map holds for it and the tests that reach them; recent landings that touched it with
  the issue each served; corrections made in those sessions.

The reader opens the graph read-only and never ingests, so a missing, locked or older graph costs the hook nothing but the
lines. `north land follow` runs `north graph ingest` after each landing, and ingests take a lock so concurrent follows run one
at a time. Started 2026-11-06: the week of ungrounded-design corrections and the scoreboard's rework rate are compared with the
week before once a week of data exists.
