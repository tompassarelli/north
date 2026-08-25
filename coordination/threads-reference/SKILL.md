---
name: threads-reference
description: North shared-thread command syntax and work-projection details. Use only when threads-distilled calls for operational detail.
---

# Threads reference

| Operation | Command |
| --- | --- |
| Read one thread and its facts | `north show <id|slug|substring>` |
| Read thread projections | `north threads [--all]` |
| Read ready work | `north ready [--all]` |
| Read the next work item | `north next` |
| Create a committed thread | `north capture "<title>" [owner]` |
| Assert one fact | `north tell <thread> <predicate> <value>` |
| Retract one exact fact | `north retract <thread> <predicate> <value>` |
