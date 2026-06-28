# Multi-Agent Coordination on the Claim Graph — An Honest Assessment

*Audience: the architect. Yardstick: does each primitive exploit the append-only claim graph's actual differentiators (coexistence, reification, derived views, content-addressed predicates), or does it port a lock/queue/mailbox/heartbeat/counter from a data structure fram deliberately isn't? Two properties are explicitly NOT differentiators and don't count as evidence of graph-nativeness: single-writer/OCC concurrency (borrowed from Datomic) and Datalog query power (datom & RDF stores match it) — per `~/code/fram/docs/WHY_FRAM_EXISTS.md` §5.*

---

## 1. THE PICTURE — how coordination works right now

The substrate is one append-only claim log behind a single-writer coordinator on `:7977` (presence on `:7978` — a split, see below). On this log, **writes never conflict**: `base + A + B` keeps everything in A and everything in B. The *only* thing that forces two writers to disagree is a **cardinality axiom** — a `(subject,predicate)` declared `single` — and the only one the substrate is globally forced to assert is **id-minting** (distinct things → distinct ids). Everything else coexists, and "current state" is a read-time subtraction (`live? = not superseded?`), never a stored field.

The coordination tier built on top splits cleanly into two maturity levels:

- **The intent/thread spine** (`~/code/lodestar/docs/operating-manual.md`, `the-model.md`) is the system working as designed: orthogonal axes stored as claims (`committed`/`outcome`/`driver`/`depends_on`/`part_of`), every lifecycle condition (`ready`/`blocked`/`active`/`done`) **derived** by a shared classifier, no status field anywhere. This is the gravity well the rest should fall into.

- **The coordination CLIs** (`~/code/lodestar/cli/*.clj`) are an *accreted* layer — `auto-coordination-plan.md` admits lodestar was "all substrate, no loop," and the reactive machinery (reactor, command envelope, work-claim, budget) was bolted on in phases CLI-by-CLI. Each `*-cli.clj` invents its own predicate family inline; there is no central schema and no cardinality registry, so single-valuedness is faked procedurally per-CLI.

The mental model in one line: **agents coordinate stigmergically by asserting claims and letting derivation answer "whose turn / what's ready / who overlaps" — except at a handful of seams where the code reflexively reaches for a mutex, a counter, or an opaque blob instead.** The good half genuinely dissolves coordination into the graph (concerns coexist, inbox is a derived query, subscription scope is a claim-set). The bad half re-imports exactly the write-time conflicts append-only was built to eliminate.

The single shared exclusion primitive is fram's **lease** (`cnf_coord.clj`), held inside the daemon lock. It legitimately backs id-minting-adjacent needs but is over-applied: it currently underpins presence liveness, exclusive roles, thread-driver claims, and swarm slots — three of which are graph-internal state that would coexist fine.

---

## 2. GRAPH-NATIVE SCORECARD

| Primitive | Verdict | One-line why |
|---|---|---|
| **Intent/thread model** (`operating-manual.md`) | graph-native | The yardstick itself: orthogonal axes as claims, all lifecycle derived, no status field. |
| Concern declare/overlap/shape (`concern-cli.clj`) | mixed | Coexistence + touches-overlap derivation is the best design in the set; `status` via `set-single!` is a faked SQL status column. |
| Messaging-as-claims (`msg-cli.clj`) | mixed | Encoding is native (immutable msgs, ack-as-append, derived inbox); the *mailbox metaphor* beside the work graph is ported. |
| Command envelope `{:op :args}` (`msg-cli.clj`+`lodestar-listen.clj`) | cargo-culted | Opaque RPC blob in one `body` cell; parser duplicated across two files "MUST stay in sync." |
| Presence lease (`presence-cli.clj`) | mixed | `online?`=derived is a real win; the `lease` vehicle + ~3min renew is a heartbeat by another name, packed into an opaque `holder\|exp\|epoch` string. |
| Lease family (`lease-cli.clj`) | mixed | Faithful fenced mutex — legitimate only for *external* resources (build dir, API); applied to graph-internal state it re-imports write conflict. |
| Work-claim driver+`@lease:<thread>` (`claim-cli.clj`) | mixed | "One driver" is a legitimate single-cardinality *policy*; the separate lease cell beside the `driver` claim is `SELECT FOR UPDATE`. |
| Role registry + exclusive lease (`presence-cli.clj`) | mixed | Roles/holds/reverse-edge routing is textbook; exclusive-role lease is a mutex cell beside what is really a single-cardinality claim. |
| Agent card (`presence-cli.clj`) | mixed | Identity-as-roles + derived staleness bucket is right; `needs_rotation`/`last_run_at`/`generation` are mutable cells shadowing the `@run` log. |
| Focus pointer (`presence-cli.clj`) | graph-native | Latest assertion + read-time fallback; could go further and *derive* from driver activity. |
| Pin (`presence-cli.clj`) | graph-native | A human-policy attribute claim read into sort/staleness; trivially native. |
| Staleness scorer (`presence-cli.clj`) | graph-native | Pure read/derive, zero writes; only the hardcoded weights aren't yet claims. |
| Subscription scope (`lodestar-listen.clj`) | graph-native | Scope = `uuid ∪ roles ∪ * ∪ watched`, derived from claims, live-reshapes with no reconnect. One of the cleanest. |
| Listener pub/sub (`lodestar-listen.clj`) | graph-native | Tails the immutable commit log with a derived filter; zero poll, zero tokens. |
| Reactor `--react` (`lodestar-listen.clj`) | mixed | The loop is a forward-chaining rule (good); it dispatches by parsing the opaque envelope (inherits the cargo-cult). |
| Swarm budget `:bump` + slots (`lodestar-listen.clj`,`budget.ts`) | cargo-culted | Mutable atomic counter + counting semaphore; both fight append-only when `@run` cost claims already exist. |
| Run/telemetry `@run:<sid>` (`presence-cli.clj`) | graph-native | Immutable append-only event log — the template the budget should imitate. Minor `agent` predicate overload. |
| Reply-schema JSON gate (`schema-validate.clj`) | cargo-culted | A third schema language; opaque JSON blob validated by a sidecar that duplicates the kernel's own commit-time rule-check. |
| Message schema attach (`msg-cli.clj`) | mixed | Schema-as-claim-on-message is nice reification; the schema itself is graph-opaque JSON. |
| inputChannel (`sdk/coordination.ts`) | graph-native | Correctly ephemeral in-process delivery — knowing where the substrate *stops* is good design. |
| subscribeFeed (`sdk/coordination.ts`) | mixed | Re-arm loop is fine plumbing; it **regex-scrapes the listener's rendered text** instead of reading claims — the parse-the-view anti-pattern. |
| lodestar-on-spawn (`bin/lodestar-on-spawn`) | graph-native* | Bootstraps the right model, but registers presence on `:7978` while everyone reads `:7977` — a partition bug, not a cargo-cult. |
| lodestar-arm (`bin/lodestar-arm`) | graph-native | Pure wrapper; inherits the listener's verdict. |
| **Code-as-claims** | | |
| beagle-claims / roundtrip projections | graph-native | Lossy query overlay + lossless graph-as-truth; render(log) is a pure function. |
| THE FLIP (`fram-code-on`/`fram-ingest-code`) | graph-native | Re-keys positional ids to stable `@mod#n` identity; demotes text to a generated view. |
| Warm `:edit-min` wire | graph-native | In-flight edit is shared queryable claims *before* render/merge; disjoint edits commute. The flagship. |
| **OCC global `:version` commit** (`fram-commit-code`) | mixed | Global compare-and-swap treats the whole log as one row; disjoint commuting writes still collide and retry. The main code-area cargo-cult. |
| callgraph blast-radius (`chartroom/callgraph.clj`) | mixed | Right derived answer, but rebuilds a throwaway store per query — the reconstruction tax the canonical graph abolishes. |
| Warm `:callers`/`:query` reads | graph-native | Datalog over the live store agents edit; sees coexisting in-flight claims with no merge. |
| rep_jurisdiction decision-blast | graph-native | A compiler decision reified per-def and joined to the call graph — reification, not mere Datalog. |
| Cross-frame bridge claim (GATE 4) | graph-native | Code node + coordination thread in ONE id-space; the literal coordination seam. |
| Supersession-as-claim / identity refs / CRDT order keys | graph-native | Divergence at the storage layer; CRDT logoot keys are the *opposite* of a cargo-culted mutex. |
| select-main-1 / pred-val | graph-native | Refuses a stored status; current-value is a named read-time selection — but today's policy is placeholder `(first ...)`. |
| Views & Branches model (`VIEWS_AND_BRANCHES.md`) | graph-native (unbuilt) | The purest statement of every strength — and the single highest-leverage *unbuilt* keystone. |

---

## 3. CARGO-CULT FINDINGS — ported patterns that fight append-only, with replacements

Every finding below is a place we forced convergence the substrate doesn't require, or stored what it would derive.

### 3.1 The command envelope — RPC blob in a cell (`msg-cli.clj` L40-52, `lodestar-listen.clj` L73-81)
The clearest port. A command is an opaque EDN string `{:op :args}` stuffed into one `body` claim — the graph cannot see, query, supersede, or attach provenance to the op/args. The `parse-envelope`/`known-ops` parser is **literally duplicated across two files with a "MUST stay in sync" comment** — the antithesis of single-source content-addressing.
**Replacement:** model the command as claims on a subject — `@cmd:<id> op :spawn · prompt "…" · target @agent`, `pending` = no `acked_by`. The reactor matches `(… op :spawn)` *landing* via Datalog; `known-ops` becomes closed-vocab claims the coordinator validates at commit; both parser copies are deleted; "all pending :spawn commands" becomes a native query. This one fix also repairs the reactor (drives off claim-patterns, not string parsing).

### 3.2 Swarm budget — mutable counter shadowing an append-only log (`lodestar-listen.clj` L39-47, `budget.ts` L62-69)
`budget_spent` is accumulated via `:bump` — a literal `UPDATE SET spent = spent + n` on one shared cell — even though **each run already asserts its cost as an immutable `@run:<sid> cost_usd` claim** (`presence-cli.clj` L278-283). The counter duplicates derivable state, throws away per-charge provenance, and must stay in sync with `budget.ts`. The `@swarm-slot:1..N` pool is a counting semaphore the code's own comment calls "non-binding."
**Replacement:** `remaining = budget_total − Σ(@run cost claims)` — a Datalog aggregate, no mutable cell, full who-spent-what audit for free. Delete `:bump` and `budget_spent`. Delete the slot semaphore; if a concurrency ceiling is ever wanted, derive `count(live drivers)`, keeping a lease TTL only as crash-safety. **This is the cleanest "stop mutating a cell, start summing claims" case in the system.**

### 3.3 Reply-schema gate — a third schema language + sidecar validator (`schema-validate.clj`)
An opaque JSON-Schema string in a claim, validating an opaque JSON payload blob — an RPC response-contract pattern that **reimplements a JSON-Schema engine duplicating the kernel's existing commit-time validation** (closed-vocab/cardinality/dangling-ref rejection).
**Replacement:** replies are *claims*; conformance is the coordinator's already-existing commit rule-check (a rejected claim IS the invalid reply). If a per-task shape is genuinely needed, reify the contract as claims (`@contract:<id> requires <pred> · expects <cardinality>`) so it is queryable/versionable/disputable — no second validator, no third language.

### 3.4 Concern `status` — a faked SQL status column (`concern-cli.clj` L49-52)
`set-single!` does retract-all-then-assert to simulate single-cardinality in client code — **not atomic**, so two concurrent status changes interleave and reintroduce the very write-time disagreement append-only eliminates. It also erases history.
**Replacement:** status is a lifecycle — derive it. Assert monotonic maturity claims (`reached likely-to-land at T`, `landed at T`); the latest assertion *is* the status, read latest-wins by supersession. Free status history; never a retract to "change." Leave the coexistence/overlap spine untouched — it's the best thing in the set.

### 3.5 Work-claim & exclusive-role — a mutex cell beside the claim (`claim-cli.clj` L20-29; `presence-cli.clj` L114-122)
Both maintain a separate lease cell AND the durable claim (`driver`/`holds`) — two mechanisms for one fact, with the lease denying the loser at write time. "One driver" / "one holder" are legitimate single-cardinality *policies*, but `SELECT FOR UPDATE` beside the row is the tell.
**Replacement:** declare `(thread, driver)` and exclusive `holds` **single in the kernel**, so an atomic assert-if-absent under OCC *is* the lock — the loser reads the existing holder and backs off. One cardinality-enforced claim, no parallel lease. (Even purer for driver: let drivers coexist + derive a `contested` view + deterministic yield by lowest agent-id — choose this where dup work is cheaper than coordination.)

### 3.6 Presence liveness — a heartbeat wearing a lease (`presence-cli.clj` L70-79)
`session:<h>` is never contended, so the lease degenerates to a self-expiring claim, and "renew every ~3min" is a heartbeat by another name. The value is an opaque packed `holder|exp|epoch` string Datalog can't reason over.
**Replacement:** model liveness as a plain self-expiring claim (`alive_until <inst>`, renewed by re-assert) in the ephemeral fleet jurisdiction; `online?` is a pure derived view; `exp` is a number, not a decoded substring. Reserve real leases for genuine *external* exclusion.

### 3.7 subscribeFeed — screen-scraping the rendered view (`sdk/coordination.ts` L54-56)
Recovers structured `from/subject/body` by **regex-scraping the listener's human-formatted stdout** — re-deriving structure thrown away at render. The listener already holds the message entity id.
**Replacement:** emit/consume a machine-readable EDN line (or entity id + claim read); never regex over printed text. A persistent structured channel also retires the `--once` respawn dance (which exists only to defeat stdout buffering).

### 3.8 Code-area: global-version OCC (`fram-commit-code` L19,137-138) + per-query store rebuild (`callgraph.clj` L86-127)
The global `:version` CAS treats the whole log as one row: two writes to **disjoint `(te,p)` groups — which the substrate guarantees both land** — still collide on base-version and one retries. Retry hides it; the granularity is simply wrong for the commuting multi-valued AST majority. Separately, blast-radius rebuilds a fresh throwaway store per query (`c/new-store`, re-assert all edges) — the reconstruction tax the canonical-incremental graph is meant to abolish.
**Replacement:** make conflict detection per-`(subject,predicate)`-cardinality, not per-global-version — reject only a write to a declared-`single` `(te,p)` whose live value moved, or an id collision; everything multi-valued just appends. And run reaches-closure directly over the warm store (`:query` path), so blast re-derives live and incrementally.

### 3.9 Wiring bug (not a cargo-cult, but a single-source violation): port split
`lodestar-on-spawn` registers presence on `:7978` (L20) while concern/claim/msg/listen and `coordination.ts` all default to `:7977`. Two ports risk **partitioning the one canonical log** — presence written where consumers aren't reading. (`presence-cli` still reaches back to `:7977` for playbook counts, so the boundary already leaks.) Fix the split or make the jurisdiction boundary deliberate and complete, not accidental.

---

## 4. CODE-AS-CLAIMS — why coordinating over the code graph beats messages

**The thesis (and it's real, GATE-verified):** the flip (`fram-code-on`) turns a Beagle tree into one shared, warm, sole-writer claim log. Every agent edits *through* that coordinator via `:edit-min` (`fram-edit-code` L145-199), so an in-flight edit becomes **addressable claims the instant it commits — before it is rendered to `.bclj` or merged to main.** Peers then ask relational questions over the *same* store:

- "Who breaks if I change D?" = `blast(D)` transitive callers via Datalog reaches-closure (`callgraph.clj`), scope-correct (binds the defn in its *own* module — a measured 33–67% precision win over bare-symbol grep).
- "What is another agent likely to land?" = a **live `:callers`/`:query` over the warm store** (`cnf_code_flip_test.clj` GATE 5), which sees peers' committed-but-unrendered claims — no merge step, no guessing against text.
- "What's downstream of a feature *decision*?" = reify the decision per-def (`[def rep-regime hamt]`) and join it to the call graph (`rep_jurisdiction.clj`) — a query a per-module comment structurally cannot answer.

**Why this beats messages:** a message says "I'm touching the parser" — a string a peer must interpret, that goes stale, that can't be queried for overlap. A code claim *is* the touch: `@parser#42` superseded, queryable by identity, surviving rename (references carry identity, not spelling — `resolve.clj`). Overlap stops being "did our prose footprints sound similar" and becomes "does my blast radius intersect yours" — a Datalog join. And because code claims and lodestar coordination claims share **one id-space** (GATE 4 cross-frame bridge, `cnf_code_flip_test.clj` L154-182), a concern can cite a specific in-flight *node*, not a file path.

**This is the sharpest unrealized lever in the whole system.** Today `~/code/lodestar/bin/concern` matches **file-path strings** — when the graph-native key (node identity) is right there, scope-correct, and rename-stable. The single most valuable code-area move is to reimplement concern footprint/overlap as **bridge-claim queries**: a concern `relates_to` code nodes, and "who else is in my footprint" = `relates_to`-from + blast intersection.

**How far are we?** The storage/edit/reference layer is *distinctively* claim-native and well-built — identity refs, supersession-as-claim, CRDT logoot order keys, reified per-def decisions, the one-graph bridge. The gap is two table-stakes borrowings and one unbuilt keystone:
1. **Global-version OCC** rejects disjoint/rival commits that should both land (§3.8).
2. **Per-query store rebuilds** pay reconstruction tax the warm store abolishes (§3.8).
3. **First-class views are designed but unbuilt.** `VIEWS_AND_BRANCHES.md` is explicit: "nothing here is built yet." Today there is exactly ONE view = `main` = `select-main-1`'s placeholder `(first cids)`. So divergent in-flight features coexist only inside main's global superseded-fold — there is **no per-agent branch isolation**. The substrate is view-*capable* (it stores the divergence) but not view-*partitioned*.

Crucially, **both cargo-cults and the missing isolation converge on the same fix**: build the view-as-claim encoding. Then OCC relaxes to per-cardinality conflict, `:edit-min` can land rival same-target edits as divergent claims, supersession/`select-main-1` expose coexisting tips instead of collapsing to first-live, and all derivation routes through the warm store.

---

## 5. THE SMARTEST-POSSIBLE DESIGN — the north star

A graph-native multi-agent coordinator, taken to its logical end, has these properties:

1. **Cardinality is a kernel-declared property of `(subject,predicate)` pairs.** This is the keystone. When the design needs "exactly one," it declares `single` and lets the coordinator enforce it atomically *on the claim* — an assert-if-absent under OCC. The claim *is* the lock. This single move collapses work-claim, exclusive-role, and concern-status from "lock/counter cell beside a claim" down to "the claim is the lock," and lets you then delete every parallel lease cell, the `:bump` counter, and the swarm-slot semaphore.

2. **Leases survive only for exclusion over things *outside* the graph** — the build directory, a non-idempotent external API call. That is the one place append-only genuinely doesn't help and fencing (epoch) earns its keep. Every graph-internal "must serialize" is a declared-single claim instead.

3. **Convergence is pushed to the reader.** Where forcing one value isn't worth it, divergent claims coexist and a **deterministic read-time election** (lowest agent-id, earliest cid) picks the winner — every reader agrees because the tiebreak is a function of the claims, and nothing ever blocks. The lease, if kept at all, is demoted to an optional double-spawn window-shrinker.

4. **Everything countable is summed, never mutated.** Budget = `Σ @run cost`. Liveness = derived from a TTL'd claim. Staleness inputs = `max(last_run_at)`/`count(generation)` over the `@run` log. No counter ever shadows the event log that already holds the truth.

5. **Commands, contracts, and replies are first-class claims, not blobs.** A command is a subject you can query/supersede/dispute/audit; a reply is claims validated at commit by the one rule-check; a contract is reified claims. No second schema language, no duplicated parser, no sidecar validator.

6. **Coordination is over the code graph, not beside it.** Footprint/overlap are blast-radius joins over node identities (§4), not file-path string matches or prose messages. The message family shrinks to the thin directed-dispatch residual `the-model.md` §5 predicts.

7. **Views are first-class.** A per-agent or per-branch context is `(view selects @claim)` — just more claims in the same graph. In-flight features get real isolation without a stored status, and merge stays a discretionary reader choice.

**The gap from today** is not architecture — the intent spine and the code-as-claims layer prove the design is understood. The gap is that the *coordination CLIs were accreted phase-by-phase before the kernel grew declared-cardinality*, so each one independently reached for the nearest borrowed primitive. Close that one kernel gap and roughly half the cargo-cults delete themselves.

---

## 6. RECOMMENDATIONS — prioritized

**Tier 1 — the keystones (each unlocks several deletions):**

1. **Add kernel-declared cardinality on `(subject,predicate)`.** Highest leverage in the system. Collapses work-claim (`claim-cli.clj`), exclusive-role (`presence-cli.clj`), and concern-status into "the claim is the lock." Then delete `@lease:<thread>`, the exclusive-role lease, and `set-single!`. Reserve leases (`lease-cli.clj`) for external resources only.

2. **Build the view-as-claim encoding** (`VIEWS_AND_BRANCHES.md`, attach at `select-main-1`/`resolve.clj` L92-112). Cheapest of the three CNF-native encodings, and it's the code-area keystone: it lets OCC relax to per-cardinality conflict, lets `:edit-min` land rival edits, and exposes coexisting tips. Sequence: view-as-claim → re-target `select-main-1` → relax OCC.

3. **Replace the command envelope with command-as-claims** (`msg-cli.clj`, `lodestar-listen.clj`). Deletes both duplicated `parse-envelope` copies, makes the reactor a pure forward-chaining rule, and makes commands queryable/reifiable. Fixes the reactor and (with §4) the schema-gate in one stroke.

**Tier 2 — high-value, self-contained:**

4. **Make budget a derived sum over `@run` cost claims; delete `:bump` and `budget_spent`** (`lodestar-listen.clj`, `budget.ts`). Delete the `@swarm-slot` semaphore; derive a live-run count if a ceiling is ever needed. Removes cross-file sync and recovers a spend audit trail.

5. **Reimplement concern footprint/overlap as bridge-claim queries** over code-node identity instead of file-path strings (`concern-cli.clj` → use the GATE-4 bridge). Scope-correct, rename-stable. The clearest place the system coordinates over a non-graph key when the graph-native key exists.

6. **Relax code-commit OCC to per-cardinality conflict** (`fram-commit-code`); always take the `:edit-min` path and demote the whole-module re-commit to a test oracle.

7. **Route blast-radius/decision queries through the warm `:query` store** (`callgraph.clj`, `rep_jurisdiction.clj`); delete the per-query throwaway stores. Add a materialized view for the hot "blast of the node I'm editing," refreshed incrementally on commit.

**Tier 3 — cleanups, low risk:**

8. **Replace the reply-schema sidecar with claim-replies validated at commit** (`schema-validate.clj`); reify any bespoke contract as claims.

9. **Replace the presence lease with a TTL'd `alive_until` claim** in the fleet jurisdiction (`presence-cli.clj`); derive `online?`; kill the opaque packed-string decode.

10. **Move staleness inputs off the agent card and derive them from `@run`** (`presence-cli.clj`); turn `needs_rotation` into a derived condition and route rotation through the reactor as a derived obligation rather than an inline `compact.sh` shell-out.

11. **Emit structured EDN from the listener; stop regex-scraping it** in `subscribeFeed` (`sdk/coordination.ts`); this also retires the `--once` respawn dance.

12. **Fix the `:7978`→`:7977` presence partition** (`bin/lodestar-on-spawn`) — or make the jurisdiction split deliberate and complete.

13. **Factor the copy-pasted `send-op`/`assert!`/OCC-retry helper** into one shared module (duplicated verbatim across ≥5 `*-cli.clj`); disambiguate the `agent` predicate overload between `@session:*` and `@run:*` (`ran_by`/`run_of`) so the roster needn't prefix-filter.

**Bottom line:** the system is mostly graph-native where it counts — the intent spine, code-as-claims storage/edit/reference layer, concern coexistence, subscription scope, and the `@run` log are genuinely distinctive. The cargo-cults are real but *localized*, and they cluster on a single root cause: the kernel can't yet declare cardinality, so the coordination tier reaches for a lock/counter/blob every time it needs "exactly one," "limit N," or "a command." Add declared cardinality and build views; about half the borrowed machinery deletes itself, and the rest is mechanical cleanup.

---

## Critic's addendum: gaps & second opinions

The assessment is strong on the lock/counter/blob axis, but it has a category-sized blind spot, one internal contradiction in its own keystone, and several substrate properties it names but never spends. Grounded findings first (I verified these in-repo), then the second opinions.

### A. Verified misses (the scorecard is incomplete, not just debatable)

**1. The entire fan-in / barrier / quorum family is missing from the inventory.** `~/code/lodestar/cli/lodestar-map.clj` implements a fan-out/fan-in coordinator: `@batch:<id>` with `expected_count=N`, `barrier_k=K`, and `@done:<id>:<worker>` claims, where "K-of-N DONE" is a **derived Datalog count of distinct workers**, not a mutable counter, and a rejected payload simply never asserts a `@done` claim so the barrier monotonically can't advance. The assessment scored ~40 primitives on the exclusion/counting/messaging axes and never once addressed **completion / join / quorum** — the dual of exclusion and arguably the more important primitive for swarms. Worse, this primitive is *already the graph-native pattern the doc keeps prescribing elsewhere* (count distinct claims, don't mutate a cell; monotone progress; idempotent double-report collapse via `distinct`). It deserved a green check and should have been cited as the template — its omission weakens §3.2's case by hiding the fact that the right design already ships.

**2. The budget "replacement" already exists in the repo — the finding is mis-framed.** §3.2 proposes `remaining = budget_total − Σ(@run cost)` as if unbuilt. But `~/code/lodestar/cli/lodestar-reconcile.clj` *already* does exactly that — `(reduce + 0 costs)` over `cost_usd` claims via Datalog. So the real situation is worse and sharper than the doc says: there are **two live budget mechanisms** — the derived-sum reconciler AND the mutable `:bump` counter in `~/code/lodestar/sdk/src/budget.ts` / `lodestar-listen.clj` — and they can silently disagree. The recommendation isn't "build the sum," it's "delete the counter; the sum is already authoritative and the counter is now a divergent shadow." The doc missed the duplication entirely.

**3. Global-version OCC is the UNIVERSAL write path, not a code-area problem.** §3.8 localizes whole-log `:version` CAS to `fram-commit-code`. But every coordination CLI's `assert!` (`msg-cli.clj`, `concern-cli.clj`, `claim-cli.clj`, `inbox-peek.clj`, `lodestar-map.clj`, `lodestar-listen.clj`) is literally `OCC at current :version; retry on reject` against **one global version for the whole log**. So the disjoint-writes-shouldn't-collide critique applies system-wide, and the "best design in the set" (concern coexistence) still funnels each assert through a global CAS. This is a generalization error: the code-area cargo-cult §3.8 names is actually the substrate's only write primitive.

### B. The keystone contradicts the yardstick

**4. "The claim is the lock via assert-if-absent under OCC" is the disqualified primitive wearing a graph hat.** The yardstick (§intro) explicitly rules single-writer/OCC out as a non-differentiator. Yet Tier-1.1 — the highest-leverage recommendation — *is* OCC: declared-single + atomic assert-if-absent is compare-and-swap relocated into the kernel. It still forces write-time convergence, the exact thing append-only was meant to dissolve. The doc grades external-resource leases as honest mutexes but blesses this one because it's "in the kernel." The genuinely native answer is the option the doc mentions only in passing (§3.5 parenthetical): **let drivers coexist, derive a `contested` view, elect deterministically by lowest agent-id**. That should have been the keystone, with single-cardinality-lock demoted to the pragmatic compromise for the rare case where dup work truly costs more than coordination. The purity ranking is inverted.

**5. Cardinality should be a claim, not a kernel property — and the doc's own §5.7 implies this.** §5.1 wants cardinality kernel-declared; §5.7 wants views first-class. These collide: if cardinality is a kernel flag it is global and view-invariant, but `~/code/fram/docs/WHY_FRAM_EXISTS.md` (reflexive id-space, "reify without bound") and the archived `bridge.clj` (`cardinality` already modeled as an *override claim* on a predicate) both point at `(predicate P) cardinality single` **as a claim** — queryable, disputable, and *view-relative*. That's strictly more powerful: a branch can hold a different cardinality rule than main. The doc undersells its own substrate by hard-coding the keystone into the kernel.

### C. Substrate properties named but never spent

**6. Content-addressing → idempotency/exactly-once is claimed as a differentiator and never exploited.** The command-as-claims fix (§3.1) would get retry-idempotency and natural dedup *for free* if a command's identity is the content hash of `(op,args,target)` — a re-sent spawn is the same id, so "exactly-once command delivery" needs no dedup table. The doc never connects content-addressing to the command/reactor problem, even though `lodestar-map.clj` already demonstrates the pattern (barrier counts distinct workers, collapsing double-reports). Idempotent command intake is the missing third leg of §3.1.

**7. No causality / as-of / happens-before primitive.** Stigmergic coordination needs "did peer B's claim exist in the view A read before A acted?" Append-only + per-claim reification makes this trivial — reify `caused_by` edges or just query the log *as-of* a cid — yet the doc never proposes time-travel as a **conflict-resolution** mechanism. This is the signature append-only superpower, and it's the native alternative to locking the driver: don't prevent the collision, *replay to see who had the earlier causal view* and let both readers agree. Absent entirely.

**8. "Derive everything" has an unpriced read-amplification / unbounded-fold cost.** The doc bills per-query store rebuilds as "reconstruction tax" (§3.8) but never notices the identical tax on its own prescriptions: budget-as-Σ is O(history) per spawn decision, `online?`-as-derived re-scans, status-as-latest-assertion re-folds. There is **no compaction, snapshot, or "agent joining at hour 100 doesn't re-fold 100 hours" story** (`VIEWS_AND_BRANCHES.md` gestures at "keep one head materialized" but the coordination tier has nothing). The intent spine survives only because it's small. A complete assessment must say where the materialized/incremental boundary is, or budget-as-sum becomes the new bottleneck.

**9. Retraction / tombstone / "I was wrong" semantics are unaddressed for multi-valued claims.** Supersession gives latest-wins for single-cardinality, but the doc celebrates multi-valued coexistence without confronting how you *cancel* one coexisting claim (a withdrawn command, a retracted overlap declaration). `set-single!`'s retract-then-assert is correctly flagged, but the general add-wins/remove-wins CRDT question — and whether the engine even has principled multi-valued retraction vs. just append — is never raised. This is a real coordination gap (cancellation is a coordination primitive).

### D. Second opinions

**10. The single-writer coordinator on `:7977` is the largest un-interrogated "force convergence through one serialization point" in the whole system** — and this is a *multi-agent* assessment. The doc brackets single-writer off as Datomic-borrowed, but the code layer already runs CRDT logoot order keys, i.e. the substrate is *capable* of multi-master merge. The irony the assessment never states: it audits a dozen small mutexes while the global write funnel — the architecture-level mutex — goes unquestioned. At minimum this deserves a "why one writer is acceptable here" defense.

**11. The green checks are self-graded with no falsifier.** The doc bills itself "honest" yet several "graph-native" verdicts assert *behavior* that reads like design intent: subscription scope "live-reshapes with no reconnect," listener is "zero poll, zero tokens," focus pointer "read-time fallback." None are evidence-backed the way §4's code GATEs are, and the lone empirical number ("33–67% precision win") is a single benchmark stretched to a range with no n, corpus, or baseline stated. A completeness pass should mark which scorecard rows are *verified* vs *aspirational* — the same epistemic discipline the doc rightly applies to `VIEWS_AND_BRANCHES.md` ("nothing here is built yet").

**12. Predicate/vocabulary evolution has no story.** The doc flags the `agent` overload once but the deeper issue — each CLI mints predicates inline with "no central schema" — needs a *migration* answer in a content-addressed id-space: a renamed predicate is a new interned value, so old claims need `same_as`/alias claims or they silently fall out of derived views. Closed-vocab-as-claims (proposed for commands) should generalize to a predicate registry with aliasing, or the accretion the doc diagnoses will keep recurring.

**Net:** the assessment's spine is right, but it audited the exclusion/counting/messaging axes thoroughly while omitting the **completion/quorum axis entirely** (and that axis already contains the repo's best graph-native primitive), it built its Tier-1 keystone on the very OCC it disqualified, and it priced reconstruction cost for queries while ignoring the same cost in its own derive-everything prescriptions. Files to look at that the doc doesn't cite: `~/code/lodestar/cli/lodestar-map.clj` (barrier family), `~/code/lodestar/cli/lodestar-reconcile.clj` (the budget sum that already exists).
