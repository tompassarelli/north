# agent-machinery

Delegation contracts, run design, role templates, deterministic model
selection, and reusable engineering procedures.

The package is deliberately a source authority, not a runtime. It owns the
provider/model/effort catalog, empirical selection policy, and pure resolver,
but does not connect to providers, manage accounts or leases, dispatch work,
persist telemetry, coordinate live participants, install hooks, or project
policy into a harness. A runtime supplies live inventory and observations and
executes the returned ranked plan.

## Public surface

- `agent-machinery:catalog.json` is the complete export manifest. Its
  `delegation` module groups acknowledged work ownership with portable run
  design; `agent-practice` groups the optional
  engineering workflows.
- `agent-machinery:doctrine.md` defines the portable actor, routing, and topology
  rules.
- `agent-machinery:contracts/` contains the machine contracts. Raw schemas
  classify structure; the catalog-advertised `validateContract` export also
  enforces semantics.
- Detailed routing, composition, and extension guidance lives in
  `agent-machinery:docs/`. Generated provider-neutral templates live in
  `agent-machinery:agents/`.
- `agent-machinery:selection/catalog.json` and `resolveExecutionPlan` are the
  single authority for provider/model/effort eligibility, quality-gated
  ranking, and bounded model × effort exploration. `summarizeSelectionEvidence`
  produces daily or weekly calibration periods without owning their schedule.

```sh
bun test
bun run check
```

Consumers should resolve assets through the manifest or the exports from
`agent-machinery:index.mjs`; no path outside this package is an authority.

## License

Licensed under either MIT or Apache-2.0, at your option. See `PROVENANCE.md`
for the public source revisions and retained attribution.
