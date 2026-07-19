// Minimal verdict harness (JS) for the North R-1 apparatus. An unexpected
// verdict forces process.exit(1) so the apparatus fails loud, matching the
// Clojure r1.harness in fram.
let pass = 0, fail = 0;
const lines = [];
export function check(label, expected, actual) {
  const ok = JSON.stringify(expected) === JSON.stringify(actual);
  ok ? pass++ : fail++;
  lines.push(`  ${ok ? "PASS" : "FAIL"} ${label} — expected=${JSON.stringify(expected)} actual=${JSON.stringify(actual)}`);
  return ok;
}
export function section(t) { lines.push(`\n== ${t} ==`); }
export function note(t) { lines.push(`  ---- ${t}`); }
export function finish() {
  for (const l of lines) console.log(l);
  console.log(`\nRESULT ${pass} passed, ${fail} failed`);
  if (fail > 0) {
    console.log("APPARATUS VERDICT: UNEXPECTED — exiting nonzero");
    process.exit(1);
  }
  console.log("APPARATUS VERDICT: all cells matched contract expectation");
}
