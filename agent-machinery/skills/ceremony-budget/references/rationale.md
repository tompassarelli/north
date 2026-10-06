# Why ceremony-budget exists

Not required to apply the skill. Read this when revising it, when
deciding whether a proposed addition belongs, or when the skill appears to
be failing at a case it should have caught.

## The failure it corrects

Residual uncertainty gets treated as a debt the agent must personally retire
through more work. More assurance always looks marginally justified, so the
loop never closes and the requested artifact slips. The observed surface
forms differ — provenance, attestation, hardening, test isolation, "while I'm
here" fixes — but the mechanism underneath is one thing: **discomfort is
being read as evidence.** Every rule in `SKILL.md` is a way of
refusing that inference.

This is why the skill's centre of gravity is the negative list rather than
the positive one. An agent that wants to escalate can construct a
justification for almost anything; what actually changes behaviour is a
short, memorable list of the specific justifications that do not count.

## Observed incidents behind it

- 2026-08-24/25 — operator, verbatim: "i just don't understand the
  provenance and attestation paranoia that seems to highjack productivity
  constantly, can you figure out a saner solution to this pattern."
- 2026-09-03 — operator, verbatim: "JESUS MOTHER FUCKING CHRIST WHY IS THERE
  SO MUCH FUCKING CEREMONY IN DOING THIS."
- 2026-09-07 — operator named the recurring set directly: immutability /
  reproducibility / attestation / provenance work that is "extraordinarily
  difficult" for no gain; security paranoia that fits neither the research
  project nor, often, the production one; speculative fixing of self-noticed
  "gaps" off the critical path; and test discipline that is "pedantic and
  overly ceremonially, academically correct, but not really necessary"
  instead of batching aggressively. Same turn asked for the polish level to
  become an explicit, operator-visible setting rather than an inference.

The recurrence is the point. A single instance would be a deferred note.

## Lineage — and the deletion that caused the relapse

A concrete mechanism for this already existed and was lost. The operator
policy repository carried `dotfiles/agents/docs/verification-doctrine.md`
(canonical 2026-07-28), which held a P0–P3 paranoia ladder keyed on blast radius ×
reversibility, fixed at intake, plus a paste-able mid-flight override for
misbehaving lanes. Commit `82e30538` (2026-07-30) deleted it during a
projection consolidation, with no replacement.
The August and September complaints above are that removal coming due.

The current three settings — Ship / Research / Durable — are the light form
of that ladder. Two properties were deliberately kept: the setting is
**fixed at intake**, and it moves **only on a fact that was not true
before**. The heavy apparatus (attestations, staged canaries, rollback
probes, coverage enumerations) was deliberately not carried forward: it is
itself the ceremony being complained about, and nothing in the operator's
current work has a consumer for it. Restore a tier from that ladder only
when a specific artifact has a named external consumer that needs it — not
because the fuller ladder is more complete.

## Anti-pattern index, recovered

From the deleted doctrine, §5. Retained because naming a tarpit is what
lets an agent recognise it from the inside; a Bad/Good pair only works if
you already suspect you are in one. Verification-specific rows belong to
`verification`; the escalation rows belong here.

| Anti-pattern | Signature | Correct move |
|---|---|---|
| Effort-as-evidence | "I reviewed extensively…" with no observation | Demand probe + output, or discard the claim |
| Anxiety escalation | Tier grows mid-flight without a new fact | Restate the intake setting; escalate only by naming the new fact |
| Coverage theater | "One more check," sampling worries in anxiety order | Verify against the contract, not against unease |
| Soak loop | N≥k reruns of a deterministic claim | One run; convert flakiness into one deterministic test |
| Policy churn | Re-deriving the funnel each cycle instead of running the next probe | The funnel is fixed; execute |
| Scope self-expansion | Absorbing newly found risks into the current pass | Classify: fail now, or new thread |
| Dispositionless verification | A pass that ends "continuing to investigate" | Forbidden; emit pass / fail / cannot-determine now |
| Archaeology substitution | Source reading standing in for an unrunnable probe | `cannot-determine` and route to a capable environment |
| Authority laundering | Escalating for permission the contract already grants | Execute; deference is not diligence |

Full deleted text, if a case needs it: in the operator policy repository,
`git show 82e30538^:dotfiles/agents/docs/verification-doctrine.md`.

## Bounds on this skill

It governs how much assurance is proportionate. It does not lower a real
gate, weaken a test to make it pass, excuse a known defect, or authorise
skipping bounded correctness for the requested claim. Safety, secret
handling, and destructive-operation boundaries are outside its scope and
never trade against delivery speed.
