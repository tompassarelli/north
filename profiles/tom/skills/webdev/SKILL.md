---
name: webdev
category: webdev
description: >-
  Use when writing or reviewing web UI: HTML/CSS/JS, forms, layout,
  responsive/mobile behavior. House rules that apply across web projects;
  framework- and repo-specific guidance stays in each repo.
---

# webdev — cross-project web UI rules

## Mobile form inputs: font-size ≥ 16px

Set `font-size` to at least `16px` on every field that can receive focus —
`input`, `select`, `textarea` — including their focus/active states. Mobile
browsers (iOS Safari foremost) auto-zoom the viewport when a focused field's
computed font-size is below 16px, and the page is left zoomed after blur.

```css
input, select, textarea {
  font-size: 16px; /* ≥16px; rem is fine if root stays ≥16px */
}
```

Do not "fix" this with `user-scalable=no` or `maximum-scale=1` in the
viewport meta — that disables pinch-zoom for everyone and fails
accessibility; the 16px floor removes the trigger instead.
