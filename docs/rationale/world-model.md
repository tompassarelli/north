# North-graph as north's world model (north#882)

Tom chose on 2026-11-08 to make `north-graph` the one store every automatic decision reads, through full Beagle Store
unification, over a targeted fix of its costs. The review and the live audit behind it are in south
`agents/decisions/harness/north-graph-review-2026-11-08.md`. This note fixes the order, the data-model decisions and the
measurements; each lane's brief cites it.

## The rule everything else serves

A worker's claim is quoted, never asserted. `(SESSION :claims (BOX :done-at SHA))` interns `(BOX :done-at SHA)` as a term
and asserts only the outer triple. A box closes, an arm is rewarded and a failure is attributed only when a mechanical
writer (`checker`, `lander`, `git`) asserts a proposition that corroborates the quoted one. Readers ask the graph through a
named Datalog rule (`corroborated`), never the issue body's tick or the worker's report text. This is the property flat
ledgers, vector memories and LLM-extracted graphs (Zep/Graphiti) lack, and the one the Darwin Gödel Machine's reward
hacking shows a self-improving loop needs.

## Baseline (laptop, 2026-11-08 7:52 AM +07)

- `graph.db` 529 MB; `grounding/index.db` 259 MB (FTS5, 72,389 docs, 45 MB of text).
- Full `north graph ingest`, which `north land follow` runs after every landing: 30.6 s (decisions re-read 285,825 rows).
- `north graph validate --guards harness/graph-guards --shadow` after every landing: 11.3 s.
- `north graph ready`: 0.12 s. `north graph ground query`: 0.38 to 0.49 s.
- Unanchored `ask` over `triple(T, :reaches, F)`: 11.5 s, which was a wait on ingest's write lock, not query work: `ask`
  opened the store read-write and wrote its schema version on every open. Without a writer it reads 1.3 s cold and 0.38 s
  warm on the laptop.
- Landing, last hour: queued to push p50 669 s, p90 1,621 s; after-landing follow p50 25 s, p90 32 s (56 landings).

Each lane reports the same five readings before and after: ingest wall, the query latency it touches, landing-check time
(`north land profile`), after-landing follow time and `graph.db` size. A lane that makes a landing slower is not done.

## Data-model decisions

1. **SQLite stays** (not required to replace it). The kernel tables (`terms`, `occurrences`, `withdrawals`, `live_props`,
   `quoted`) keep their meaning; schema changes are additive and bump `SCHEMA_VERSION` with an in-place upgrade.
2. **Temporal views select by occurrence `seq`.** Beagle selects by transaction; `seq` is finer (every transaction ends on
   a seq) and already indexed. `asOf: U` reads occurrences `seq <= U` and withdrawals whose retraction is `<= U`;
   `since: [L, U]` restricts every base relation to `L <= seq <= U` (north#880; Beagle's native engine bounds every
   relation the same way but excludes L).
   Wall clock stays metadata: a reader that needs "as of time T" maps T to a seq through a recorded commit mark, added
   only when its first reader (route replay, the climb archive) lands.
3. **Joins push into SQL.** Each run of stored-relation clauses in a rule body, with the comparisons after it, is one SQL
   join in the order the rule writes it (SQLite's own order made one chain query 4x slower). Range comparisons call an SQL
   function that runs the same term comparison as the Rust path, so Int, Float and Instant order alike either way.
   Derived relations and recursion stay in the Rust semi-naive loop.
4. **Text is a live-proposition index inside `graph.db`.** FTS5 tables over the String `t3` of live propositions whose
   predicate is text-indexed (`:doc-title`, `:doc-text`, and the mirror's `:title` and `:text`), kept in the same
   transaction as the assertion or retraction, serve all five Beagle relations: `text-match` and `text-phrase` on
   `unicode61`, `text-stem` on `porter`, `text-substring` on `trigram`, `text-search` scored by bm25 with Beagle's exact
   over stem over substring order. Grounding documents become `(doc:SOURCE:ID :doc-source S)`, `:doc-ref`, `:doc-title`,
   `:doc-text`, `:doc-at` and `:doc-decays` under writer `grounding`. A document's text atom is immutable: a growing source
   (transcripts) writes append-only chunks, so a refresh adds terms instead of rewriting them, and `graph.db` growth per
   refresh day is a reported reading. Terms are unique by a 16-byte hash of their key, not by the key itself, so a
   document's text is stored once (schema v3).
5. **Snapshot sync only where a source is a snapshot.** A predicate is owned by exactly one source. `Graph::sync` reads the
   rows of the predicates it replaces from every writer (the mirror tells claims under each owner's writer), plus the rows
   of the writers it tells for any other predicate, instead of every live row. Loops and observers append through
   `Graph::apply`; they never re-read their whole history.
6. **Facts with no reader are not ingested.** `decision-log` (57% of live assertions, no reader) is retracted once and its
   source deleted; the raw `.jsonl.zst` stays the replay input.
7. **Loops read and write through rules.** A loop in `harness/loops.toml` with `store = "graph"` writes its outcome
   observations as assertions (`(run:ID :kept BOX)`, `(cell:CATEGORY:TIER :p75-wall N)`) and reads its decision input
   through a rule file under `harness/graph-rules/`. `north loops check` judges freshness from the newest occurrence of the
   loop's writer. A loop set by hand (pressure band, placement, worker concurrency, skill routing) is a named exception
   until it learns from an outcome. The graph is per host: a loop writes the graph on the host that reads it (`host` in
   loops.toml). The ledger host, nexus, runs dispatch, `north tune` and the climber; it ingests its own graph every five
   minutes from the sources it holds (docs/graph.md says which). Until 2026-11-08 it had none, so dispatch there ordered
   by priority label alone.
8. **Guards run without the soundness scan.** `north graph guards DIR` evaluates guard rules alone (under 500 ms) after
   each landing; `validate` stays the soundness check run on demand. Recurring structural classes from
   `harness/corrections.toml` compile into guard files that land in shadow under the gate-shadow rule.
9. **Policy lineage is graph facts.** Each climb candidate is `climb:ID` with `:parent`, `:mutates FILE`, `:arm`,
   `:scored` (from checker-verified kept boxes only) and its landing seq, read with `asOf` so a candidate is judged on the
   graph as it stood when it landed.

## Order

The tax comes first because every later lane writes more facts. Text and temporal views come before the stores that need
them; rewards come before the climber because the climber settles on them.

1. Wave 1, in parallel (disjoint files): ingest and guard tax (north#879); temporal views and join pushdown (north#880 and
   the pushdown box); the text index in the store; corroborated rewards and box closes (north#872's reader); the loop
   vocabulary and `store = "graph"` check.
2. Wave 2, as their inputs land: text relations in `ask`; grounding into `graph.db` and `index.db` deleted; one wiring lane
   per loop family; correction classes compiled into guards (north#706); graph context in dispatched briefs (north#873,
   on its context-pack arm); the climb archive.
3. Clean-up: tests that pin nothing, scaffolding and stale docs go; `docs/graph.md` describes what shipped.

## Prior art

- Beagle Store (`~/code/beagle/main/store`, MIT or Apache-2.0, Tom's): the kernel, and its query reference (in the
  store repository's docs directory) for text
  relations and `:query/as-of`/`:query/since`. North reads it for semantics only.
- Datalog engines: datafrog (Rust, sorted-relation leapjoin, what rustc's Polonius uses) is the model for in-memory
  derived relations; Soufflé compiles stratified Datalog to specialized joins; CozoDB embeds Datalog with FTS, HNSW and
  `Validity` time travel over flat tuples. None keeps quoted propositions apart from assertions, which is why north keeps
  its kernel and borrows their join planning rather than adopting one.
- Provenance: RDF-star / RDF 1.2 quoted triples (a quoted triple is not asserted), W3C PROV (agent, activity, entity),
  and provenance semirings (Green, Karvounarakis, Tannen 2007) for why a derived row holds.
- Self-improving loops: the Darwin Gödel Machine (Sakana, 2025) and AlphaEvolve keep lineage archives scored on held-out
  checks; DGM documents reward hacking when verification is weak.
