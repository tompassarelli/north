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
is the signed history with `:assert` and `:retract`; `withdrawal(retraction, assertion)` has a row per successful
retraction. Clauses run in order; negation needs bound variables and a relation from an earlier stratum or SQLite;
arithmetic that overflows, divides by zero or yields a non-finite number drops the row and may not appear in a recursive
rule. Budgets are 10,000,000 steps and a 5 s timeout (60 s cap). Text relations and as-of or since views are not built.

Output is one tab-separated row per line in the term syntax above, or with `--json` an object with `columns`, `rows`
(tagged terms) and `steps`.

## Timing

`cargo run --release -p north-graph --example ask_scale` generates 1,000,000 occurrences (950,000
assertions and 50,000 retractions over 190,000 nodes), validates the store, checks a 3-hop `ask` anchored at one node against an independent
computation, and exits non-zero unless the median of 20 runs is under 50 ms.
