import { strict as assert } from 'node:assert';
import { mkdtempSync, mkdirSync, readFileSync, writeFileSync, rmSync } from 'node:fs';
import { join } from 'node:path';
import { tmpdir } from 'node:os';
import { runningCpuCommitment } from './machine-capacity-logic.ts';

const here = import.meta.dir;
const fixture = await Bun.file(join(here, 'capacity-admission.recorded.json')).json();
const scratch = mkdtempSync(join(tmpdir(), 'capacity-admission-recorded-'));
const root = join(scratch, 'agent-capacity-v1');
mkdirSync(join(root, 'leases'), { recursive: true });
mkdirSync(join(root, 'queue'));
writeFileSync(join(root, 'mode'), 'present\n');
writeFileSync(join(scratch, 'gpu'), '0\n');
const wrapperStart = readFileSync(`/proc/${process.pid}/stat`, 'utf8').split(') ')[1].split(' ')[19];
const ids: string[] = [];
for (const [index, holder] of fixture.holding.entries()) {
  const id = crypto.randomUUID();
  ids.push(id);
  writeFileSync(join(root, 'leases', `${id}.json`), JSON.stringify({
    ...holder, id, class: holder.class, owner: `fixture:${index}`, aggregateSlice: holder.class === 'native' ? 'native.slice' : 'agent-capacity.slice',
    createdAt: Date.now(), expiresAt: Date.now() + 60000,
    ...(holder.kind === 'run' ? { wrapperPid: process.pid, wrapperStart } : {}),
  }));
}
const env = {
  ...process.env, XDG_RUNTIME_DIR: scratch, AGENT_CAPACITY_CPU_PRESSURE: join(scratch, 'pressure'),
  AGENT_CAPACITY_GPU_BUSY: join(scratch, 'gpu'), AGENT_CAPACITY_GPU_CLIENTS: '0',
};
function snapshot(protectedPressure: number, overload = false) {
  const now = Date.now();
  writeFileSync(join(scratch, 'pressure'), `some avg10=${protectedPressure} avg60=0 avg300=0 total=0\n`);
  writeFileSync(join(root, 'unleased.json'), JSON.stringify({ at: now, windowSeconds: fixture.unleasedHeavy.windowSeconds, heavy: [{ cores: fixture.unleasedHeavy.cores }] }));
  for (const [index, holder] of fixture.holding.entries()) {
    if (holder.measured === null) continue;
    writeFileSync(join(root, 'leases', `${ids[index]}.usage`), JSON.stringify({
      ...holder.measured, recentCores: overload && holder.class === 'moderate' ? fixture.aggregateCpuLimit : holder.measured.cores, at: now,
    }));
  }
  const invoke = (args: string[]) => {
    const result = Bun.spawnSync([process.execPath, join(here, 'machine-capacity.mjs'), ...args], { env });
    return JSON.parse(result.stdout.toString());
  };
  const status = invoke(['status']);
  const probe = invoke(['probe', '--class', 'moderate']);
  assert.equal(status.committedBatchCpus, probe.committedBatchCpus);
  assert.equal(status.aggregateCpuLimit, fixture.aggregateCpuLimit);
  writeFileSync(join(scratch, 'status.json'), JSON.stringify(status));
  const gate = Bun.spawnSync(['python3', '-c', `import sys,json;sys.path.insert(0,sys.argv[1]);import spawn_capacity;print(json.dumps(spawn_capacity.check({'tool_name':'spawn_agent','tool_input':{'message':'recorded capacity scenario'}})))`, join(here, '../../../hooks/lib')], {
    env: { ...env, PYTHONDONTWRITEBYTECODE: '1', SPAWN_CAPACITY_STATUS: join(scratch, 'status.json'), SPAWN_CAPACITY_HOME: scratch, AGENTS_ORG_FILE: join(scratch, 'no-org'), AGENTS_ORCHESTRATION: join(scratch, 'no-priority') },
  });
  assert.equal(gate.exitCode, 0, gate.stderr.toString());
  return { status, probe, denied: JSON.parse(gate.stdout.toString()) !== null };
}
try {
  mkdirSync(join(scratch, '.local/state/agents'), { recursive: true });
  writeFileSync(join(scratch, '.local/state/agents/usage-gate.json'), JSON.stringify({ codex: { epoch: Date.now() / 1000, stale_min: 30, refusal: null } }));
  const baseline = snapshot(fixture.protectedCpuSomeAvg10);
  const old = fixture.holding.filter((holder: any) => holder.kind === 'run' && holder.class !== 'native').reduce((sum: number, holder: any) => sum + Math.max(holder.cpus, holder.measured?.cores ?? 0), fixture.unleasedHeavy.cores);
  assert.equal(old, 3.69);
  assert.equal(baseline.status.committedBatchCpus, 4.14);
  assert.equal(baseline.probe.reason, 'RUN');
  assert.equal(baseline.denied, false);
  console.log(`Recorded successful baseline: old=${old} new=${baseline.status.committedBatchCpus} limit=${fixture.aggregateCpuLimit}; both admit. Original refusal evidence remains missing.`);
  const overloaded = snapshot(0, true);
  assert.equal(overloaded.probe.reason, 'DEFER_CPU_CAPACITY');
  assert.equal(overloaded.denied, true);
  console.log(`Synthetic measured-use overload: measured=${fixture.aggregateCpuLimit} committed=${overloaded.status.committedBatchCpus}; helper and gate refuse.`);
  const protectedLoad = snapshot(25);
  assert.equal(protectedLoad.probe.reason, 'DEFER_INTERACTIVE_PRESSURE');
  assert.equal(protectedLoad.denied, true);
  console.log('Synthetic protected pressure: some avg10=25; helper and gate refuse.');
  assert.deepEqual(runningCpuCommitment('heavy', null, 6, 6), { cpus: 7.5, basis: 'estimate-missing-measurement' });
  assert.equal(runningCpuCommitment('native', 0.07, 2, 2).cpus, 0.08750000000000001);
  console.log('No-history heavy floor=7.5; idle native measured=0.07, committed=0.0875.');
} finally { rmSync(scratch, { recursive: true }); }
