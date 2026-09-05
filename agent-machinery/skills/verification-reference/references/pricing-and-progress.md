# Pricing checks and preserving progress

## Internal cost model

Estimate wall time from the relevant prior, remaining uses N, optimization
cost C, and credible saving per use S. Break-even is ceil(C/S); without a
credible saving, it is never. Optimize only when remaining uses repay the cost.
Ordinary one-off commands need a quick internal judgment, not a report or form.

Start direct measurement with one cold and at most three representative warm/
edit observations. More samples need a stated threshold or tail claim.
Do not report p50/p95 from fewer than 20 observations. Different scope, hardware,
toolchain, and cache states are not comparable samples.

## Estimates are not cancellation

Separate a completion forecast, an observation checkpoint, and a hard resource
bound. Include legitimate setup/downloads when bounding a run. An optimistic
forecast is not a safe kill timer.

At the first unexpected delay, inspect the current phase and observable
progress before another long wait or restart. For a download, remaining bytes
and measured throughput can update the estimate. Silence and elapsed time are
not proof of deadlock.

At roughly twice the estimate, reassess the route and report a material changed
expectation. Preserve useful safe progress. Restart only when an observed
failure or corrective change justifies repeating setup and losing in-flight
work. A missed delivery deadline does not cancel authorized work; explicit stop
instructions and real safety/resource limits do.

## Optimization alternatives

Batch independent edits before an expensive check. Use supported focused
selection when it still proves the needed claim. A new harness gets one
implementation and one repair cycle by default; after that, simplify or report
the missing observation. Do not build observability or runtime infrastructure
to reassure an already resolved decision.

A named repeated edit-to-signal loop belongs to competitive-development-loop;
the one-off check remains here. Preserve useful measurements without turning
every command into a profiling campaign.
