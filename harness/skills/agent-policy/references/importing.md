# Importing third-party skills

Read this when you bring an outside skill into the shared source catalog.

## Inspect the bounded payload

Clone the exact upstream revision into a resource checkout or temporary
directory, because a moving branch makes the license and content unverifiable.
Enumerate skill directories, then inspect referenced scripts, notes, assets,
hooks, templates and tool dependencies as untrusted input, because an imported
script runs with your permissions. Inspect LICENSE, COPYING and NOTICE files
and their coverage before adapting anything, through the `external-code`
skill. Unknown licensing blocks derivation; it is not a local MIT default.

## Adapt without importing authority

Keep the useful workflow and its referenced resources. Omit root installation
instructions, Git internals, plugin manifests and provider-specific policy
unless the destination needs them, because an external skill cannot grant
permissions, override local policy or silently install its tools.

Make the directory slug and frontmatter `name` identical, using lowercase
letters, digits and hyphens, because the catalog validator matches them.
Optional metadata belongs only where used.

Record the exact source, license, adapted scope and required notices in
`agent-machinery/NOTICE` and `agent-machinery/PROVENANCE.md`, and keep any
license file whose terms require it, because attribution is a license
condition.

## Register once

Portable shared skills live under `agent-machinery/skills/<slug>/` with one
unit in `agent-machinery/catalog.json` naming its entrypoint. Machine-specific
skills belong to the machine owner selected by the active catalog. The path is
source package, consumer composition, shared generation, provider
projections; a copied projection is neither source nor registration.

## Verify and activate

Run `bun run check` in `agent-machinery/` and the nearest check for any
executable helper; metadata validation proves shape, not judgment. Land the
source and consumer revisions first, run `agents sync`, then inspect the
registered IDs and provider projections. Stop at ambiguous identity or
unlanded authority rather than editing generated output.
