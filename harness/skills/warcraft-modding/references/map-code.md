# Wisp map code

Read before writing or changing Wisp map TypeScript or building a map.

## Fast iteration

- Keep `bun wisp dev` running: it reports type errors and runs affected tests and two-client headless journeys. Run affected tests locally and leave full Bun and 32-bit Lua suites to the landing, which runs them once for every queued lane. Send heavy headless sweeps that need no private inputs or clients, to `bun wisp farm test --wait` for HEAD. [fast-native-iteration.md](fast-native-iteration.md) has the flags, measured costs and warm procedures.
- Prefer hot reload to a script-only rebuild, and a script-only rebuild to a full build. Build fully only for assets, object data or non-TypeScript sources.
- Reload safety: bind each engine callback once through `trampoline(name)`. Register every handler through `on(name, handler)` in the entry's `install()`, which runs at startup and after each reload; do no work at module scope.
- Keep state that must survive a reload in declared globals, not module locals. When a global's shape changes, convert it or start a fresh match. Preserve map files, object data, assets and handles across a reload.
- Write each client's payload before its manifest in `CustomMapData/<prefix>-hot`. All clients verify and install on the same frame, or all refuse.

## Engine constraints

- Numerics: model wrapping 32-bit integers and binary32 arithmetic, including `+` and `*` that do not round to nearest and division up to an ulp off. Use `floorDiv`/`floorMod` or the Wurst-semantic `idiv`/`imod`.
- Wrap each synchronized real operation separately in `f32()`. Write decimal literals through `f32()`, in tests too, and compile noninteger literals to exact hex floats.
- Avoid what the compiler rejects: `%`, `>>>`, `Math.floor(a / b)`, decimal literals that are not binary32, randomness, Date, JSON, Intl and host or DOM APIs. Outside tests, avoid `any`, `as unknown as` and the non-null `!`; use the project's `at()`.
- Give new content a new Preloader path: a path's first content stays cached for the whole session, whatever existence checks report.
- Copy optional fields one at a time, because Lua's `Object.assign` skips undefined fields. Create and free timers, triggers and callbacks only on synchronized turns, or checksums diverge.
- Errors: rely on recorded TypeScript throw sites, not on a Lua debug library, and enable the stack-traces plugin for full stacks. [api-gotchas.md](api-gotchas.md) covers patch-specific natives, assets, input, sync, FileIO and Warsmash investigation.
- Warsmash is AGPL-3.0. Confirm its behavior against the real game before changing Wisp, and record that behavior in wisp:docs/warsmash-notes.md; never copy or adapt its code into MIT Wisp.

## Builds and tests

- Build plumbing: declare typed UI frames through wisp:docs/ui.md. Measure existing compressed imports before re-encoding them (wisp:docs/asset-ingestion.md). Use wisp:docs/ci.md's reusable workflow, and keep private-runner .w3x builds out of public uploads.
- Build through `bun wisp map build` with hashed private inputs in makgora:build-inputs.json. Add art with `bun wisp inputs add FAMILY DIR` and commit the result; never edit stored inputs in place.
- Build a Mak'gora map once per commit and inputs: `bun wisp map build` copies a finished build of the same checkout state, profile, edition and arguments from `~/.cache/wisp/maps` and logs `reused map build HASH` (wisp:docs/compile-cache.md#map-reuse); `WISP_MAP_REUSE=off` forces a fresh build, and a before-measurement of origin/main is that reused build.
- Tests: register `test()` in `*.tests.ts` for Bun and 32-bit Lua under the `verification` skill; it owns retention and bug-fix coverage.
