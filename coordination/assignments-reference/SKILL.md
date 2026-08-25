---
name: assignments-reference
description: North assignment command syntax and state-reading surfaces. Use only when assignments-distilled calls for operational detail.
---

# Assignments reference

| Operation | Command |
| --- | --- |
| Managed work | `north delegate "<task>" --thread <thread>` |
| Manual pickup | `north tell <thread> driver <actor>` |
| Manual release | `north retract <thread> driver <actor>` |
| Inspect one thread | `north show <thread>` |
| Inspect thread projection | `north threads` |
| Inspect agents | `north agents` |

Managed delegation records and releases its driver claim as part of the dispatch lifecycle.
