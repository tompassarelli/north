# North features

- Interactive coding workspace: `north`, or `north --resume ID`.
- Capacity estimates: `machine-capacity estimate [--json] [--class CLASS] [--owner OWNER] -- CMD ARG...` uses the current directory and the last 20 matching measured runs, with exact signature, full owner/shape, owner prefix/shape/class, family/repo, family, and declared-class fallbacks. Owner levels require an explicit owner; the prefix is its leading ASCII letters. It prints p50/p90 cores, peak memory, I/O bytes, disk busy share, sample count, match level, and confidence. Disk busy share is the maximum physical whole-disk `/proc/diskstats` I/O-millisecond delta divided by run wall milliseconds (host-wide; partitions and virtual devices excluded). Stored cgroup I/O pressure is reference data only.
- Skills and hooks: `north config agents help`.
- Work items: `north work list`, `north work search TEXT`, and `north work show REPO#N`; in the app, `/work` opens the list and Enter opens the selected item. Type in the list to filter open issues, or use `/work search TEXT` to include closed issues. Escape returns to the composer.
- Work edits: `north work claim REPO#N [--by NAME] [--eta MINUTES]`, `release REPO#N [--to NAME]`, `need REPO#N BLOCKER#N`, `unneed REPO#N BLOCKER#N`, and `close REPO#N --comment RESULT`. In the app use `/work` followed by the same arguments. Changes are recorded on the GitHub issue.
- GitHub as the Socrates Agent app: `gh-app-token` prints a cached installation token (decrypted from south's `secrets/github-app.yaml`), and `gh-as-app ARGS` runs `gh` with it.

GitHub holds titles, bodies, checklist, state and blockers. `threads` holds current holders, elapsed time and ETA, handoffs and worker runs. North reads both and writes through their commands.

Check a changed Rust feature with `nix develop -c cargo check --offline` and its affected tests with `nix develop -c cargo test --offline --bin north FILTER`. Build the app with `nix develop -c cargo build --offline`, then run `target/debug/north`.
