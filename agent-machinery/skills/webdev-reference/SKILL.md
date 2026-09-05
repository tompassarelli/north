---
name: webdev-reference
description: Full notes on mobile form font sizing and browser zoom behavior.
---

# Mobile form sizing: full notes

## Adopted rule

Keep mobile form controls at a computed font size of at least 16 CSS pixels
unless the product has a deliberate accessible alternative.

```css
input, select, textarea {
  font-size: 16px;
}
```

## Rationale and limits

iOS Safari may zoom when a focused field computes below that threshold, and the
viewport can remain zoomed after blur. The concern is the computed control
size, not a particular unit: `1rem` is sufficient only when the root computes
to at least 16px.

The fragment is a baseline, not a complete responsive or accessibility audit.
Check the actual control in the affected layout when diagnosing zoom.
Do not disable user scaling to conceal the symptom; that removes useful
accessibility behavior rather than correcting the control.
