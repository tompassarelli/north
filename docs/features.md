# North features

- Interactive coding workspace: `north`, or `north --resume ID`.
- Skills and hooks: `north config agents help`.
- Work items: `north work list`, `north work search TEXT`, and `north work show REPO#N`; in the app, `/work` opens the list and Enter opens the selected item. Type in the list to filter open issues, or use `/work search TEXT` to include closed issues. Escape returns to the composer.
- Work edits: `north work claim REPO#N [--by NAME] [--eta MINUTES]`, `release REPO#N [--to NAME]`, `need REPO#N BLOCKER#N`, `unneed REPO#N BLOCKER#N`, and `close REPO#N --comment RESULT`. In the app use `/work` followed by the same arguments. Changes are recorded on the GitHub issue.

GitHub holds titles, bodies, checklist, state and blockers. `threads` holds current holders, elapsed time and ETA, handoffs and worker runs. North reads both and writes through their commands.

Check a changed Rust feature with `nix develop -c cargo check --offline` and its affected tests with `nix develop -c cargo test --offline --bin north FILTER`. Build the app with `nix develop -c cargo build --offline`, then run `target/debug/north`.
