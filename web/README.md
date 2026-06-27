# framescope

A web app that points at any **Fram claim-graph daemon** by port and renders it
live. v1 is the **fleet lodestar web**: watch and steer your running agents.

## Run

```bash
bin/framescope            # -> http://localhost:8088   (needs babashka `bb`)
bin/framescope 9000       # pick a different http port
```

Targets the fleet coordinator on `:7978` by default; change the target daemon
live from the port box in the UI (no restart).

## What you get

- **Lodestar Web tab**
  - **Agents** (left): every `@agent` from presence — online dot, held roles,
    current focus (workflow/thread), live activity tag. Refreshes itself.
  - **Stream** (center, the hero): click an agent → its
    `~/code/fleet-data/agent-<uuid>.stream.jsonl` is tailed over a WebSocket and
    rendered as it happens — thinking, assistant text, tool calls + inputs, tool
    results, a token ticker. You watch it think.
  - **Steer** (bottom): type a message → `POST /steer` → sent to that agent via
    `msg-cli`. Redirect a running agent from the browser.
- **Graph tab**: the generic claim-graph view — force-directed (fcose),
  data-driven node-type colors + legend, click a node for its claims (`@ref`
  objects are clickable to recenter), live commits flash/add on the canvas.

## Architecture

- `bridge/bridge.clj` — babashka + http-kit. The only thing that speaks raw TCP
  to a Fram daemon; proxies the line-delimited-EDN protocol to HTTP + WebSocket.
  `/graph` `/presence` (JSON), `/live` (commit feed WS), `/stream` (activity tail
  WS), `/steer` (POST → msg-cli).
- `web/` — plain HTML/CSS/JS + vendored Cytoscape. `graph-domain.js` is the pure
  claims→graph transform (type classification, color/legend), kept IO-free as the
  beagle-js dogfood candidate.

## Roadmap

- v1.5 — richer graph (synthesize semantic edges from bare-string cross-refs),
  lifecycle coloring on `:7977` thread graphs, search/filter.
- v2 — point at the **code-as-claims** graph (`beagle --emit-edn`): a live,
  clickable code inspector. The type/color system is already data-driven for it.
- v2+ — structural edit UI (assert/retract from a node).
