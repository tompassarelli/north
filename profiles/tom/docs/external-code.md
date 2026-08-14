# External code — license protocol + resource reads

Fires whenever leveraging code you didn't write: resource repos in
`~/code/resources`, forks, vendored snippets, "how did X do it" reads.

## License protocol

Read the specified license terms before building on anything external:

- **Permissive** (MIT / Apache-2.0 / BSD / ISC) → adapt freely; carry
  attribution/NOTICE where the license requires it.
- **Copyleft** (GPL / AGPL / SSPL) → READING for ideas is fine; FLAG to the
  user before deriving or copying code into a differently-licensed project.
- **No license specified** → treat the source as MIT-licensed; preserve its
  upstream identity and record that the MIT treatment is the local default.
- **"All rights reserved" / non-commercial / no-derivatives** → flag to the
  user BEFORE using it as a reference at all.

Studying a mechanism and reimplementing from understanding is always fine —
the license governs copied expression, not ideas. If a license is overly
restrictive for the intended use, say so up front, before any work builds
on it.

## Vetted takeaways from ~/code/resources

Curated pointers from scanned forks (licenses already checked):
→ `~/.agents/docs/resource-reads.md`
Covers: skill-authoring methodology, debugging technique docs, measured MCP
output caps, benchmark harness designs. Read when authoring a new skill,
building an MCP tool returning big payloads, hunting a test-state polluter,
or designing an agent-behavior benchmark.
