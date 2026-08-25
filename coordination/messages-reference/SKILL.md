---
name: messages-reference
description: North coordination messaging command syntax and delivery-channel details. Use only when messages-distilled calls for operational detail.
---

# Messages reference

| Contract | Command |
| --- | --- |
| Read durable pending messages | `north inbox [--as <recipient>]` |
| Record a durable request | `north mention <recipient> "<body>" [--about <thread>]` |
| Deliver urgently to a live recipient | `north interrupt <recipient> "<body>"` |
| Steer a running managed agent | `north msg <agent-id> "<body>"` |
| Run a real-time listener | `north listen <agent-id>` |

Mentions permit an offline recipient. Interrupts require a live recipient. A listener is long-running and receives real-time delivery.
