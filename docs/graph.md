# The fact graph

`north graph` is Beagle Store's triple kernel and six verbs (tell, retract, show, ask, validate, guards) in Rust over SQLite
(`crates/north-graph`). Beagle is read for semantics only; no Clojure or Racket runs. SQLite holds rows, indexes,
durability and concurrent writers (WAL, 30 s busy wait, `BEGIN IMMEDIATE` per write); `ask` computes its fixpoint in
Rust, running each rule body as indexed lookups against SQLite. The database is `~/.local/state/north/graph.db`;
`--db PATH` or `NORTH_GRAPH_DB` names another, and `--writer NAME` or `NORTH_GRAPH_WRITER` names the writer of an occurrence.

## Kernel

- Atom: String, Int, Float, Bool, Keyword or Instant. Identity is the kind plus the canonical payload. A Float is its
  canonical bits: every NaN is one atom and `+0.0` is a different atom from `-0.0`, which fixes the gap Beagle's README
  states for host interning. An Instant needs nanoseconds below one second.
- Term: an Atom or a Triple. Triple: three Terms, nested in any position, with recursive structural identity. Terms are
  interned to integer ids; interning or nesting asserts nothing. A term is found by the first 16 bytes of the SHA-256 of
  its key through a unique expression index (schema v3), so a long string is stored once; a v2 store is rebuilt in place
  on its first writable open and vacuumed.
- Occurrence: coordinate, assert or retract, writer, proposition. The coordinate is
  `(("graph" :kernel/tx-sequence N) :kernel/op-ordinal K)`, an ordinary nested triple. Every assertion is a new
  occurrence even when an equal one is live. A retraction withdraws the newest live equal assertion, at most once; with
  none it is still recorded and reports `stateChanged false`. `apply` commits a batch of actions as one transaction.
- `validate` checks that every stored term decodes with children preceding it, every coordinate is well formed and
  increasing, every withdrawal binds a later retraction to an earlier assertion of the same proposition, liveness agrees
  with withdrawals, and the live-triple counts agree with the live assertions, and that a retention horizon is not past the head. It exits 1 on any disagreement.
- `guards DIR` runs the guard queries of a directory (see Guards) without the soundness check.

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
- At most one view over the occurrence sequence `seq`: `asOf: U` reads the store after occurrence `U`, so `triple` and
  `assertion` hold what was asserted through `U` and not withdrawn by a retraction through `U`, `occurrence` stops at `U`
  and `withdrawal` keeps retractions through `U`. `since: [L, U]` keeps that upper state and bounds every one of those
  relations below by `L`, inclusive: assertions and occurrences from `L`, withdrawals whose retraction is from `L`.
  `quoted` is not cut, since terms carry no sequence. A negative, reversed or past-the-head bound is an invalid snapshot, and
  a bound below the retention horizon is an expired one (see Retention).

Relations: `triple(t1,t2,t3)` has one row per distinct live proposition; `occurrence(coordinate, action, proposition)`
is the signed history with `:assert` and `:retract`; `assertion(writer, t1, t2, t3)` has a row per live assertion and the String that wrote it;
`quoted(term, t1, t2, t3)` has a row per triple term, asserted or only nested; `withdrawal(retraction, assertion)` has a row per successful
retraction. Clauses run in order, except that each run of positive stored-relation clauses, with the comparisons that follow them, is one SQLite join over the variables bound before it, projected to the variables later clauses and the head read; negation needs bound variables and a relation from an earlier stratum or SQLite;
arithmetic that overflows, divides by zero or yields a non-finite number drops the row and may not appear in a recursive
rule. Budgets are 10,000,000 steps and a 5 s timeout (60 s cap). The five text relations are under Text.

Output is one tab-separated row per line in the term syntax above, or with `--json` an object with `columns`, `rows`
(tagged terms) and `steps`.

## Text

The store keeps a full-text index in `graph.db` over the String objects of four text predicates: the name predicates
`:title` (the mirror's issues) and `:doc-title`, and the body predicates `:text` (the mirror's boxes) and `:doc-text`.
Each subject with a live proposition on one of them is one row of the FTS5 table `text_stems` (porter stemming over
unicode61, diacritics removed, contentless with deletes), holding its names in a `name` column and its bodies in a `body`
column. A row is rewritten in the same transaction as the assertion or retraction that changes those propositions, so
the index never disagrees with the live triples; a store whose index is missing or older is rebuilt on its next
writable open. Readers rank with bm25 and per-column weights and may cap hits per value of a facet predicate
(`Graph::text_search`), and read stems, document frequencies and near spellings from the vocabulary.

Beside it, every live String object, whatever its predicate, is one row of three more contentless FTS5 tables keyed by
its proposition (`live_props.prop`) and kept in the same transaction: `text_words` (unicode61 over letters and decimal
digits, case-folded, diacritics kept), `text_porter` (porter over the same tokens) and `text_trigram` (case-folded
trigrams without positions, `detail=none`, 61 MB where positions took 197 MB on the laptop snapshot). Schema v4 adds
them; a v3 store builds them on its next writable open. `ask` answers Beagle Store's five text relations from them,
each examining the third position of live propositions:

- `text-match(E, A, NEEDLE)`: the needle's tokens as an unordered conjunction, from `text_words`.
- `text-phrase(E, A, NEEDLE)`: the same tokens in order across punctuation, so `"quick brown fox"` matches
  `quick-brown_fox` and not `brown quick fox`.
- `text-substring(E, A, NEEDLE)`: case-folded containment that keeps punctuation, so `OWN_FO` matches `quick-brown_fox`;
  a needle of three or more characters narrows to the values holding every one of its trigrams and is confirmed in
  Rust, a shorter one scans the live Strings (only the constant attribute's, when there is one).
- `text-stem(E, A, NEEDLE)`: porter stems, so `runs`, `running` and `run` unify; `text-match` stays exact.
- `text-search(E, A, NEEDLE, SCORE)`: any of the three, binding a Float score: exact evidence scores in [2, 3), stem in
  [1, 2), substring in [0, 1), each tier ordered by bm25 (`r / (1 + r)` of the negated bm25); a pair holding several
  matching values keeps its best.

Tokens follow Beagle's tokenizer v0: maximal letter and digit runs, lowercased without locale, `_`, `-`, whitespace and
punctuation delimiting, repeats dropped (`text-phrase` keeps the needle's full sequence). The needle is a String constant
or a String variable bound by an earlier clause; a negated text clause, an unbound needle and an empty or
punctuation-only needle are refused when the query is checked, and a bound needle that is not a String matches nothing.
A constant attribute restricts the SQL to that predicate; a variable one reads every live String through the same
index. Under `asOf` or `since` a text relation is refused with `TextUnderView`, because the index holds only live
propositions. Known differences from Beagle: SQLite's case folding differs from Rust's `to_lowercase` on a few
characters such as `İ`, so a substring needle holding one may miss a trigram candidate; porter leaves `runner`
unstemmed, where Beagle's reference lists it with `run`; and the needle check counts every alphabetic and numeric
character as a token character where unicode61 counts only letters and decimal digits (`²`, `Ⅻ`).

Grounding docs live here (`north graph ground`, writer `grounding`): each is `doc:SOURCE:ID` with `:doc-source`,
`:doc-title`, `:doc-text` (at most 4,000 characters, redacted), `:doc-at` (Float seconds) and `:doc-family`, the
retrieval family the hook caps at 80 hits per query; `:doc-ref` only where it differs from the source's own form
(`convo:ID` for transcripts and builds, `ledger:ID`, `github ID`, `threads ID`) and `:doc-decays true` only on a doc
that decays, since each proposition costs about 700 bytes across the kernel's tables. The predicates are grounding's own
because a predicate is replaced by exactly one ingest source, and the mirror replaces `:title` and `:text`. A refresh
writes only the propositions that changed, and the kernel keeps every term it ever interned, so a source that rewrites a
doc's text leaves the old string behind; `grounding/watermarks.json` records each source's newest doc, last refresh and
row count.

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
  `(box:north#634:SLUG :text|:ticked|:owner|:done-when ...)`, where SLUG is the box's claim slug (its first 48 characters of
  lowercase words joined by `-`), so a run's `Box:` line and the issue's task-list line name one box. `:done-when` is the
  command of the box's ``Done when: `CMD` ``. A ticked box that names `owner: SESSION` is the session's claim
  `(SESSION :claims (box:... :done-at SHA))`, written by SESSION, one per commit named after `landed` (short ids expand
  to the full commit id when the ledger or push log knows it) or `:none` when the box names no commit. Each worker run the
  threads ledger records as done on an item's box is its claim too, `(AGENT :claims (box:... :done-at SHA))` with the run's
  landed commit or `:none`, and a `Box:` line that carries a Done-when command adds `:done-when` by `ledger`, so the claims of
  a closed issue stay. `:title`, `:state`, `:label`, `:blocked-by`, `:has-box`, `:text`, `:ticked`, `:owner` and `:claims`
  are replaced from the mirror and the runs: a box unticked or an issue closed retracts what no longer holds. `:done-when`
  is only added, so a closed issue's boxes keep the command they declared.
  Waits come from the same issues: `(box:... :waits-on box:...|north#560)` for each unticked box whose text names what it
  waits on (`waits on`, `waiting for`, `blocked by`, `depends on`, `follows`, `after`, `needs` followed by `north#N`, `#N`,
  `box N`, a sibling box's label such as `B16` or `4-3`, which the box leads with or names first inside parentheses as in
  `(C15)` or `(C20, after C15)`, or a label range such as `C15-C26`, which waits on each labelled sibling inside it; `after`
  is a wait only when such a reference follows it directly, so `after Tom chose` is prose), `(north#8 :follows north#7)` from `Follows:` lines, and
  `(box:... :unparsed-wait TEXT)` for a wait that names a box it cannot resolve. The ingest report prints each unparsed wait
  as `mirror: cannot parse a wait of ...`. `:follows`, `:waits-on` and `:unparsed-wait` are replaced from the mirror.
- `ranking` (writer `ranking`): `(north#633 :rank 1)` from the ranked list in `docs/rationale/backlog-ranking.md` and
  `(north#572 :doc-waits-on box:...)` for each `prerequisite -> dependent` line of its DAG section whose ends read as
  `north#N`, `north#N box K` or `north#N boxes K, L and M`; any other DAG line is printed as `cannot parse a wait in
  backlog-ranking.md`. It reads the file from `~/code/north/main` and the box ids from the mirror, so it runs after `mirror`.
- `ledger` (writers `checker`, `ledger` and `threads`): `(SHA :check-passed|:check-failed COMMAND)` and `(SHA :checked-for north#634)`
  by `checker`, from the ledger's checks; `(SESSION :works-on north#634)` and `(north#634 :landed-as SHA)` by `ledger`; `(OWNER :holds box:...|north#634)` by `threads` from the threads claims table (see Guards).
- `pushes` (writer `push-log`): per pushed commit `:repo`, `:branch`, `:pushed-at`, `(SESSION :pushed SHA)`, and for a
  commit that reached main `:landed-as`, `:landed-by` and `:landed-at`.
- `touches` (writer `git`): `(SHA :touched FILE)` for each commit the push log landed in the last 30 days, from `git log
  --name-only` of that repository's checkout under `~/code/REPO/main`. Facts are only added: a landed commit's files do not change.
- `corrections` (writer `corrections`): `(correction:ID :class|:at|:paraphrase|:in-session|:superseded-by ...)` from the
  grounding ledger and `(class:NAME :class-paraphrase|:class-owner|:class-guard|:class-guarded-at ...)` from
  harness/corrections.toml. The ledger holds Tom's corrections as neutral paraphrases and so does the graph.
- `coverage` (writer `coverage`): `(test:BINARY::NAME :reaches fn:FILE:START-END)`, `:in-binary`, `:in-file` and
  `(coverage:REPO :commit SHA)` from the coverage map, replaced whole by each map.
- `observations` (writers `lander`, `git` and `checker`): `(SHA :landed-on "main")` by `lander` for each commit the push log,
  the ledger's landings or the threads database's `landings` record, and `(SHA :ancestor-of "origin/main")` by `git` for each commit a claim names that
  `git rev-list origin/main` lists in `~/code/REPO/main`. It runs after `mirror` and appends through `Graph::apply`: it reads
  only the live assertions of its two predicates and adds what is new, and never retracts, since a landed commit stays landed.
  `north task check --done-when` records each verdict in the ledger's checks with the main commit it ran on, which `ledger`
  tells as `checker`'s `:check-passed` or `:check-failed`. The host that ran the check also queues it as a `done_check` row on
  its threads spool, so the ledger host's threads database holds every host's verdicts in `done_checks`, and `observations`
  tells them as the same `checker` facts; a fact both sources tell is asserted once.

A worker's claim counts only when a mechanical writer corroborates it. `harness/graph-rules/corroborated.json` derives
`corroborated(BOX, SHA, WRITER)`: a quoted `(BOX :done-at SHA)` claim whose SHA is not `:none`, with `checker` asserting
`(SHA :check-passed CMD)` for the box's own `:done-when` CMD, or with `lander` asserting `(SHA :landed-on "main")` and `git`
asserting `(SHA :ancestor-of "origin/main")`. The closed-box report (`north ledger summary`, dispatch's arm shares, `north ops
climb run`) marks a kept box checker-verified only when the rule holds by `checker` for the box its run's brief named; the
`Evidence:` stamp in the issue body, which any writer of the body can type, is read only on a host with no graph.

A predicate belongs to one source: a run refuses a source that replaces a predicate another source of the run replaces.
A sync reads only the live assertions it can change: every writer's assertions of a replaced predicate, and for each
other predicate it tells, the assertions of the writers it tells it as. The decision log was ingested until
2026-11-08 and nothing read it; each ingest retracts any assertion still live under a writer `graph_retired_writers` lists, and `north graph retain`
deletes its history (see Retention).

### Which host ingests what

The graph is per host. The laptop's is built by `north land follow` after each landing, from every source. The ledger host
(nexus) keeps its own, built every five minutes by south's `north-graph-ingest` timer
(`modules/north-graph-ingest.nix`), so `north graph ready`, dispatch's backlog read and the learning loops that read the graph
run where the ledger, the mirror and the threads database are. It runs
`north graph ingest mirror ranking ledger losses observations` and no other source, each read from a copy the ledger host
already holds: `mirror` from its own `github-mirror.db`, `ranking` from the `~/code/north/main` checkout that checkout-sync
keeps current, `ledger` from its `ledger.db`, `losses` from its threads database, and `observations` from the threads
database's `landings`, which the lander writes there, and `done_checks`, which each host's threads spool carries there, with
the ancestry of claimed commits from the `~/code/REPO/main` checkouts checkout-sync keeps; a repository with no checkout there
leaves a note and no ancestry. A named source that cannot be read fails the unit, which notifies through `update-notify`.

The rest stay laptop-only, so a ledger-host graph holds no fact from them:

- `pushes` and `touches`: the push log (`safe-push/pushes.tsv`) is written where `safe-push` runs, the lander is the laptop,
  and `records-mirror` does not carry it. `:landed-as` and `:works-on` reach the ledger host's graph through `ledger`.
- `corrections`: `grounding/ledger.jsonl` is written and read on the laptop, and no copy is mirrored to the ledger host.
- `coverage`: the coverage map is built on the laptop and no copy is mirrored to the ledger host.

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

## Retention

`north graph retain [--db PATH]` removes history no reader needs, Beagle Store's retention rule
(`crates/north-graph/src/store/retain.rs`). It holds `graph.db.ingest.lock`, so an ingest waits for it, and deletes in
short write transactions (5,000 withdrawal pairs or 10,000 terms each, a few hundred milliseconds), so a writer never waits
past its busy timeout. It deletes:

- every occurrence of a writer listed in `graph_retired_writers` (harness/defaults.toml) whose proposition is not live,
  with the other side of each of its withdrawals and its failed retractions, at any sequence. `north graph ingest`
  retracts what such a writer still holds live, so after an ingest its whole history goes.
- below the horizon, every writer's withdrawn history: an assertion and the retraction that withdrew it when both have a
  sequence below it, and each failed retraction below it, none of which changes any view at or after the horizon.
- every term no kept occurrence, live proposition or kept triple term references.

A pair is deleted in one transaction, so no view sees an assertion without its withdrawal. A term is deleted only if no
occurrence or triple term written since the reach was read references it, checked inside the deleting transaction, so a
writer that interns an old term again keeps it. The newest occurrence is always kept, since SQLite would otherwise give
the next occurrence a sequence a reader has already seen.

The first run sets `auto_vacuum = INCREMENTAL` and runs the one full `VACUUM`, which rewrites the file under the write
lock; every later run reclaims free pages with `PRAGMA incremental_vacuum` in steps of 2,048 pages. Each run then
truncates the WAL.

The horizon is the newest sequence known to be at least `graph_retain_days` (45) days old. Time comes from marks: each
`north graph ingest` and `north graph retain` records the head sequence under `mark/UNIX-SECONDS` in `meta`, at most one an
hour, so the first horizon exists 45 days after a host's first mark. The horizon only rises; it is kept in `meta` as
`retain/horizon` and marks below it go. 45 days covers the longest history reader: the climb archive reads each candidate
`asOf` its landing sequence over dispatch's 30-day arm window, and route replay reads 14 days.

An `asOf` below the horizon, or a `since` whose lower bound is below it, is a `SnapshotExpired` query error, which a reader
must not retry. Views at or after the horizon read what they read before. `occurrence` and `withdrawal` in the current view
hold only the kept history. `validate` reads a store with a horizon as sound and names the horizon in its summary.

Each host that keeps a graph runs it once a day around 4:20 AM local from a user timer: the laptop from firn's
`north-graph-retain` module, the ledger host from south's `north-graph-ingest` module beside its ingest timer. The first run
on a host, which does the full `VACUUM`, is run once by hand. It never runs on the landing path or after each ingest: it
reads every term and every occurrence.

## Guards

`north graph guards DIR` runs every `DIR/*.json` as an `ask`; `north graph validate --guards DIR` does the same after the soundness check. A guard is a forbidden pattern: each row
its query answers is a finding, printed as `guard NAME: ROW`, and any finding exits 1. A query that does not parse or runs out of
budget is an error, so a guard cannot pass by breaking. `--shadow` prints each finding as `would refuse guard NAME: ROW` and
leaves the exit code to the soundness check alone (`guards` then exits 0), so a new guard runs in shadow mode before it refuses.

A guard file may carry two things a plain `ask` does not. `"uses": ["corroborated"]` puts the strata of
`harness/graph-rules/corroborated.json` (the `graph-rules` directory beside DIR) before the guard's own, so a guard reads a
shared rule instead of copying it. `{"ago": 86400}` in place of a constant is the Instant that many seconds before the run
started, one `now` for every guard of a run; `lt` and `gt` order two Instants by time.

Each guard is named for the correction class it finds (`graph_guard` in `harness/corrections.toml`, where every other class
says in `not_structural` why no graph fact holds it), and a class whose guard file is on the harness tree files no guard box in
`north ledger flow detect`. Each guard is a shadow gate in `harness/gates.toml` (`graph-guard-NAME`) until its review is
recorded. The lander runs `north graph guards harness/graph-guards --shadow` as a `land.check`, without the soundness scan that took 12 s on a 500 MB graph, on its own host (`.safe-push`), so it logs each would-be refusal
and refuses nothing until a reviewer has read those lines (north#706 shadow-first rule) and the `--shadow` flag is dropped there; it reads that host's
graph as fresh as its last `north graph ingest`, and a host with no graph answers no rows.

- `dropped-thread`: a box whose wait names something the ingest cannot resolve (`unparsed-wait`), until the box names
  `north#N box K` or a sibling label.
- `unverified-tick`: a worker's Done claim on a ticked box whose commit landed or was pushed over 24 h ago and that
  `corroborated` (a checker passing the box's `Done when:` command at that commit, or the lander's landing on main) does
  not confirm. A claim that names no commit is never corroborated and never found here, since it has no time to wait from.
- `duplicate-claim`: each holder of an unticked box another session also holds. The `ledger` source tells
  `(OWNER :holds box:...)` under writer `threads` for each claim in the threads claims table on an open issue's box (the
  claim's box slug resolved to the one box of the issue whose slug it begins or ends), and `(OWNER :holds north#N)` for a
  claim on a whole issue; `:holds` is replaced each run.

## Ready set

`north graph ready [--db PATH] [--query ready|held]` lists the backlog as a dependency graph, read-only: each unticked box of
an open issue with no live wait, in measured order (`ready N`, issue, its figure and rank, box), then each box held by a wait with
the edge that holds it (`held`, issue, figure and rank, box, `waits on` the box or issue and its text or title). A wait is live while its target box
is unticked or its target issue is open; `:blocked-by`, `:follows` and `:doc-waits-on` on the issue hold each of its boxes.
The measured order puts first the issue whose fix removes the most measured usage per dollar of one of its
boxes: the `losses` ingest source tells each issue the 24 h usage of the harness cause classes it owns (`:loss-usd`, the
classes `north ledger flow` charges, each owned by the issue `harness_loss_owners` in harness/defaults.toml names) and what
one of its boxes costs (`:box-usd`, its worker usage per Done run over 14 days, else the fleet's), and the figure prints as
`7.8 = $9.35 lost 24 h / $1.20 a box`. Priority label, then rank, break ties, so an issue that owns no measured loss keeps
its label and rank order; boxes of issues with no rank come last among equals. `--query ready` and `--query held` print the `north graph ask` query behind each list,
so `north graph ask "$(north graph ready --query ready)"` prints the same rows as tab-separated terms.

`north fleet dispatch` reads the same set (`crates/north-graph-ground/src/backlog.rs`): it orders issues as `north graph ready` lists their
ready boxes, issues with no ready box after them by priority label and rank, and skips an issue whose every unticked product box is held with `no box is ready: box "..." waits on ...`. A box is
ready for this purpose when at least one product box is ready, so a ready Clean-up box never frees an issue whose product
boxes wait. The graph is as fresh as the last ingest, which `north land follow` runs after each landing; when the graph cannot
be read dispatch prints a `note:` and falls back to label and age order and the issue-level waits it reads from text.

A proxy-role session launches a worker only on one of the first five issues of that order, or on an issue it created or
edited in a turn that answers a message Tom typed (the newest prompt of its transcript carries no `[from NAME]` or `[gate ID]`
sender); otherwise `north fleet launch` names the top five and refuses. The rule is the `proxy-launch-rank` gate of
harness/gates.toml, in shadow: it warns the proxy and appends each would-be refusal to
`$XDG_STATE_HOME/north/gates/proxy-launch-rank.jsonl`. A launch whose Item names no issue (a worker that files its own, a red main to repair) is outside the rule (north#706). Dispatch's own launches run without the caller's fleet session, so
the rule never reads them as the proxy's.

## Grounding

The UserPromptSubmit grounding hook and `north work brief new --category C --item REPO#N` read this graph through
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
