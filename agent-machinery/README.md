# agent-machinery

Delivery doctrine, work-ownership contracts, and reusable engineering
procedures.

The package is deliberately a source authority, not a runtime. It does not
connect to providers, manage accounts or leases, dispatch work, persist
telemetry, coordinate live participants, install hooks, or project policy into
a harness.

## Public surface

- `agent-machinery:catalog.json` is the complete export manifest. Its
  `delegation` module holds acknowledged work ownership; `agent-practice`
  groups the optional engineering workflows.
- `agent-machinery:doctrine.md` defines the portable delivery rules.
- `agent-machinery:contracts/` contains the machine contracts. Raw schemas
  classify structure; the catalog-advertised `validateContract` export also
  enforces semantics.

```sh
bun test
bun run check
bun run check:guides    # lists vendored guides older than 90 days
bun run refresh:guides  # re-fetches them at upstream HEAD
bun scripts/effect-kit.mjs init|check|sync|upgrade|scan  # the Effect kit; --help
```

Consumers should resolve assets through the manifest or the exports from
`agent-machinery:index.mjs`; no path outside this package is an authority.

## Skill delivery

`agents sync` reads an optional inline
`agents: [claude]`, `agents: [codex]`, or `agents: [claude, codex]` field in
skill frontmatter. It delivers those skills only to the named agents and
removes earlier managed copies from excluded agents. Skills without the field
keep their catalog targets, including shared delivery.

## License

Licensed under either MIT or Apache-2.0, at your option. See `PROVENANCE.md`
for the public source revisions and retained attribution.
