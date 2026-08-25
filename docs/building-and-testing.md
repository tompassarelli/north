# Building and testing

Running North is not the same job as rebuilding it. Most people only need the
first section.

## Running

Running the ledger needs only [babashka](https://babashka.org): the compiled
Clojure is committed under [`out/`](../out), so no Beagle compiler is required
at runtime.

You also need the Beagle Store engine selected by the host-published
`~/.local/state/north/beagle-store.env`. The installed wrapper sources that exact
selection and puts both North's and Beagle Store's `out/` on the classpath. Tests may
instead pass an explicit `BEAGLE_STORE_HOME` and matching `BEAGLE_STORE_OUT`.

The agent SDK and the MCP edge additionally need [Bun](https://bun.sh).

The packaged path avoids all of this:

```console
$ nix run github:tompassarelli/north
```

## Rebuilding from source

Only needed when you change a `.bclj` source. This requires
[Beagle](https://github.com/tompassarelli/beagle), the typed Lisp North is
written in and the repository that provides the Beagle Store engine under
`store/`.

```console
$ ./build.sh
  built north/projections
  ...
north built -> /path/to/north/out  (engine: /path/to/beagle/store/out on classpath at runtime)
```

[`build.sh`](../build.sh) declares North's `src` and the engine's `src`
directories as the `north/src` and `store/src` module roots so the type
checker resolves both namespaces with full types, then compiles each
coordination-domain module into `out/`. **Commit the result** —
`out/` is a checked-in build product, and a source change that ships without it
is a change that does not take effect for anyone running from the checkout.

`build.sh` reads `BEAGLE_HOME` (default `~/code/beagle/main`) and `BEAGLE_STORE_HOME`
(default `$BEAGLE_HOME/store`). Set them explicitly if your checkout does
not sit at that authoring default.

## Tests

Set `BEAGLE_STORE_HOME` to Beagle's `store/` first so the classpath resolves the
matching engine output.

```console
$ export BEAGLE_STORE_HOME=/path/to/the/exact-beagle-checkout/store
$ CP="out:$BEAGLE_STORE_HOME/out"
$ bb -cp "$CP" tests/clock_test.clj
$ bb -cp "$CP" tests/staleness_test.clj
```

CI runs every test command through [`bin/test-suite`](../bin/test-suite) with
`--sandbox-home`. The mode creates a new `HOME`, removes ambient XDG and
North/Beagle Store state selectors, projects an explicit `AGENT_MACHINERY_HOME`
under the sandbox dependency area, points the variable at that projection,
prints the scratch path, and deletes it after the command. No mutable checkout
fallback is created. Without that explicit package root, the new home remains empty.
Use the same boundary locally while keeping the Beagle Store fixture explicit:

```console
$ BEAGLE_STORE_TEST_CHECKOUT="$BEAGLE_STORE_HOME" \
    bin/test-suite --sandbox-home -- bb -cp "$CP" tests/clock_test.clj
```

The SDK suite reads a composed hook generation only when
`NORTH_TEST_AGENT_PROVIDER_HOOKS` names one explicitly. Source-owner repositories
prove individual guard behavior; North's suite proves the composed chain when a
generation is supplied.

Omit `--sandbox-home` to run a command with the existing environment unchanged:

```console
$ bin/test-suite -- bb -cp "$CP" tests/clock_test.clj
```

Other Babashka suites under [`tests/`](../tests) follow the same shape —
`tests/validate_test.clj`, `tests/projections_test.clj`,
`tests/schema_test.clj` and the rest — as do the CLI suites under
[`cli/tests/`](../cli/tests).

The TypeScript agent SDK is driven by its own package scripts, which own the
hermetic preloads and test isolation. Do not bypass them by invoking `bun test`
directly:

```console
$ cd sdk && bun run check && bun run test
```

Documentation freshness is independent of the Beagle Store coordinator and needs only
Babashka:

```console
$ bin/docctl scan
$ bin/docctl invalidate
$ bin/docctl queue
```

See [`docctl.md`](docctl.md) for sidecar manifests and review policy.

`bun run check` runs the license-integrity check, `tsc --noEmit`, and a
no-bundle build of every entry point; `bun run test` runs
[`sdk/test/support/run-suite.sh`](../sdk/test/support/run-suite.sh).
