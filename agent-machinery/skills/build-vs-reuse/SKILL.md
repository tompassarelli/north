---
name: build-vs-reuse
description: >-
  Choose adoption, configuration, extension, a fork, or local implementation when ownership affects a consequential design decision.
---

# Build or reuse

Separate distinctive product behavior from supporting machinery. Compare
credible options on semantic fit, integration and ongoing ownership cost,
migration, licensing, and replacement cost.

Use `prior-art` when options are unknown and `external-code`
before copying or adapting outside material. If local implementation wins,
define its interface, replacement boundary, and cheapest falsifying check.

Bad: building a bespoke queue/cache/auth layer because the off-the-shelf
option "isn't quite right," without pricing the ongoing ownership cost
against the friction it removes. Good: adopt and configure the existing
option unless a named, current gap in it blocks the actual requirement.

For an option worksheet, use `references/notes.md`.
