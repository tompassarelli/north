# Coverage test selection: the tests a change reaches, before the push

North lands first and verifies after the push, so a failing test costs a revert. Before this, the
cheap way to catch most of those before the push was to run each affected crate's tests, but that
costs as much as the after-push run (north#634). A test that never executes the code a change
touches cannot be broken by it, unless the change is to a constant, a type or a macro that the map
below does not trace. So the pre-push run can be the few tests that reach the changed functions.

## Method

- Record: `north coverage record` builds the workspace tests on a check host with
  `-C instrument-coverage` and LLVM codegen. Dev builds use Cranelift, which cannot instrument. It
  runs the same suite as the after-push nextest line, with `north coverage exec` as the cargo
  target runner. nextest calls the runner as `BINARY --exact TEST --nocapture`, so the runner gives
  each test its own `LLVM_PROFILE_FILE`. The pattern is pooled with `%4m`, so the north processes a
  test spawns merge into one file per binary. After the test it reduces the profile to the names
  of the executed functions (`llvm-profdata merge -sparse`, then `show --all-functions`) and
  deletes the raw files, so disk stays bounded by the tests running at that moment.
- Spans: `llvm-cov export` over every test binary and the instrumented `north` binary gives each
  function's file and line span. A function that no test executes keeps an empty entry, so a
  change inside it correctly selects nothing.
- Select: the lane's `git diff -U0` hunks are matched to the innermost function containing each
  changed line. Each function's span is carried from the map's commit to the lane's base through
  `git diff -U0 MAP BASE`.
  - A function that was edited since the map was recorded selects every test reaching its file.
  - A line outside every function (a constant, a type, a `use`, a macro, a new test) cannot be
    traced, so it runs nothing before the push. Neither does an untraced input (Cargo.toml,
    build.rs, a run-time table, a file under `tests/` that is not Rust), a new file, a map over
    3 days old, or a selection of over 400 tests.
  - The full suite always runs after the push.
- Miss: when the after-push run is red and its prefix search names a culprit lane, the lander runs
  `north coverage miss BASE TIP --log LOG` on that lane's own landed range. A test that failed
  in that log but that the selection did not run is a miss; the verb names each one and starts
  `north coverage refresh` in the background, so the next selection reads a map with that reach.
  A selection that ran nothing (an untraced input, an old map, over 400 tests) is not judged. One
  refresh runs at a time, whether nightly or after a miss.

## Measured, 2026-11-06

The map was recorded on gex45 at 883c0d41a with the full suite: `--workspace --lib --bins --test it`.

- 773 tests left a reach, all of those listed.
- The spans cover 29,519 functions, 13,620 of them executed by at least one test.
- The map is 3.0 MB of JSON.

The number of tests reaching each of the 27,228 non-test functions is the number a one-function
change selects:

- Median 0, p75 1, p90 4, p99 25, max 112.
- 27,086 of them (99.5%) are under 30.
