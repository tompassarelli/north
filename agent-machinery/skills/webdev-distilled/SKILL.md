---
name: webdev-distilled
description: >-
  Apply shared accessibility rules when writing or reviewing web forms and responsive interfaces.
---

# Web UI

Keep focusable `input`, `select`, and `textarea` text at a computed size of at
least 16px, including focused and active states. This avoids unwanted mobile
viewport zoom.

Preserve pinch zoom: do not set `user-scalable=no` or `maximum-scale=1`.
Keep framework-specific rules in the project.

For the CSS example, use `agents path webdev-reference`.
