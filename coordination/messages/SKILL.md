---
name: messages
description: Use North's coordination message channels when a task requires durable participant-to-participant communication, an inbox read, or an urgent delivery to a live recipient. Do not use for routine progress narration or as a daemon availability probe.
---

# Coordination messages

Choose the channel by delivery contract:

- `north inbox [--as <recipient>]` reads durable pending messages.
- `north mention <recipient> "<body>" [--about <thread>]` records a durable
  request and permits an offline recipient.
- `north interrupt <recipient> "<body>"` is urgent and requires a live
  recipient.
- `north msg <agent-id> "<body>"` steers a running managed agent.

Do not send a message simply because coordination is active. Do not call a
message command to test whether North is up. Attempt a required delivery once;
on an availability failure, preserve the undelivered body in the report rather
than retrying or silently changing channels.

`north listen <agent-id>` is a long-running real-time listener, not session
setup. Arm it only when the current task explicitly requires live interrupt
delivery to that agent and you own the listener lifecycle. Never arm it as a
default, a health check, or a substitute for durable messages.
