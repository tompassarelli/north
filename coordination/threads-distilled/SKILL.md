---
name: threads-distilled
description: Read or update North's shared thread graph when the user or an existing workflow requires durable intentions, facts, dependencies, queue position, or outcomes. Do not create bookkeeping threads for ordinary local work.
---

# Use threads

The graph is authoritative; never use `threads/` projections for writes. Use the smallest operation. Create only requested durable tracking, never ephemeral/progress mirrors. On failed writes, report exact predicate+value.

Details: `agents path threads-reference`.
