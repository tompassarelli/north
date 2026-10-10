import { readFileSync } from 'node:fs';
import { estimateCapacity, type CapacityRun } from './machine-capacity-logic.ts';

const fixture = process.argv[2] ?? new URL('./capacity-usage.recorded.jsonl', import.meta.url);
const rows: CapacityRun[] = readFileSync(fixture, 'utf8').trim().split('\n').map(line => JSON.parse(line));
rows.sort((a, b) => a.at.localeCompare(b.at));
const cutoff = Date.parse(rows.at(-1)!.at) - 7 * 86400000;
const earlier: CapacityRun[] = [];
const scored: { signature: string; measured: number; estimate: number; error: number }[] = [];
let missing = 0;
let defaults = 0;
let exact = 0;
let nearest = 0;
const relativeError = (estimate: number, measured: number) => measured === 0 ? estimate === 0 ? 0 : Infinity : Math.abs(estimate / measured - 1);
if (relativeError(2, 1) <= 0.3) throw new Error('Injected prediction fault escaped the replay detector');
for (let index = 0; index < rows.length;) {
  const end = rows.findIndex((run, next) => next > index && run.at !== rows[index].at);
  const group = rows.slice(index, end < 0 ? rows.length : end);
  for (const row of group) {
    if (Date.parse(row.at) < cutoff) continue;
    if (row.p90Cores === null || !Number.isFinite(row.p90Cores)) { missing += 1; continue; }
    const result = estimateCapacity(row.signature, earlier, 24);
    if (result.matchLevel === 'default') { defaults += 1; continue; }
    if (result.matchLevel === 'exact') exact += 1;
    else nearest += 1;
    const estimate = result.cores.p90!;
    scored.push({ signature: JSON.stringify(row.signature), measured: row.p90Cores, estimate, error: relativeError(estimate, row.p90Cores) });
  }
  earlier.push(...group);
  index += group.length;
}
const passed = scored.filter(run => run.error <= 0.3).length;
const share = scored.length === 0 ? 0 : passed / scored.length;
console.log(`Recorded replay: ${rows.length} rows; last 7 days ending ${rows.at(-1)!.at}; strictly earlier timestamps only`);
console.log(`Excluded: missing p90=${missing}; declared defaults=${defaults}; scored exact=${exact}; nearest=${nearest}`);
console.log(`Within ±30%: ${passed}/${scored.length} = ${(share * 100).toFixed(2)}%; required >=80%`);
console.log('Worst 5 signatures (largest relative error across every scored run):');
const worst = new Map<string, typeof scored[number]>();
for (const row of scored) if (!worst.has(row.signature) || row.error > worst.get(row.signature)!.error) worst.set(row.signature, row);
for (const row of [...worst.values()].sort((a, b) => b.error - a.error).slice(0, 5)) {
  console.log(`${row.signature} measured=${row.measured} estimate=${row.estimate} error=${Number.isFinite(row.error) ? (row.error * 100).toFixed(2) + '%' : 'infinite (measured zero)'}`);
}
if (share < 0.8) process.exitCode = 1;
