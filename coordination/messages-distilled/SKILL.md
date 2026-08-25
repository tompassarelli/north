---
name: messages-distilled
description: Use North's coordination message channels when a task requires durable participant-to-participant communication, an inbox read, or an urgent delivery to a live recipient. Do not use for routine progress narration or as a daemon availability probe.
---

# Send messages

Use only inbox, durable mention, urgent live interrupt, or managed-agent steering—never progress narration/probing. Listen only for explicit live delivery you own. Attempt once; on failure retain the body, without retry/channel switch.

Details: `agents path messages-reference`.
