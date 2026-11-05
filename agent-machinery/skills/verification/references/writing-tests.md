# Writing and changing tests

Read before writing or changing a test. The burden is on the test: it names the guarantee it protects, in its title, as a behaviour a person would notice breaking.

## Earn the test
- Before writing a test, ask what is the fastest check that would fail if the guarantee broke (a type, a table, a property over a pure core, one seeded scenario) and write that one, because cheaper tests keep the loop fast. Reach for a sweep, whole-roster run, native pair or committed record only when nothing cheaper can fail.
- Tighten only where a bug escaped, extending the property, scenario or reference that should have caught it, because escapes, not fear, buy strictness.
- A test that proves a just-built feature or port works (parity against the code it replaces, a check that the new command runs) is landing evidence, not a regression test: cite its run in the commit and commit it only if it is also the cheapest check of a lasting guarantee, because scaffolding outlives the port and freezes the code.
- Remove the need first: one table over every case, types that rule out bad states, one source of truth generating the rest, because a bug that can't be written needs no test.
- Keep logic in a pure deterministic core fed time, randomness and peers as inputs; test through the caller's interface and fake only what we don't own, because fakes of our own code drift from it.
- Product code never branches on a test's literal inputs, because that passes tests while breaking the product.

## Admission
- A test is kept only if it protects a guarantee a person would notice breaking and it reproduces a named bug, alone kills a recorded mutant, or pins an external contract. Three layers enforce it, with no per-test reviewer (Tom's choice, 2026-11-06). After the push, every Rust test the lane adds next to a code change must fail on base with the lane's non-test changes reverted (`land scan test-admission run BASE..HEAD`, a `land.after` line run on a build host), or the commit declares `Behavior: unchanged`; a test that passes there guards nothing and red main names the lane. A test kept for a contract carries `Test-contract: <test name>; category: <category>` with a category from the repository's closed `test-admission.contract` list in `.safe-push`, and the landing refuses any other category; such a test does not count against the budget below. When the repository's median green post-land run exceeds its verification budget (`verification_budget_seconds`, 20 s), a lane adds tests only by deleting as many. Within budget, the after-push fail-to-pass check is the only layer that judges an added test.

## Five kinds of test worth keeping
1. **Recorded scenarios**: real or seeded input through the whole system, checking invariants and agreement; keep one injected fault that must trip it.
2. **Properties over a pure core**: generated inputs against bounds, round-trips or a reference model; locally a fixed seed and a few hundred cases; wide search after landing; keep shrunk failures named.
3. **Product measurements**: owner-decided numbers whose source the box or a rationale doc names; a missed soft one lands with `Target-debt:` ([planning](../../planning/SKILL.md)).
4. **External references**: truth from a spec, reference implementation or recorded real output, cited in the title or fixture.
5. **One integration check per real boundary**, run end to end.

Which kind carries the weight in a domain: [testing domains](testing-domains.md).
