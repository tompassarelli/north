# North contributor guidance

This file governs changes to the North repository: architecture ownership,
stewardship, code boundaries, and verification. It is not an operator guide and
does not activate North coordination or Agent Machinery run-design features. Consumer
instructions live in switchboard-controlled modules. Product documentation
under `docs/` is reference material, not agent policy. The coordination graph
is canonical; filesystem views are projections only.

## Runtime boundaries

- Coordination, posture, telemetry, concerns, and supervision belong to North.
- Provider SDK/CLI code belongs only under `sdk/src/providers/`.
- Portable work-ownership and agent-run-design contracts belong to Agent Machinery;
  North owns concrete run admission, provider/account/runtime selection,
  transport, supervision, and settlement.
- MCP is the shared data/tool boundary for interactive provider sessions.
- Never add a provider model ID to provider-neutral Agent Machinery contract code.
- Keep consumer behavior out of repository `AGENTS.md` files. Put optional
  runtime guidance in the module or skill that owns the behavior.

## JVM and Clojure source authority

- The required end state is zero directly authored maintained `.clj`. Tom-owned
  North JVM and Clojure semantics must be authored in tracked, typed `.bclj`
  sources using `#lang beagle/clj`; maintained `.clj` must be registered,
  downstream-generated output only.
- Existing maintained `.clj` without a registered typed source/output mapping is
  explicit migration debt, not an irreducible boundary or source authority. Do
  not add new direct `.clj`; this debt may only shrink.
- Any change that touches a legacy direct `.clj` must in the same change move its
  authority to tracked, typed `.bclj`, register the generated `.clj` output, and
  prove byte-for-byte parity. A missing compiler capability routes upstream to
  Beagle and blocks the North change; host-language Clojure is never a fallback.
- A bridge or referent candidate must change zero direct `.clj`.

## JavaScript and Bun source authority

- The required end state is zero directly authored maintained `.ts` or `.tsx`.
  Tom-owned North JavaScript and Bun semantics must be authored in tracked,
  typed `.bjs` sources using `#lang beagle/js`; maintained `.js` and `.d.ts`
  must be registered, downstream-generated output only.
- Existing maintained `.ts` or `.tsx` without a registered typed source/output
  mapping is explicit migration debt, not source authority. Do not add new
  direct `.ts` or `.tsx`; this debt may only shrink.
- External TypeScript declarations are permitted only as immutable pinned
  foreign inputs transformed with `beagle ts-externs`; they are never
  North-authored source authority.
- Any change that touches legacy direct `.ts` or `.tsx` must in the same change
  move its authority to tracked, typed `.bjs`, register its generated `.js` and
  `.d.ts` outputs, and prove byte-for-byte parity. A missing compiler capability
  routes upstream to Beagle and blocks the North change; host-language
  JavaScript or TypeScript is never a fallback.
- Do not add an allowlist enforcement gate while migration debt remains. At
  final cutover, enforcement becomes an unconditional zero-count gate for
  maintained `.ts` and `.tsx`.

## Safe writes and verification

- Assume concurrent agents may be working in the same checkout.
- Preserve unrelated dirty work.
- Run `bun run check && bun run test` from `repo:sdk` in the current worktree
  for SDK changes. The package script owns the hermetic preloads and test
  isolation; do not bypass it.
- The bridge TUI is headless-testable: `createTestRenderer` from
  `@opentui/core/testing` renders the real widget tree and captures snapshots, so
  TUI changes get assertions rather than a screenshot.
- A provider fallback is permitted only before side effects are observable.

## Research projects break forward

- North and sibling personal projects support the current `main` line only.
- No code, data reader, build path, API path, alias, fallback, migration,
  fixture, or test may exist solely for compatibility with an older revision or
  a removed design.
- A breaking change updates every actual in-tree caller in the same change and
  deletes the replaced implementation completely. Do not leave shims,
  tombstones, removal errors, apology comments, or archaeology notes.
- Do not preserve historical source, build, or test snapshots or recovery
  manifests. Git history is the recovery and archaeology mechanism; generated
  artifacts describe only the current source.
- A content-addressed pin may name an active dependency. Once actual consumers
  move, delete the replaced pin and every path that existed only to support it.
