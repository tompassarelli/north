# Targets rationale

Cold-path reasoning behind numeric performance, time and size targets. Agents do not load this file with rules;
a `Provenance:` record's `rationale` field points at an entry here.

## Soft targets

Reasoning: most gating numbers had no recorded source; an inventory found no source for 207 of 254 bars. Ungrounded
bars blocked delivery: galileo 9.11p measured 57.8 ms against a 50 ms bar and could not land. Tom (12 Oct 2026): a
target can be missed temporarily "as long as you circle back to it later, like intentional debt". So a bar gates only
with a `Provenance:` record naming its origin; a soft bar does not gate, and a miss lands with a `Target-debt:`
trailer naming the measured value, owner and revisit trigger, retired by `Target-paid:`.

Alternatives that lost:
- Galileo's ad hoc rule, where a miss under 2x lands with the row unticked plus a follow-up box: the 2x factor is
  itself ungrounded, it allows a visible drop to 30 fps on a 16.7 ms frame, and unticked rows hide the debt and break
  later count checks.
- A separate debt store: it duplicates git history, which already carries every landing and its trailers.

Trade-off accepted: agents can land over a soft target. The debt stays in `Target-debt` trailers on main and
`north provenance debt --due` surfaces it in the tick once its trigger fires.

Overturned by: debt entries piling up unpaid past their triggers, or a soft-target miss reaching users.

Evidence: north#40; RAIL, https://web.dev/articles/rail; Nielsen, response time limits,
https://www.nngroup.com/articles/response-times-3-important-limits/.
