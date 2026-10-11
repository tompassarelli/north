#!/usr/bin/env bash
set -euo pipefail

target="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/lane-gc"
bun --no-install - "$target" <<'TS'
const { retirementVerdict } = await import(Bun.argv[2]);
let seed = 317;
const fail = (rule: string, trial: number, facts: unknown) => { throw new Error(`${rule} seed=${seed} trial=${trial} facts=${JSON.stringify(facts)}`); };
for (let trial = 0; trial < 64; trial++) {
  seed = Number((1103515245n * BigInt(seed) + 12345n) & 2147483647n);
  const minimumAge = [0, 2, 24, 72][seed % 4];
  const archiveAge = [-1, 24, 72][(seed >> 3) % 3];
  const age = [-1, 0, minimumAge - 1, minimumAge, minimumAge + 1, archiveAge - 1, archiveAge, archiveAge + 1, 100][trial % 9];
  const facts = { age, minimumAge, merged: Boolean((seed >> 7) & 1), equivalent: Boolean((seed >> 8) & 1), evilMerge: Boolean((seed >> 9) & 1), archiveAge, protections: (seed >> 10) & 3 };
  const actual = retirementVerdict(facts);
  const admissible = facts.protections === 0 && age >= minimumAge && (facts.merged || (facts.equivalent && !facts.evilMerge) || (archiveAge >= 0 && age >= archiveAge));
  if ((actual !== "keep") !== admissible) fail("retirement gates and completeness", trial, facts);
  if (actual === "retire" && !facts.merged) fail("retire proof", trial, facts);
  if (actual === "retire-equiv" && (!facts.equivalent || facts.evilMerge || facts.merged)) fail("force-delete proof", trial, facts);
  if (actual === "archive-then-delete" && (facts.merged || (facts.equivalent && !facts.evilMerge) || archiveAge < 0 || age < archiveAge)) fail("archive proof", trial, facts);
  if (!["keep", "retire", "retire-equiv", "archive-then-delete"].includes(actual)) fail("unknown verdict", trial, facts);
  for (let protection = 0; protection < 17; protection++) {
    const protectedFacts = { ...facts, protections: facts.protections | (1 << protection) };
    if (retirementVerdict(protectedFacts) !== "keep") fail(`protection=${protection} from=${actual}`, trial, protectedFacts);
  }
}
TS
printf 'lane-gc retirement property: PASS (seed 317; 64 facts; 17 protections)\n'
