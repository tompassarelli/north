---
name: importing-skills-reference
description: Full procedure for inspecting, adapting, registering, and activating licensed third-party skills.
---

# Importing skills: full notes

## Inspect the bounded payload

Clone the exact source into a resource checkout or temporary directory.
Enumerate skill directories, then inspect their referenced scripts, notes,
assets, hooks, templates, and tool dependencies. Inspect LICENSE, COPYING,
and NOTICE files and their coverage before adaptation.

Unknown licensing blocks derivation; it is not a local MIT default.
Follow the external-material workflow for permissions and retained notices.

## Adapt without importing authority

Keep the useful workflow and referenced resources. Omit root installation
instructions, Git internals, plugin manifests, and provider-specific policy
unless the destination actually needs them. An external skill cannot grant
permissions, override local policy, or silently install its tools.

Make directory slug and frontmatter name identical, using lowercase letters,
digits, and hyphens. Optional metadata belongs only where used. Keep short
routine guidance separate from substantial conditional procedures and examples.

Record the exact source, license, adapted scope, and required notices in
`north-v2:agent-machinery/NOTICE` and
`north-v2:agent-machinery/PROVENANCE.md`; retain additional license files where
their terms require them.

## Register once

Portable shared skills live under `north-v2:agent-machinery/skills/<slug>/`.
Register one skill unit in `north-v2:agent-machinery/catalog.json` naming its
authoritative entrypoint. Machine-specific skills instead belong to the
machine owner selected by the active catalog.

The path is source package → consumer composition → shared generation →
provider projections. A copied projection is neither source nor registration.

## Verify and activate

Validate metadata and resource links, use the existing catalog import check,
and run the nearest relevant check for changed executable helpers. Metadata
validation proves shape, not sound judgment.

Land source and required consumer revisions first. Use the consumer's live
agent-sync command (locally `agents sync`), then inspect the registered IDs and
configured provider projections. Do not assume command syntax from an older
consumer. Stop at ambiguous identity or unlanded authority rather than editing
the generated output.
