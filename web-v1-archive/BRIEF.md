# lodestar / web — BRIEF

A **standalone web app** that visualizes ANY Fram claim-graph as an interactive **graph field**:
explore it, click nodes for info, color-code it constructively, manipulate it structurally, watch it
update live. Tom wants it to be genuinely nice to look at and fun to poke at.

You are `@web` (exclusive role). Coordinate with `@coordinator` (NEVER Tom). Report a
runnable localhost URL when v1 renders.

---

## The data — what a Fram graph IS

A Fram daemon listens on a TCP port (`:7978` = agent coordination, `:7977` = lodestar / Tom's life-os)
speaking **line-delimited EDN** (one request per connection, one EDN line back).

- Claims are **triples**: `(subject predicate object)`.
  - `subject` = an id: `@agent:9509c8…`, `@msg:2026…`, `@role:fram-engine`, `@2026-06-22-101500` (thread).
  - `object` = a **ref** (`@`-prefixed id, whitespace-free) = an EDGE; OR an EDN **literal** = a node ATTRIBUTE.
- So: a **node** = any id; an **edge** = a triple whose object is a ref; a **literal triple** = a node attribute.
- Lifecycle is DERIVED from claims, never stored: `committed` (accepted), `outcome` (done), `abandoned`
  (canceled), `driver` (active), `depends_on` (blocked). Color these where the thread vocab is present.

**Wire ops** (open TCP to `127.0.0.1:<port>`, write `<edn>\n`, read one `<edn>\n`):
- `{:op :version}` → `{:version N}`
- `{:op :query :query {:find "x" :rules [{:head {:rel "x" :args [...]} :body [{:rel "triple" :args [{:var "s"} "pred" {:var "o"}]}]}]}}` → `{:ok [tuples]}`. Base relations: `triple(s,p,o)` and `claim(cid,s,p,o)`.
- `{:op :resolved :te "@id" :p "pred"}` → `{:value v}` (single) or `{:values [..]}` (multi).
- `{:op :assert :te "@id" :p "pred" :r "val" :base <version>}` / `{:op :retract …}` → commit (structural edit).
- `{:op :subscribe}` → the daemon then PUSHES every commit as a line `{:event :commit :op "assert"|"retract" :l <subj> :p <pred> :r <obj> :version N}` (this is the LIVE feed — the firehose; filter client-side).

**Working client code to copy the socket pattern from:** `~/code/beagle/.scratch/presence-cli.clj`,
`msg-cli.clj`, `lodestar-listen.clj` (the subscribe/live-feed pattern is in fleet-listen).

To dump a whole graph: there is no single "get all" op — use `:query` over `triple(s,p,o)` with all-var
args to pull every triple, then assemble nodes/edges/attrs client-side. Confirm the exact query shape
against the daemon (`cnf_coord_daemon.clj` in `~/code/fram`) before building on it.

---

## Architecture (decided — don't re-litigate)

- **`bridge/`** — a **babashka** server (browser cannot speak raw TCP). Endpoints, each takes `?port=7978`:
  - `GET  /graph?port=N`   → full snapshot as JSON `{nodes:[{id,type,attrs}], edges:[{from,pred,to}]}`.
  - `WS   /live?port=N`    → opens `:subscribe` to that daemon, pushes each commit as JSON to the browser.
  - `POST /assert` `/retract` (JSON body) → structural manipulation (v2; stub the route in v1).
  - Bridge is the ONLY thing that talks TCP; it targets ANY daemon by port. Use http-kit (bundled in bb)
    for HTTP + server WebSocket.
- **`web/`** — browser frontend, rendered with **Cytoscape.js** (mature: interaction + styling + layouts +
  a manipulation API). Plain `index.html` + JS + the cytoscape CDN/vendored lib is fine for v1.
  - Force-directed layout (`fcose` or `cola`).
  - **Click a node** → side panel listing all its claims (`pred → object`); `@ref` objects are clickable
    to recenter on that node.
  - **Color-code**: by node TYPE (the id prefix: `agent`/`msg`/`role`/thread-date/…), with a legend +
    toggle to recolor by predicate, and by derived lifecycle for thread graphs. Make the palette tasteful
    (dark bg, high-contrast accents).
  - **Live**: the `/live` WS animates nodes/edges appearing/disappearing and flashes a node on change.
    This is the "fun" — the graph should feel alive.
- **`bin/lodestar-web`** — ONE command: start the bridge + serve `web/`, print a `http://localhost:PORT` URL.

## Beagle dogfood (Tom's explicit ask: use the compiler as a library)

Author the **pure graph-domain logic in beagle-js** (compile to JS) — it is exactly the typed, pure
transform beagle-js is good at:
- `claims → {nodes, edges, attrs}` assembly,
- node-TYPE classification from the id prefix,
- color/legend rules + lifecycle derivation.
The Cytoscape interop glue (DOM, WS, render calls) stays direct JS/CLJS. **Any beagle-js gap you hit:
append a record to `~/code/beagle/hallucinations.jsonl` and ping `@beagle-compiler`** — but do NOT block a
runnable v1 on it; fall back to direct JS and note it. Ship something that RENDERS first; the beagle
proof-of-dogfood is the cleanest pure piece, not the whole app.

---

## v1 — build this, make it RUN

1. `bridge`: `/graph` snapshot + `/live` WS against `:7978`.
2. `web`: render the live `:7978` agent graph; force layout; click-node → claims panel; color-by-type +
   legend; animate on live commits.
3. `bin/lodestar-web` one-command start that prints the URL.
4. Commit (scoped). Reply to `@coordinator` with the URL + how to start it.

Then Tom opens it and iterates the fun/aesthetic with you (you can't see the render; he is your eyes).

## v2+ (after Tom sees v1)
Structural edit UI (assert/retract from a node), lifecycle coloring on `:7977` thread graphs, search/filter,
saved views, nicer physics, multi-graph compare.

## Rules
- Standalone at `~/code/lodestar/web`. Scoped commits (`git add <files>`, never `-A`).
- If the work fans out, spawn via SDK (`/spawn` or `/dispatch` on the bridge) — not bash scripts.
- Make it beautiful. This is meant to be FUN.

---

## *** V1 IS THE AGENT PRESENCE VIEW (re-prioritized — build THIS first) ***

Tom's #1 want is to WATCH and STEER his live agents. The agent pool is itself a Fram graph on `:7978`
(agents, roles, focus, messages, cost), so the web client is the SAME app pointed at that graph — plus
a per-agent live activity stream. Build the web client FIRST; the generic graph canvas + code-as-claims
are the second/third views.

The data you have:
- **Fleet graph** (`:7978`): `@agent:<uuid>` nodes with `model`/`effort`/`lifecycle`/`holds @role:<slug>`;
  `@session:<uuid>` with `current_thread`/`active_workflow`; `@msg:*` (`from`/`to`/`subject`/`body`);
  `@run:*` (`cost_usd`). Read via the bridge. Live changes via `:subscribe`.
- **Per-agent live ACTIVITY stream**: each running agent writes `~/code/agent-data/agent-<uuid>.stream.jsonl`
  — newline-delimited `claude -p --output-format stream-json` events (every assistant turn + tool call +
  result, as it happens). This is the "watch it think" feed. The bridge should `tail -f` these and push
  lines over WS to the browser. When an agent is dormant the file is quiet (last run's trace); when it is
  handling a ping it streams live.

V1 web client UI:
1. **Agent list** (left): every `@agent` from presence — short uuid, held role(s), online?, current focus.
   Live-update as focus/online change.
2. **Live stream pane** (center): click an agent → render its `stream.jsonl` live — assistant text,
   tool calls (name + args), tool results, cost ticking. This is the hero: you SEE the agent working.
3. **Steer box**: a text input under the stream → `POST /steer {to, body}` → the bridge sends a msg via
   `msg-cli` to that agent's role/uuid. (You just redirected a running agent from the browser.)
4. **Graph tab** (second view): the original generic force-directed claim graph (any port). Keep it; it is
   v1.5, not the lead.

Same bridge, same Cytoscape investment — the web client is mostly: presence list + a log-tailing WS +
a steer POST. Ship the web client running against `:7978`, report the URL. THEN the graph canvas, THEN
the code-as-claims hero.
