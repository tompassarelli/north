# Lodestar

The life app — what you steer by. Capture an intention; query what's **ready**,
**blocked**, and the highest-leverage keystone. The board is *derived* from a
graph of claims, never hand-maintained.

Lodestar is a **consumer of the [Fram](https://github.com/tompassarelli/fram)
engine** (a domain-neutral claim substrate). It supplies the *life domain*: the
lifecycle projections, the cardinality vocab (`FRAM_SINGLE_VALUED`), capture
conventions, time tracking, and the operating manual.

This is a personal life-management tool, published as reference. It is shaped
around how one operator works; expect to adapt the wrapper and conventions to
your own setup.

## Shape

- **Engine** → [Fram](https://github.com/tompassarelli/fram) (`~/code/fram`):
  claims, Datalog, the coordinator daemon. The hard substrate.
- **Life domain** → `src/lodestar/{projections,clock,clockify,staleness,audit}.bclj`:
  the lifecycle derivations, billing projection, and staleness layer that make
  the engine a life app.
- **CLI** → `bin/lodestar`: aims the Fram engine at your data and sets capture
  provenance defaults. Life verbs (`ready`/`blocked`/`leverage`/`next`/`agenda`/
  `plate`/`capture`/`clock`/…) route to `lodestar.main`; engine verbs
  (`import`/`export`/`show`/`validate`/`tell`/`untell`/…) route to Fram.
- **MCP** → `bin/lodestar-mcp`: the AI-facing edge — every tool maps to a tested
  CLI op through the coordinator write path.
- **Data** → your own private store (the canonical `claims.log`, projected to
  `~/.local/state/lodestar/` at runtime). Data is **not** part of this repo.

## Docs

- `docs/operating-manual.md` — the working manual: thread model, claim format,
  derived lifecycle, the CLI surface, and session behavior.
- `docs/claim-native-redesign.md` — the design record for the claim-native model.
- `docs/PROPOSAL.md` — the original vision and architecture.

## Building

Lodestar is a consumer of the [Fram](https://github.com/tompassarelli/fram)
engine and is written in [Beagle](https://github.com/tompassarelli/beagle) (a
Lisp that emits Clojure). Building from the `.bclj` sources requires both Fram
and Beagle checked out alongside this repo; `build.sh` links the engine sources
in (`src/fram`, gitignored) and compiles the life-domain modules into `out/`.
Set `FRAM_HOME`/`BEAGLE_HOME` if they don't live at `~/code/fram` /
`~/code/beagle`.

## License

MIT — see [LICENSE](LICENSE).
