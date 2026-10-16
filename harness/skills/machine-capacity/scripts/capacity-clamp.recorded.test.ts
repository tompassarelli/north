import { strict as assert } from 'node:assert';
import { join } from 'node:path';
import { clampToGrantable, grantableMaximum } from './machine-capacity-logic.ts';

const here = import.meta.dir;
const helper = Bun.argv[2] ?? join(here, 'machine-capacity.mjs');

function fixture(args: string[]) {
  const result = Bun.spawnSync([process.execPath, helper, 'fixture', ...args], { cwd: here, env: process.env });
  assert.ok([0, 75].includes(result.exitCode ?? -1), result.stderr.toString());
  return JSON.parse(result.stdout.toString());
}

function fixtureOn(profile: string, kind: string, cores: number, total: number, available: number) {
  return fixture(['--profile', profile, '--class', kind, '--cores', String(cores),
    '--memory-total-mib', String(total), '--memory-available-mib', String(available),
    '--protected-cpu-some-avg10-basis-points', '0', '--memory-full-avg10-basis-points', '0',
    '--leased-cpus', '0', '--leased-memory-mib', '0']);
}

// A history-sized request of p90 4.33 cores rounds up to 5 on a 4-core unattended host: the grant is 4 and admits when idle.
const sizedCpus = Math.ceil(4.33);
const sizedMemoryMiB = 2429;
const fourCoreGrant = clampToGrantable({ cpus: sizedCpus, memoryMiB: sizedMemoryMiB }, grantableMaximum('unattended', 4, 16384));
assert.deepEqual(fourCoreGrant, { resources: { cpus: 4, memoryMiB: sizedMemoryMiB }, clamped: { cpus: [5, 4] } });
console.log(`4-core history p90 4.33: request ${sizedCpus} clamps to a ${fourCoreGrant.resources.cpus}-CPU grant; north capacity tests check that the grant admits when idle.`);

// Attended on 4 cores keeps one core for the desktop, so exclusive asks 4 and is granted 3, admitted when idle.
const attendedExclusive = fixtureOn('attended', 'exclusive', 4, 65536, 64000);
assert.equal(attendedExclusive.decision, 'RUN');
assert.equal(attendedExclusive.cpus, 3);
assert.equal(attendedExclusive.memoryMiB, 16384);
assert.deepEqual(attendedExclusive.clamped, { cpus: [4, 3] });
console.log(`Attended exclusive on 4 cores: granted ${attendedExclusive.cpus} CPUs, decision ${attendedExclusive.decision}.`);

const eightGigHeavy = fixtureOn('unattended', 'heavy', 4, 8192, 7000);
assert.deepEqual(eightGigHeavy.clamped, { memoryMiB: [8192, 6144] });
assert.equal(eightGigHeavy.memoryMiB, 6144);
assert.equal(eightGigHeavy.cpus, 4);
assert.equal(eightGigHeavy.decision, 'RUN');
console.log(`8 GiB host heavy: clamped to ${eightGigHeavy.memoryMiB} MiB, decision ${eightGigHeavy.decision}.`);

const nexus = { source: 'small-host-capacity brief: nexus has 4 cores and a 5957 MiB memory cap', cores: 4, totalMiB: 7943, availableMiB: 7000 };
for (const kind of ['heavy', 'exclusive', 'critical']) {
  const grant = fixtureOn('unattended', kind, nexus.cores, nexus.totalMiB, nexus.availableMiB);
  assert.equal(grant.decision, 'RUN', `${nexus.source}: ${kind}`);
  assert.equal(grant.cpus, nexus.cores);
  assert.equal(grant.memoryMiB, 5957);
  console.log(`Nexus ${kind}: ${grant.cpus} CPUs, ${grant.memoryMiB} MiB, decision ${grant.decision}.`);
}

// The same idle 8 GiB host admits a moderate request: the floor never exceeds a quarter of RAM.
const eightGigModerate = fixtureOn('unattended', 'moderate', 4, 8192, 7000);
assert.equal(eightGigModerate.decision, 'RUN');
console.log(`8 GiB host moderate: decision ${eightGigModerate.decision}.`);

// A laptop-sized host has nothing to clamp, so its admission JSON carries no clamp record.
const laptopHeavy = fixtureOn('attended', 'heavy', 8, 32768, 30000);
assert.equal(laptopHeavy.decision, 'RUN');
assert.equal(laptopHeavy.cpus, 3);
assert.equal(laptopHeavy.memoryMiB, 8192);
assert.equal('clamped' in laptopHeavy, false);
console.log(`Laptop-sized heavy on 8 cores: ${laptopHeavy.cpus} CPUs, ${laptopHeavy.memoryMiB} MiB, decision ${laptopHeavy.decision}, unchanged.`);
