# Failed checks and broken drivers

## Start at the first divergence

Compare expected and observed values at the earliest failing boundary.
Later errors may be consequences. A generic final error is not a root cause
while directory, environment, routing, input, or state mismatches remain.

Separate completed product failure from harness/launch failure, timeout, and
unfinished execution. A harness failure does not falsify the product.
Use an already sanctioned direct route if available; otherwise report the gap,
not a passing claim.

## Repair one cause family

A run failing before its advertised product boundary is integration debugging.
Preserve the counterexample, locate the owning cause, and fix other occurrences
of that same cause on the reachable delivery path before another expensive run.
Do not widen into unrelated code or serially invent new acceptance criteria.

After two failures before the boundary, stop repeating the same attempt.
Return to the frozen journey and identify the earliest unproven seam.
A changed diagnostic or owning repair earns another attempt; a revised ETA
alone does not.

## Ownership and concurrency

One owner supervises and reaps each run, preserving completed work and failure
evidence. Never run concurrent acceptance attempts against one mutable fixture
or shared state directory. Independent required probes can run together only
when their setup/results do not interfere.

## Completion and lessons

Run the fixed required boundary and report its observation. Never weaken a gate,
retry away a failure, or interpret fallback-path success as preferred-path proof.
A useful verification tactic can become durable guidance when requested or
supported by repeated evidence; that does not add another release gate.
