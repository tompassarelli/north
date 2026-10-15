# North architecture

North is the Rust harness that starts, observes and directs Socrates workforce
sessions. The installed `north` command opens the fleet screen; `north fleet`
also opens it, and its subcommands expose the same session operations to other
harness commands. Hooks, policy and provider-independent procedures live in
the rest of this repository and `agent-machinery/`.

## Owning boundary

```text
terminal input
    -> fleet screen state
    -> authorized fleet operation
    -> session runtime
    -> transcript and usage events
    -> fleet screen projection
```

`src/fleet/` owns sessions, delivery, decisions and the runtime boundary.
`src/tui/fleet/` owns the terminal loop, transcript, launch form and fleet
digest. `src/providers/` adapts provider protocols behind the fleet runtime.
`src/cli/` wires commands and provider implementations. The run ledger records
session outcomes and task verdicts; it is not a second session authority.

The fleet screen keeps session selection stable while state changes, shows
transcripts and live token totals, and presents open decisions, usage and
landings in its digest. A bare `north` follows the same path as `north fleet`.

Run `nix develop -c cargo check --locked --offline` to check the Rust
interfaces. `verify-north` drives the fleet screen and CLI against the installed
build, including the headless session path.
