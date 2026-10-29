import { readFileSync } from 'node:fs';
import { estimateCapacity, estimateDuration, type CapacityRun } from './machine-capacity-logic.ts';

const fixture = process.argv[2] ?? new URL('./capacity-usage.recorded.jsonl', import.meta.url);
const recorded: CapacityRun[] = readFileSync(fixture, 'utf8').trim().split('\n').map(line => JSON.parse(line));
const rows = recorded.filter(run => run.shape !== 'nix develop' && run.shape !== 'nix shell').sort((a, b) => a.at.localeCompare(b.at));
const cutoff = Date.parse(rows.at(-1)!.at) - 7 * 86400000;
const earlier: CapacityRun[] = [];
const scored: { signature: string; level: string; measured: number; estimate: number; error: number; floorPassed: boolean; covered: boolean; overReserved: boolean }[] = [];
let missing = 0; let defaults = 0;
const levels = ['exact', 'owner-shape', 'owner-prefix-shape-class', 'family-repo', 'family'];
const relativeError = (estimate: number, measured: number) => measured === 0 ? estimate === 0 ? 0 : Infinity : Math.abs(estimate / measured - 1);
if (relativeError(2, 1) <= 0.3) throw new Error('Injected prediction fault escaped the replay detector');
for (let index = 0; index < rows.length;) {
  const end = rows.findIndex((run, next) => next > index && run.at !== rows[index].at);
  const group = rows.slice(index, end < 0 ? rows.length : end);
  for (const row of group) {
    if (Date.parse(row.at) < cutoff) continue;
    if (row.p90Cores === null || !Number.isFinite(row.p90Cores)) { missing += 1; continue; }
    const result = estimateCapacity(row.signature, earlier, 24, { owner: row.owner, shape: row.shape });
    if (result.matchLevel === 'default') { defaults += 1; continue; }
    const estimate = result.cores.p90!;
    scored.push({ signature: JSON.stringify({ ...row.signature, owner: row.owner, shape: row.shape }), level: result.matchLevel,
      measured: row.p90Cores, estimate, error: relativeError(estimate, row.p90Cores),
      floorPassed: Math.abs(estimate - row.p90Cores) <= Math.max(0.3 * row.p90Cores, 0.25), covered: row.p90Cores <= 1.3 * estimate + 0.25, overReserved: estimate > 2 * row.p90Cores + 0.25 });
  }
  earlier.push(...group); index += group.length;
}
const passed = scored.filter(run => run.error <= 0.3).length, share = scored.length === 0 ? 0 : passed / scored.length;
console.log(`Recorded replay: ${rows.length} rows; last 7 days ending ${rows.at(-1)!.at}; strictly earlier timestamps only`);
console.log(`Excluded: nix develop/shell shapes recorded before inner-command shaping=${recorded.length - rows.length}; missing p90=${missing}; declared defaults=${defaults}; scored=${scored.length}`);
const report = (label: string, runs: typeof scored, predicate: (run: typeof scored[number]) => boolean) => {
  const count = runs.filter(predicate).length, ratio = runs.length === 0 ? 0 : count / runs.length;
  console.log(`${label}: ${count}/${runs.length} = ${runs.length === 0 ? 'N/A' : (100 * ratio).toFixed(2) + '%'}`);
  return ratio;
};
const measures: Record<string, (run: typeof scored[number]) => boolean> = { 'Coverage': run => run.covered, 'Over-reservation': run => run.overReserved, 'Strict ±30%': run => run.error <= 0.3 };
const coverage = report('Coverage primary (measured <= 1.3 x estimate + 0.25 cores; required >=95%)', scored, measures['Coverage']);
const overShare = report('Over-reservation primary (estimate > 2 x measured + 0.25 cores; required <20%)', scored, measures['Over-reservation']);
console.log(`Strict ±30% (printed, not gating): ${passed}/${scored.length} = ${(share * 100).toFixed(2)}%; the earlier gate required >=80% and ${share >= 0.8 ? 'met' : 'missed'} it`);
for (const [name, predicate] of Object.entries(measures)) for (const level of levels) report(`${name} ${level}`, scored.filter(run => run.level === level), predicate);
report('Comparison only: ±30% with 0.25-core absolute tolerance floor overall', scored, run => run.floorPassed);
for (const level of levels) report(`Comparison only: 0.25-core floor ${level}`, scored.filter(run => run.level === level), run => run.floorPassed);
console.log('Worst 5 signatures including anonymous owner/shape context (largest strict relative error across every scored run, strict view not gating):');
const worst = new Map<string, typeof scored[number]>();
for (const row of scored) if (!worst.has(row.signature) || row.error > worst.get(row.signature)!.error) worst.set(row.signature, row);
for (const row of [...worst.values()].sort((a, b) => b.error - a.error).slice(0, 5)) {
  console.log(`${row.signature} level=${row.level} measured=${row.measured} estimate=${row.estimate} error=${Number.isFinite(row.error) ? (row.error * 100).toFixed(2) + '%' : 'infinite (measured zero)'}`);
}
const failed = scored.length <= 379 || coverage < 0.95 || overShare >= 0.2;
console.log(`${failed ? 'FAIL' : 'PASS'} capacity estimate replay`);
if (failed) process.exitCode = 1;

const durationFixture = process.argv[3] ?? new URL('./capacity-duration.recorded.jsonl', import.meta.url);
const timed: CapacityRun[] = readFileSync(durationFixture, 'utf8').trim().split('\n').map(line => JSON.parse(line));
timed.sort((a, b) => a.at.localeCompare(b.at));
const timedEarlier: CapacityRun[] = [];
let durationScored = 0, durationCovered = 0, durationThin = 0;
for (let index = 0; index < timed.length;) {
  const end = timed.findIndex((run, next) => next > index && run.at !== timed[index].at);
  const group = timed.slice(index, end < 0 ? timed.length : end);
  for (const row of group) {
    const duration = estimateDuration(row.signature, timedEarlier);
    if (duration.p75Seconds === null) { durationThin += 1; continue; }
    durationScored += 1;
    if (row.wallSeconds! <= duration.p75Seconds) durationCovered += 1;
  }
  timedEarlier.push(...group); index += group.length;
}
const durationShare = durationScored === 0 ? 0 : durationCovered / durationScored;
console.log(`Duration replay: ${timed.length} exact-signature runs; uncalibrated (fewer than 3 earlier exact runs)=${durationThin}; scored=${durationScored}`);
console.log(`Duration p75 coverage: ${durationCovered}/${durationScored} = ${(durationShare * 100).toFixed(2)}%; required >=72%`);
const durationFailed = durationScored === 0 || durationShare < 0.72;
console.log(`${durationFailed ? 'FAIL' : 'PASS'} duration p75 replay`);
if (durationFailed) process.exitCode = 1;
