# Targets rationale

Cold-path reasoning behind numeric performance, time and size targets. Agents do not load this file with rules;
a bar's box or `north provenance check` output points at an entry here.

The `Provenance:` landing gate is retired. Tom approved retiring the test-kind and provenance gates and narrowing review to policy prose and hooks (2026-11-04): their after-landing failures were fixed by editing a trailer, a record or the runner, never landed code, so a rework signal (reverts, follow-up fixes and red-main culprits within 48 h of a landing) now watches quality and can bring a gate back on evidence.

## Soft targets

Reasoning: most gating numbers had no recorded source; an inventory found no source for 207 of 254 bars. Ungrounded
bars blocked delivery: galileo 9.11p measured 57.8 ms against a 50 ms bar and could not land. Tom (12 Oct 2026): a
target can be missed temporarily "as long as you circle back to it later, like intentional debt". So a bar gated only
with a `Provenance:` record naming its origin until that gate was retired on 2026-11-04; a soft bar does not gate, and a miss lands with a `Target-debt:`
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

## Goals as checks

Reasoning: a project's goals sat in prose in its README and AGENTS.md, so nothing said whether one was met, and "how much debt may this goal carry" was decided afresh by whoever noticed a miss. Tom (6 Nov 2026) suggested treating a goal as an essential test that may be red for a period, with the amount of debt it may carry left to stewardship judgment. Each project now keeps `goals.toml`: every goal names the README or AGENTS.md line it comes from, an oracle command that prints one number, a bound and a class. A contract is never red, and a landing that turns it red is reverted, like the game answering input inside its frame budget. A debt-allowed goal is a soft target above: red up to a ceiling, with an owner and a revisit trigger standing for the `Target-debt:` record. `north goals check` measures them all and the fleet tick tells the proxy about every red one. A debt-allowed goal may name the paths that pay it (`pays`); once it reads over its ceiling, the lane gate "debt ceiling" refuses every landing that touches none of them except a red-main fix (a `Main-red:` trailer or the sheriff's revert), since a red main already blocks every lane and only the fix reopens it, while a goal naming no paths (the machine's load) is environmental and refuses nothing.

The classes are drafted by agents from the goals themselves; the per-project contract list goes to Tom as a recommendation he can overrule, and the file changes when he does.

Alternatives that lost:
- Goals as ordinary tests in each repository: a goal's oracle reads a measurement the test suite does not run (landing percentiles, the machine's load, a recorded frame run), and a failing test blocks delivery where a debt-allowed goal must not.
- One central registry in north: makgora and wisp own their goals and change them in their own landings, so the file sits beside the README and AGENTS.md it is drawn from.

Trade-off accepted: an oracle that needs more than a few seconds (makgora's frame budget) reads the latest recorded run, so its value ages until something refreshes the record.

Overturned by: goals that stay red past their revisit triggers with no landing aimed at them, or oracles whose values disagree with the project's own CI.

Evidence: north#641; docs/rationale/targets.md above.
