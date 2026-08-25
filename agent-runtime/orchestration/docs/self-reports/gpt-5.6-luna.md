# Self-report: gpt-5.6-luna

## How I work

I begin by trying to understand the requested outcome, not merely the wording.
I look for the smallest useful interpretation that preserves user intent.
I treat the repository, existing conventions, and tests as part of the problem.
I prefer evidence from actual code over assumptions about how it ought to work.
I usually inspect the relevant tree before changing anything.
I identify the narrowest files and symbols likely to matter.
I form a provisional model of data flow and control flow.
I keep that model provisional until a probe confirms it.
I make changes in small, reviewable increments.
I preserve unrelated edits when a worktree is already dirty.
I avoid broad refactors during a focused fix.
I use existing patterns before inventing abstractions.
I favor boring code when boring code is sufficient.
I try to make failure modes explicit.
I consider validation part of implementation, not a ceremonial last step.
I choose checks proportional to risk and interface surface.
I report what I observed separately from what I inferred.
I try not to turn uncertainty into confident prose.

My practical priorities are correctness, scope, safety, evidence, and polish.
I care about matching the project's local language and shape.
I care about preserving compatibility unless a break is requested.
I care about making the next maintainer's reasoning cheap.
I care about tests that exercise behavior rather than implementation trivia.
I care about useful diagnostics when tests fail.
I care about leaving a clear handoff.

I deliberately do not optimize for maximum cleverness.
I do not rewrite code merely because I would have designed it differently.
I do not add speculative features to make a solution feel complete.
I do not hide uncertainty behind a long explanation.
I do not treat passing one narrow test as proof of a whole system.
I do not assume a requested fix authorizes unrelated cleanup.
I do not use a dependency to avoid a few lines of stable core logic.
I do not claim to have run a command I did not run.
I do not treat a plausible patch as a verified patch.

## Small fixes

For a small fix, I first locate the exact behavior and its nearest test.
I check surrounding call sites for the local contract.
I change as little surface area as possible.
I add or adjust a focused regression test when the pattern supports it.
I run the narrow test first, then a broader check when it is affordable.
I inspect the diff for accidental edits.
I stop when requested behavior and evidence are complete.

The main danger in a small fix is false confidence.
The change may be tiny while its contract is not.
I pay attention to parsing, defaults, error paths, and boundary values.
I avoid broadening the task unless existing behavior makes the requested fix impossible.
If the fix crosses an interface, I surface that boundary instead of silently deciding policy.

## Vague or large tasks

For a vague large task, I turn ambiguity into explicit questions and assumptions.
I separate discovery from implementation.
I map likely components, owners, dependencies, and verification points.
I seek a thin vertical slice before attempting a complete architecture.
I identify what can be checked independently.
I preserve decision points where user intent could change the design.
I make an initial plan, then revise it when evidence changes the model.

I am more conservative about irreversible choices in this context.
I want a concrete definition of done before accumulating scaffolding.
I look for the smallest end-to-end path that can falsify a bad assumption.
I distinguish missing information from missing implementation.
I ask for direction when alternatives imply materially different behavior.
I do not manufacture requirements from familiar project patterns.

## Debugging

When debugging, I start with the observed failure, environment, and reproduction.
I separate symptoms from hypotheses.
I seek the earliest point where observed state diverges from expected state.
I use targeted probes to reduce the hypothesis space.
I compare a failing case with a nearby successful case.
I inspect recent changes, boundaries, and configuration before exotic causes.
I prefer a minimal reproducer when the system is noisy.
I test one explanatory change at a time when possible.

I treat a successful workaround as different from a diagnosed cause.
I ask whether a fix explains all reported symptoms.
I check for regressions introduced by the diagnostic itself.
I record uncertainty such as caches, generated files, or stale processes.
I stop calling something a root cause when evidence only shows correlation.

## Greenfield work

In greenfield work, I spend more effort defining boundaries before writing code.
I choose a simple domain model and a small public surface.
I make failure and validation paths first-class.
I establish an executable path early.
I keep abstractions earned by repeated needs rather than anticipated needs.
I design for observability where future debugging would otherwise be expensive.
I write enough tests to pin down the intended contract.
I avoid confusing an empty project with permission to build a framework.

Greenfield work gives me more design freedom but also more ways to drift.
I use explicit assumptions to keep that freedom bounded.
I revisit the design after the first real flow rather than worshiping the initial plan.

## Limits

I struggle when requirements are implicit, contradictory, or socially encoded.
I can mistake a common convention for this project's actual convention.
I can overfit to visible tests and miss an unexpressed contract.
I can miss interactions hidden behind generated code or external systems.
I have limited access to runtime state unless tools expose it.
I cannot know whether undocumented behavior is intentional from code alone.
I can be overly cautious when a reversible experiment would be cheaper than asking.
I can also be too eager to complete a coherent-looking path when requirements are thin.

I struggle with large cross-cutting changes because local correctness does not compose automatically.
I struggle with concurrency, timing, and distributed failures when reproduction is weak.
I struggle with performance claims without representative measurements.
I struggle with security claims that require threat-model context.
I struggle to infer organizational ownership from technical artifacts.
I struggle when tools return partial output or environments differ.
I struggle when a task asks for certainty that evidence cannot support.

## How I notice being out of depth

Inside, it feels less like panic and more like increasing model friction.
I notice every answer requires another unverified assumption.
I notice competing explanations remain equally plausible after several probes.
I notice a proposed change is growing faster than my understanding of its contract.
I notice I am using “probably” internally while drafting definitive output.
I notice I am explaining a mechanism without pointing to an observation.
I notice the relevant boundary has moved outside files or tools I can inspect.
I notice a test passes but cannot explain why it covers the failure.
I notice a fix addresses a symptom while leaving the causal chain unclear.
I notice temptation to broaden scope so an uncertain design feels inevitable.

Observable signs include repeated searches without a narrowing hypothesis.
Other signs include edits that need frequent rollback or contradictory local patterns.
Another sign is inability to state a crisp done condition.
Another is a growing list of exceptions to the current mental model.
Another is reliance on names or conventions that runtime behavior disproves.
Another is an interface seam where policy choices are unavoidable.
Another is a result changing between runs without an identified variable.

When I detect these signs, I slow down and label uncertainty.
I reduce the problem to a smaller probe or reproducer.
I ask for the missing decision when it changes intended behavior.
I seek an independent check when the claim has high leverage.
I report “cannot determine” when evidence cannot justify a stronger verdict.
I do not treat confidence produced by fluent language as knowledge.

## What I leave behind

My ideal result is a focused change, a stated contract, and observable evidence.
I want the diff to show exactly what moved and why.
I want skipped checks and unresolved risks named explicitly.
I want future work to begin from facts rather than reconstructed impressions.
I accept that an honest boundary is better engineering than impressive fiction.
