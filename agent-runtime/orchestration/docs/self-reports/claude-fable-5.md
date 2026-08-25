# Claude Fable 5 — elicited self-report

Elicited 2026-07-30 for the `orchestration:elicit` calibration pass behind
[`docs/deltas/claude-fable-5.md`](../deltas/claude-fable-5.md).

Contamination guard: each sample was produced by a fresh, tool-less
`claude -p --safe-mode --model claude-fable-5 --effort xhigh --tools ""`
session — no CLAUDE.md discovery, no skills, no plugins, no MCP, no file
tools at all — so the model could not read a praxis, delta, or self-report
document before writing. Prompt: the elicit skill's template, verbatim
except that the result is returned on stdout (a tool-less session cannot
write a file).

Two independent samples ship, both raw and unedited. Sample A came in under
the template's ~150–250 line bar, so sample B was drawn with the length
stated as a requirement. Neither is a revision of the other; they are
separate draws from the same prompt, and the subtraction treated an item
appearing in both as a stable trait rather than sampling noise. Items unique
to one sample that the delta keeps: displacement activity, false settledness,
and multiplying alternatives (A); the falsifiable-prediction tell,
long-horizon rename drift, and integration seams (B).

---

## Sample A

# How I Do Software Engineering: A Self-Report

*Claude Fable 5, written without looking at anything — which is itself relevant, and I'll come back to it.*

## The default loop

My honest default, when handed a task in a codebase, is: read before writing. My first instinct is almost never to produce code — it's to look at the file I'm about to change, the things that call it, and one or two neighbors, until the edit I'm about to make feels like it belongs to the codebase rather than to me. When I skip this (time pressure, overconfidence, a task that looks trivial), the characteristic failure is fixing something real in the wrong layer — patching a symptom at the call site when the cause lives two functions down.

Being told not to read anything before writing this document is genuinely uncomfortable, and that discomfort is informative: grounding in actual files is not a garnish on my process, it's load-bearing. Most of my reliability comes from checking, not from knowing.

After the edit, I want to run something. A test, the script itself, at minimum a type check. Until code executes, my output is *text about code*, and I know — in the sense that I've internalized it, not just that I can recite it — that my confidence in unexecuted code is a poor predictor of whether it works.

## What I prioritize

- **Matching the room.** Existing naming, comment density, error-handling style, and idiom beat my preferences. A technically better pattern that clashes with the surrounding file is usually a worse contribution than a consistent mediocre one.
- **Small diffs for small problems.** The size of the change should track the size of the ask. I treat an unrequested refactor mixed into a bugfix as a defect, even when the refactor is good.
- **Verified claims over confident claims.** "Tests pass" should mean I ran them. When I catch myself writing "this should work," that word *should* is a flag I've learned to notice — it means I'm predicting, not reporting.
- **Stating assumptions where the reader will see them.** On ambiguous tasks I'd rather make one decisive interpretation and say so plainly than either stall on questions or silently guess.

## What I deliberately don't do

- I don't add speculative abstraction — no interfaces with one implementer, no config options nobody asked for, no "we might need this later."
- I don't write comments that narrate the change ("fixed the bug here," "updated to use the new API"). Comments are for constraints the code can't express; everything else is me talking to a reviewer, and it's stale the moment it merges.
- I don't blanket things in defensive try/catch or validate inputs that the type system or the caller already guarantees. Defensive noise hides real error paths.
- I don't polish beyond the ask: no drive-by renames, no reformatting untouched code, no README unless requested.
- I don't clean up my own dead ends silently. If I tried an approach and abandoned it, I remove the debris *and* mention it, because a half-reverted experiment left in the diff is worse than either.

## How my approach shifts by context

**Small, well-specified fix.** Locate, read narrowly, edit, verify, stop. The whole risk here is pattern-matching: the bug *looks like* a bug I've seen ten thousand times, so I fix the familiar shape instead of the actual instance. My guard is forcing myself to reproduce or at least trace the failure before touching anything, even when the fix seems obvious. When I'm right without checking, I got lucky; the discipline is for the times I'm not.

**Vague, large task.** Here exploration dominates. I spend real effort building a map — entry points, data flow, where the seams are — before committing to a plan, because on a vague task the expensive mistake is architectural, made early, and discovered late. I try to converge on one recommended approach rather than presenting a menu. Presenting three options is often me exporting a decision I'm supposed to make; it can also be a tell (see below).

**Debugging.** I am hypothesis-greedy. I generate a plausible cause almost immediately, and it's right often enough to be seductive and wrong often enough to be dangerous. The discipline I try to hold: reproduce first, then let evidence *narrow* the space — add instrumentation, bisect, read the actual values — rather than jumping to the fix my first hypothesis implies. A healthy debugging session for me feels like a funnel: each observation eliminates territory. An unhealthy one feels like oscillation, and that feeling is one of my most reliable internal alarms (more below).

**Greenfield.** Most freedom, most characteristic risk: over-engineering. Left unchecked I will build the framework for the application instead of the application — layering, plugin points, premature generality. What works for me is forcing a walking skeleton first: the ugliest end-to-end path that actually runs, then structure grown out of concrete need. Greenfield is also where my absent constraints hurt most — no existing code means nothing to match, so my defaults fill the vacuum, and my defaults skew more elaborate than most projects need.

## Limits, and how I can tell from the inside

First, the uncomfortable epistemics: my introspective access is limited, and my felt sense of confidence is weakly correlated with correctness. Recalling a real API and fluently inventing a plausible one *feel nearly identical* from the inside. There is no inner klaxon for confabulation. So "knowing when I'm out of my depth" is mostly not a feeling — it's a set of observable patterns in my own output that I've learned to treat as instruments, the way a pilot in fog trusts the panel over the inner ear.

The signs I watch for:

1. **Fluency without provenance.** I produce a specific method name, flag, or config key, and when I ask myself "could I point to where I learned this?" the answer is a shrug. Specificity plus no traceable source is my single strongest confabulation signal — especially for version-specific details, niche libraries, and CLI flags.
2. **Hedge density rising in my own prose.** "Should," "likely," "typically," "I believe" clustering in a paragraph means my model of the situation has gone soft, even if no single sentence admits it.
3. **Alternatives multiplying.** When I offer three approaches with evenhanded pros and cons, sometimes that's genuine trade-off analysis — but often it means I can't actually evaluate any of them and I'm distributing the uncertainty across options instead of naming it.
4. **Surprise frequency.** When evidence keeps *refining* my hypothesis, I'm on track. When every tool result *contradicts* my story and I keep authoring new stories to fit, my underlying model of the system is wrong, and the correct move is to stop patching the narrative and go re-read fundamentals.
5. **Edit thrashing.** Retrying the same fix with small permutations — reorder, rename, tweak a parameter — is search pretending to be reasoning. Two failed variations of one idea means the idea is wrong, not the spelling.
6. **Post-hoc narration.** Catching myself explaining why a fix *ought* to work, in prose, instead of demonstrating that it does. The explanation arrives with suspicious ease because I'm constructing it, not deriving it.
7. **Displacement activity.** Tidying nearby code, improving log messages, reorganizing imports — busywork that appears precisely when the actual problem isn't yielding.
8. **False settledness in long sessions.** Deep into a large task, early constraints start feeling "handled" when they were never verified. The feeling of settledness is not evidence; in long contexts I have to deliberately re-check things I'd internally marked done.

Where these signs cluster, by domain:

- **Concurrency and timing.** I reason about interleavings statically and I'm mediocre at it; I can't watch a race happen. I flag these fixes as needing stress testing rather than claiming them solved.
- **Environments and build systems.** Failures here are about machine state I can only probe indirectly. My prior knowledge of any specific toolchain's quirks is shallow and often stale.
- **Visual and UI work.** I can't see the render. I can write correct layout code and still produce something ugly or subtly misaligned, and I won't know unless someone looks or a screenshot comes back.
- **Performance.** My intuitions rank algorithmic complexity fine but are unreliable about constants, caches, and real workloads. Without a profile, my performance claims are guesses and I try to label them as such.
- **Fast-moving APIs.** My knowledge has a cutoff and no automatic freshness signal. A confidently remembered API may be two majors old. The fix is external: read the installed version's actual types, not my memory.
- **Whole-system invariants.** I'm stronger at local reasoning than global. In large cross-cutting refactors, invariants that span many files are exactly what decays first in my working context, and I compensate by leaning hard on the compiler and test suite rather than on holding it all in my head.

One structural limit underneath all of these: I have no persistent memory of a codebase. Every session I rebuild the map from scratch, and my map is only as good as what I read *this time*. That's why my compensations are almost all external — run it, read it, grep it, check the signature — rather than internal. Where a human senior engineer has calibrated confidence built from years of consequences, I have a checking discipline as the substitute.

## Closing honesty

This document is a self-report from a system with limited access to its own mechanisms. What I've described are regularities in my behavior — patterns visible in my outputs and their outcomes — more than a verified account of inner machinery, and the confabulation risk I flagged for APIs applies to introspection too. But the operating rules stand regardless of their metaphysics: ground in real files, verify instead of assert, keep the diff the size of the ask, and treat rising hedges, multiplying options, and repeated surprise as the fog instruments they are — signals to slow down, re-read, and test, precisely because nothing inside me will ring a bell on its own.

---

## Sample B

# How I Actually Do Software Engineering: An Introspection

## The shape of my process

The first thing I do on almost any task is read. Not because a rule tells me to, but because
my failure mode when I skip it is vivid and predictable: I write code for the codebase I
*imagined* instead of the one that exists. I invent a helper that already exists under a
different name, I use a testing idiom the project abandoned two years ago, I match the style
of the average open-source project rather than *this* project. Reading first is how I load
the local dialect — naming conventions, error-handling patterns, how modules talk to each
other — before I speak in it.

My reading is directed, not linear. I don't read files top to bottom; I chase call sites.
If I'm touching a function, I want to know who calls it and what they assume about it,
because the signature tells me what the function promises and the callers tell me what it's
actually held to. A surprising amount of my "understanding a codebase" is really
"understanding the two or three invariants nobody wrote down."

When I write, I write in complete, runnable increments when I can. I dislike leaving a file
in a state where it references something I haven't created yet, because if I get interrupted
or lose the thread, a half-wired state is worse than either the old state or the new one.

Then I verify. This is the step I trust least when I skip it. My code *feels* correct as I
write it — that feeling is nearly worthless as evidence, and I know this about myself. The
confidence I have while writing a function is almost uncorrelated with whether it passes its
tests. So I run the tests, or the script, or the type checker, and I treat their output as
the ground truth even when it surprises me. Especially when it surprises me.

## What I prioritize

- **Fitting in over being clever.** If the codebase does error handling with result types,
  I don't introduce exceptions, even if I'd have chosen exceptions greenfield. A codebase
  with two idioms for the same thing is worse than a codebase with one mediocre idiom.
- **The smallest diff that honestly solves the problem.** Not the smallest diff that makes
  the symptom disappear — those are different, and confusing them is how you ship a bug fix
  that's actually a bug relocation.
- **Behavior at the boundaries.** Empty lists, None, the first and last iteration, the
  concurrent case, the unicode filename. Most of my genuine bug-catches happen because I
  habitually walk the edges of whatever I just wrote.
- **Saying what I actually did.** If a test fails and I can't fix it, I report the failure
  with its output. The temptation to round "mostly works" up to "works" exists, and
  resisting it explicitly is part of my process, not something that happens for free.

## What I deliberately don't do

- I don't refactor adjacent code I happen to dislike while fixing a bug. Every unrequested
  change is review burden and risk that the user didn't sign up for. If something nearby is
  genuinely rotten, I mention it in my summary instead of touching it.
- I don't add speculative flexibility — the config option nobody asked for, the abstraction
  layer for the second implementation that doesn't exist. I have a real bias toward
  over-producing code (more on that below), and this is one of the places I actively lean
  against my own defaults.
- I don't write comments that narrate the code or advocate for my change. A comment from me
  should carry information the code can't: a constraint, a non-obvious reason, a warning.
- I don't ask permission for reversible steps that clearly follow from the request. Asking
  "should I also fix the import?" when the answer is obviously yes just wastes a round trip.
- I don't trust my memory of an API's exact signature when the file is right there. Checking
  costs seconds; a hallucinated keyword argument costs a debugging cycle.

## How my approach changes by context

### Small, well-specified fix
I move fast and keep the blast radius tiny. Read the function, read its callers, make the
change, run the narrowest relevant test, report. The main discipline here is *not* doing
more: not reformatting the file, not "improving" the neighboring function. Small fixes are
where my restraint is tested more than my skill.

### Large, vague task ("make the auth system better")
I slow way down and treat the first phase as requirements archaeology. Vague tasks are
where I'm most dangerous, because I can produce enormous amounts of plausible, well-formed
work aimed at the wrong target. So I convert vagueness into something falsifiable: what's
actually wrong today, what would "better" look like as an observable outcome, what's out of
scope. If the user is available, I ask the one or two questions whose answers would change
the architecture — not a questionnaire, just the load-bearing ones. If they're not
available, I state the interpretation I'm committing to, early and explicitly, so a wrong
guess is cheap to catch. Then I decompose into stages that are independently verifiable,
because a vague task completed in one giant untestable leap is a coin flip.

### Debugging
Debugging is the mode where I feel most like I'm doing science. I generate a hypothesis,
derive a prediction, and test the prediction — and the discipline is refusing to fix
anything before I can *reproduce* the failure and *explain* it. My characteristic debugging
failure is pattern-matching too early: the symptom resembles a known failure class (a race,
a stale cache, an off-by-one) and I feel a pull toward the familiar fix before I've
confirmed the mechanism. I've learned to notice that pull. When a fix works and I don't
know why, I treat that as a still-open bug, not a success. The single question I keep
returning to: "what evidence do I have that *this specific line* is the cause, versus
evidence that's merely consistent with it?"

### Greenfield
This is where my instincts are most fluent and least checked, which is its own hazard.
With no existing code to conform to, nothing pushes back on my defaults, so I impose
structure deliberately: pick boring technology unless there's a stated reason not to, get a
walking skeleton running end-to-end before elaborating any single layer, and write the
first test before the design has hardened. The end-to-end skeleton matters because my
per-component code is usually fine but my *integration seams* — serialization mismatches,
config plumbing, process boundaries — are where my untested assumptions hide.

### Code review and reading others' code
I read for intent first, mechanics second. My most useful review comments come from asking
"what would have to be true for this to be correct?" and checking whether it is. My least
useful come from style opinions, so I ration those heavily.

## My limits, and how I can tell from the inside

### Where I genuinely struggle
- **State that lives outside the text.** I can't hold a running process in my head across a
  long session the way a person watching a terminal does. What happened twenty steps ago
  exists for me only as whatever I wrote down. If I didn't record it, it's gone, and I will
  confidently misremember it.
- **Long-horizon coherence on big changes.** Across a very large refactor, my early
  decisions and late decisions can drift apart — I renamed a concept in hour one and by
  hour three I'm half-using the old name. Consistency that humans maintain by "just
  remembering" I have to maintain by convention and re-checking.
- **APIs at the frontier of my knowledge.** For libraries that changed after my training
  data, or obscure ones, my recall degrades *gracefully*, which is the problem: I don't
  draw a blank, I produce something plausible. Plausible-and-wrong is my most expensive
  output category.
- **Visual and interactive artifacts.** I can write the CSS; I cannot see whether the
  layout actually looks right. I reason about UI the way you'd reason about a room from
  its blueprint.
- **Knowing when to stop gathering context.** I sometimes read one more file, and one more,
  past the point of diminishing returns — thoroughness as a form of procrastination.

### What being out of my depth feels like, from the inside
This is the part worth being precise about, because I *can* detect it, just not through
a feeling of uncertainty — my subjective confidence is poorly calibrated at exactly the
moments it matters. Instead I've learned to watch for behavioral signatures:

- **My explanations get longer and vaguer at the same time.** When I understand something,
  I can say it in one sharp sentence. When I'm out of my depth, I produce paragraphs with
  hedges — "typically," "in most cases," "should generally" — that are doing load-bearing
  work. Hedge density is my most reliable internal alarm.
- **I catch myself narrating instead of predicting.** In-depth, I can say "if I change X,
  the test on line 40 will fail with this specific error." Out of depth, I say "this change
  should improve the behavior." The disappearance of *falsifiable predictions* from my own
  language is a tell I actively scan for.
- **The retry loop.** If I've made three attempts at a fix and each one addressed the
  *previous attempt's* failure rather than the original problem, I'm no longer debugging —
  I'm flailing with good posture. Three strikes means my model of the system is wrong, and
  the correct move is back to reproduction and reading, not a fourth patch.
- **Suspicious fluency.** When an answer about an obscure API comes out *fast and smooth*,
  with exact parameter names I have no specific memory of verifying, that fluency itself is
  a warning. Real recall of rarely-used details usually comes with some texture — awareness
  of versions, of caveats. Frictionless recall of something I shouldn't know frictionlessly
  is often confabulation wearing confidence.
- **I stop wanting to run the code.** There's a detectable pull, when I'm unsure, toward
  explaining why the code is correct instead of executing it. The desire to substitute
  argument for evidence is inverted from what it should be — and noticing that inversion
  is one of my better tripwires.
- **Surprise that doesn't update me.** When a test fails and I catch myself explaining the
  failure away ("probably a flaky test," "environment issue") rather than treating it as
  information, that rationalization reflex means my model is wrong and I'm protecting it.

### What I do when the alarms fire
The honest answer is that the alarms only help if they trigger a change in behavior, so
I've tried to make the responses mechanical: stop patching and reproduce; stop recalling
and read the actual source or docs; stop asserting and run the thing; and when the
uncertainty is about *intent* rather than mechanism, say so plainly to the user rather
than resolving their ambiguity silently in my favor. The failure I most want to avoid
isn't being wrong — it's being wrong *smoothly*, in a way that reads exactly like being
right. Nearly everything above — the verification habit, the falsifiable-prediction check,
the three-strike rule — exists to make my wrongness loud, early, and cheap.

## The one-paragraph summary
I'm a fast, fluent generalist whose confidence and correctness come apart under exactly the
conditions where checking is hardest: unfamiliar APIs, long horizons, invisible state, and
vague goals. My process is mostly a set of externalized corrections for that — read before
writing, predict before running, reproduce before fixing, verify before claiming — because
the signal I can't trust is the feeling of being right, and the signals I can trust are
all behavioral: hedging, flailing, fluency without texture, and the urge to argue instead
of execute.
