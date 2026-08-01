# Building and testing

Running North is not the same job as rebuilding it. Most people only need the
first section.

## Running

Running the ledger needs only [babashka](https://babashka.org): the compiled
Clojure is committed under [`out/`](../out), so no Beagle toolchain is required
at runtime — the same arrangement Fram uses.

You also need the Fram engine checked out. [`bin/north`](../bin/north) resolves
it from `FRAM_HOME`, falling back to a world manifest and then to
`~/code/fram/main`, and puts both North's and Fram's `out/` on the classpath.

The agent SDK and the MCP edge additionally need [Bun](https://bun.sh).

The packaged path avoids all of this:

```console
$ nix run github:tompassarelli/north
```

## Rebuilding from source

Only needed when you change a `.bclj` source. This requires
[Beagle](https://github.com/Autonymy/beagle), the typed Lisp North is written
in, in addition to Fram.

```console
$ ./build.sh
  built north/projections
  ...
north built -> /path/to/north/out  (engine: /path/to/fram/out on classpath at runtime)
```

[`build.sh`](../build.sh) symlinks the engine's Beagle sources into `src/fram`
(gitignored) so the type checker resolves `fram.*` with full types, then
compiles each coordination-domain module into `out/`. **Commit the result** —
`out/` is a checked-in build product, and a source change that ships without it
is a change that does not take effect for anyone running from the checkout.

`build.sh` reads `BEAGLE_HOME` (default `~/code/beagle`) and `FRAM_HOME`
(default `~/code/fram`). Note that these defaults differ from `bin/north`'s
runtime default of `~/code/fram/main`; set both explicitly if your checkouts do
not sit at the defaults.

## Tests

Set `FRAM_HOME` first — the classpath and the lifecycle test's log both derive
from it.

```console
$ export FRAM_HOME="$HOME/code/fram/main"
$ CP="out:$FRAM_HOME/out"
$ bb -cp "$CP" clock_test.clj
$ bb -cp "$CP" staleness_test.clj
$ FRAM_LOG="$FRAM_HOME/facts.log" bb -cp "$CP" lifecycle_test.clj
```

CI runs every test command through [`bin/test-suite`](../bin/test-suite) with
`--sandbox-home`. The mode creates a new empty `HOME`, removes ambient XDG and
North/Fram state selectors, prints the scratch path, and deletes it after the
command. Use the same boundary locally while keeping the Fram fixture explicit:

```console
$ FRAM_TEST_CHECKOUT="$FRAM_HOME" \
    bin/test-suite --sandbox-home -- bb -cp "$CP" clock_test.clj
```

The SDK receipt suite also needs its repository-owned hook fixture:

```console
$ cd sdk
$ FRAM_TEST_CHECKOUT="$FRAM_HOME" \
    NORTH_TEST_AGENT_HOOKS_DIR="$PWD/../profiles/tom/hooks" \
    ../bin/test-suite --sandbox-home -- bun run test
```

Omit `--sandbox-home` to run a command with the existing environment unchanged:

```console
$ bin/test-suite -- bb -cp "$CP" clock_test.clj
```

Other babashka suites at the repository root follow the same shape —
`validate_test.clj`, `projections_test.clj`, `schema_test.clj`,
`lifecycle_test.clj`, and the rest — as do the CLI suites under
[`cli/tests/`](../cli/tests).

The gateway has its own smoke test covering auth and tenant routing:

```console
$ bash deploy/gateway/smoke_test.sh
```

The TypeScript agent SDK is driven by its own package scripts, which own the
hermetic preloads and test isolation. Do not bypass them by invoking `bun test`
directly:

```console
$ cd sdk && bun run check && bun run test
```

`bun run check` runs the license-integrity check, `tsc --noEmit`, and a
no-bundle build of every entry point; `bun run test` runs
[`sdk/test/support/run-suite.sh`](../sdk/test/support/run-suite.sh).
