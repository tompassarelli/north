---
name: iteration-economics
description: >-
  Price an infra, tooling, CI or harness box against feature work before staffing it, and size the recurring code-health review. Use when choosing between speeding up the ask -> playable loop and building features.
grounded: 2026-10-15
written: 2026-10-15
---

# Iteration economics

1. Price the box.
   - Cost `C` = agent-minutes to build, review and land + 10 x Tom-minutes + upkeep.
   - Saving per change `S` = 10 x Tom-blocked wait minutes saved (ask -> playable build) + 1 x agent-minutes saved; weight agent-minutes 0.5 when the usage cap is not binding. Count the two savings separately before weighting.
   - Rate `R` = changes/day over the trailing 7-14 days, from `~/.local/state/agents/iteration-ledger/changes.json` (`bin/iteration-ledger`), else `git log --since=14.days --oneline | wc -l` / 14.
   - Payback days = `C / (S x R)`.
2. Pick the horizon. Extrapolate the trend at most 14 days. Extend to 30 only when the roadmap or issues name committed volume and two weeks of trend support it. In maintenance mode use 7 days.
3. Decide.
   - Staff when payback <= 3 days (1-2 days in maintenance mode).
   - 3-7 days only if the box also removes the current top bottleneck stage, removes flakes or unpredictability, or enables a queued roadmap item.
   - Above 7 days only if it beats the top feature on cost of delay / duration; otherwise defer.
4. Stop optimizing a stage. Re-profile ask -> playable after each fix. Stop when the stage's share of Tom wait is under 25%, when its next item no longer beats the top feature, or when the remaining gain is a uniform <10% shave that crosses no task-switch threshold. Fix variance before mean.
5. Ask Tom, through the proxy's picker, only on direction ambiguity: the horizon is unclear and flips the decision, or two near-equal items imply different directions. Never ask about routine sizing or infra that passes step 3.
6. Declare it in the worker brief; `worker-tier-guard` refuses a `Category: tooling|infra|ci|harness` brief without it:
   - `Payback: saves <N> min/change (Tom <a>, agent <b>), <C> changes/day, cost <M> min, payback <D> days`
   - or, for enabling work with no direct saving, `Payback: enables <what>; cost <M> min`.
7. Size the recurring code-health review the same way: its effort is `C`, the hotspot interest it removes is `S`, and `R` is the change rate of the touched area.

## References

- [xkcd 1205](https://xkcd.com/1205/): time saved per use x uses bounds what a speedup may cost.
- Jaspan & Green, [build latency, predictability and developer productivity](https://research.google/pubs/developer-productivity-for-humans-part-4-build-latency-predictability-and-developer-productivity/): latency effects are nonlinear around task-switch thresholds and predictability matters.
- Fowler, [TechnicalDebt](https://martinfowler.com/bliki/TechnicalDebt.html): interest is paid only when the code is touched, so price by change frequency.
- Reinertsen / [SAFe WSJF](https://framework.scaledagile.com/wsjf/): rank by cost of delay per unit duration.
- [Amdahl's law](https://en.wikipedia.org/wiki/Amdahl%27s_law) and the [Theory of Constraints](https://www.tocinstitute.org/five-focusing-steps.html): a stage's share caps its gain and the constraint moves after each fix.
