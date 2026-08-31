---
name: webdev-reference
description: CSS example and mobile-browser rationale for the cross-project web UI rules. Use only when webdev-distilled calls for implementation detail.
---

# Webdev reference

```css
input, select, textarea {
  font-size: 16px;
}
```

Using `rem` is equivalent when the root remains at least 16px. iOS Safari and other mobile browsers may zoom the viewport when a focused field computes below that threshold, and the page can remain zoomed after blur.
