# Extending the template library

An agent-run-valid composition has an explicit semantic route, bounded
authority, executable canonical capabilities, done criteria, and compact
communication norms.

To add a stock template:

1. Add its behavior contract block to `agent-machinery:docs/roles.md`.
2. Add one entry to `agent-machinery:staffing/catalog.json`.
3. Declare every new generator input or documentation asset in
   `agent-machinery:catalog.json`.
4. Keep capability declarations transitively closed; shell authority includes
   its effective filesystem authority.
5. Rebuild with `bun run build`.
6. Validate with `bun run check`.

Role IDs use lowercase kebab case. Add templates only after a recurring
composition demonstrates the same responsibility, deliverable, topology,
capabilities, done criteria, and report shape more than once.

The package never adds concrete model routes or runtime mappings. A consumer
may map canonical capabilities to its execution substrate and supply live
provider inventory to the model-selection resolver, but it
must preserve the requested floor and fail closed when authority cannot be
enforced.
