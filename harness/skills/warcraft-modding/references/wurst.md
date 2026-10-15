# Wurst development

Read this when the requested source or toolchain is WurstScript: Wurst projects, compiler workflows, tests, compile-time object data or Warcraft III UI in Wurst. Existing Lua or Jass projects need no migration.

## Project and toolchain

- Use the project's Wurst source and toolchain without replacing existing Lua or Jass source, because migration is not the task.
- Read project instructions, `wurst.build` and dependency locks before editing, because they pin the patch, backend and library.
- Align the declared Warcraft patch, Jass or Lua target, compiler and standard-library versions, because a mismatch compiles against natives the game lacks.
- Use the pinned compiler's focused typecheck, test and map-build commands, and record the actual pinned commands in the project's AGENTS.md, because the next agent should not rediscover them.
- Inspect the selected compiler version before accepting an editor-offered install or update when the pin is unclear, because an update silently changes the toolchain.
- Check resolved standard-library sources and nearby consumers before writing helpers or using natives, because the resolved sources are the available capability; the [manual](https://wurstlang.org/manual.html) and [standard library](https://wurstlang.org/stdlib) describe public APIs, not what the pin has.
- Confirm newer APIs against resolved versions and verify engine interaction natively, because upstream articles run ahead of pinned libraries.

## Code

- Keep simulation calculations independent of live handles in pure functions and retain `Wurstunit` tests under the `verification` skill, because pure code tests without the game.
- Build and launch the map for timing, handle, rendering and multiplayer claims after focused feedback, because unit tests cannot show them.
- Use value tuples such as `vec2` for coordinates and small physics values, because they copy on assignment and need no destruction.
- Use classes for identity or managed lifecycles under the resolved library's ownership and destruction rules, because class instances leak unless destroyed.
- Use extension functions and cascades to clarify handle operations, because they keep handle code readable.
- Give lambdas an expected callback type and account for by-value local captures and noncapturing `code` callbacks, because a `code` callback cannot capture locals.
- Author unit, ability, item and upgrade data through `@compiletime` typed object APIs with stable, collision-free ID generators when code owns that data, because hand IDs collide across files.
- Let the project build inject and package generated object data without making generated files a second source, because two sources drift.
- Use compile-time import facilities when they remove repeated manual setup.
- In Lua mode use the build's `wurst.build` injection instead of handwritten parallel Lua, and validate target-specific behavior on the configured Jass or Lua backend, because the backends differ at runtime.
- Use optional alpha JHCR only for Jass maps when its rewritten map script and inserted preload data justify the setup, because it alters the shipped script.

## UI

- Use `framehandle` for custom UI and `widget` for game-world units, items and destructables, and inspect resolved `Framehandle` and `ClosureFrames` APIs before UI work, because the wrappers differ by library version.
- Create or acquire UI frames after game time starts and cache shared handles outside `GetLocalPlayer()`, because handle creation inside a local block desyncs.
- Treat frame events as synchronized across clients and separate local visual state from gameplay mutations, because a local-only mutation desyncs.
- Release retained button or edit-box focus with `framehandle.onClickReleaseFocus()` where appropriate, because a focused frame swallows game hotkeys.
- Check UI-coordinate behavior for 4:3 and full-screen frame positioning, because frames anchor differently in each.

Read [modern Wurst workflow](wurst-modern-workflow.md) for UI components, headless layout checks, object-data changes, Grill checks or workspace creation.
