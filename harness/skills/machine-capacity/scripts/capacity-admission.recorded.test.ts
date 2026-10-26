import { strict as assert } from 'node:assert';
import { mkdtempSync, mkdirSync, readFileSync, writeFileSync, rmSync } from 'node:fs';
import { join } from 'node:path';
import { tmpdir } from 'node:os';
import { aggregateCpus, runningCpuCommitment } from './machine-capacity-logic.ts';

const here = import.meta.dir;
const fixture = await Bun.file(join(here, 'capacity-admission.recorded.json')).json();
const scratch = mkdtempSync(join(tmpdir(), 'capacity-admission-recorded-'));
const root = join(scratch, 'agent-capacity-v1');
mkdirSync(join(root, 'leases'), { recursive: true });
mkdirSync(join(root, 'queue'));
writeFileSync(join(root, 'mode'), fixture.profile === 'attended' ? 'present\n' : 'away\n');
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
  ...process.env, XDG_STATE_HOME: join(scratch, 'state'), AGENT_CAPACITY_DB: join(scratch, 'capacity.db'), XDG_RUNTIME_DIR: scratch, CAPACITY_WATCHDOG_DIR: scratch, AGENT_CAPACITY_CPU_PRESSURE: join(scratch, 'pressure'),
  AGENT_CAPACITY_GPU_BUSY: join(scratch, 'gpu'), AGENT_CAPACITY_GPU_CLIENTS: '0', AGENT_CAPACITY_CORES: String(fixture.cores),
};
function snapshot(protectedPressure: number, overload = false) {
  const now = Date.now();
  writeFileSync(join(scratch, 'pressure'), `some avg10=${protectedPressure} avg60=0 avg300=0 total=0\n`);
  writeFileSync(join(scratch, 'samples.jsonl'), `${JSON.stringify({ at: new Date(now).toISOString(), intervalSeconds: fixture.unleasedHeavy.windowSeconds, cgroups: [{ cgroup: 'app.slice/app-fixture.scope', cores: overload ? fixture.aggregateCpuLimit : fixture.unleasedHeavy.cores }] })}\n`);
  for (const [index, holder] of fixture.holding.entries()) {
    if (holder.measured === null) continue;
    writeFileSync(join(root, 'leases', `${ids[index]}.usage`), JSON.stringify({
      ...holder.measured, recentCores: overload && holder.class === 'moderate' ? holder.cpus : holder.measured.cores, at: now,
    }));
  }
  const invoke = (args: string[]) => {
    const result = Bun.spawnSync([process.execPath, join(here, 'machine-capacity.mjs'), ...args], { env });
    return JSON.parse(result.stdout.toString());
  };
  const status = invoke(['status']);
  const probe = invoke(['probe', '--class', 'moderate']);
  assert.equal(status.committedBatchCpus, probe.committedBatchCpus);
  assert.deepEqual([status.profile, status.aggregateCpuLimit, fixture.aggregateCpuLimit], [fixture.profile, aggregateCpus(fixture.profile, fixture.cores), aggregateCpus(fixture.profile, fixture.cores)]);
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
  assert.equal(baseline.status.committedBatchCpus, 3.69);
  assert.equal(baseline.probe.reason, 'RUN');
  assert.equal(baseline.denied, false);
  console.log(`Recorded successful baseline: old=${old} new=${baseline.status.committedBatchCpus} limit=${fixture.aggregateCpuLimit}; both admit. Original refusal evidence remains missing.`);
  const reconstruction = fixture.reconstructedRefusal;
  const declared = reconstruction.holding.reduce((sum: number, holder: any) => sum + Math.max(holder.cpus, holder.measuredCores), reconstruction.unleasedHeavyCores);
  const committed = reconstruction.holding.reduce((sum: number, holder: any) => sum + runningCpuCommitment(holder.class, holder.measuredCores, holder.estimateP50, holder.cpus, holder.cpus).cpus, reconstruction.unleasedHeavyCores);
  const fixtureProbe = (charges: number) => {
    const result = Bun.spawnSync([process.execPath, join(here, 'machine-capacity.mjs'), 'fixture',
      '--profile', 'attended', '--class', 'moderate', '--cores', String(reconstruction.cores),
      '--memory-total-mib', '96343', '--memory-available-mib', '70000',
      '--protected-cpu-some-avg10-basis-points', '0', '--memory-full-avg10-basis-points', '0',
      '--leased-cpus', String(charges), '--leased-memory-mib', '20480'], { env });
    assert.ok([0, 75].includes(result.exitCode), result.stderr.toString());
    return JSON.parse(result.stdout.toString());
  };
  const declaredProbe = fixtureProbe(declared);
  const estimatedProbe = fixtureProbe(committed);
  assert.equal(declared, 17.71);
  assert.ok(declared > reconstruction.aggregateCpuLimit);
  assert.equal(declaredProbe.decision, 'DEFER_CPU_CAPACITY');
  assert.equal(Math.round(committed * 100) / 100, 16.21);
  assert.equal(estimatedProbe.aggregateCpuLimit, reconstruction.aggregateCpuLimit);
  assert.equal(estimatedProbe.decision, 'DEFER_CPU_CAPACITY');
  const fixtureGate = (charges: number) => {
    writeFileSync(join(scratch, 'status.json'), JSON.stringify({
      profile: 'attended', holding: [], committedBatchCpus: charges,
      aggregateCpuLimit: reconstruction.aggregateCpuLimit, protectedCpuSomeAvg10: 0,
    }));
    const result = Bun.spawnSync(['python3', '-c', `import sys,json;sys.path.insert(0,sys.argv[1]);import spawn_capacity;print(json.dumps(spawn_capacity.check({'tool_name':'spawn_agent','tool_input':{'message':'reconstructed capacity scenario'}})))`, join(here, '../../../hooks/lib')], {
      env: { ...env, PYTHONDONTWRITEBYTECODE: '1', SPAWN_CAPACITY_STATUS: join(scratch, 'status.json'), SPAWN_CAPACITY_HOME: scratch, AGENTS_ORG_FILE: join(scratch, 'no-org'), AGENTS_ORCHESTRATION: join(scratch, 'no-priority') },
    });
    assert.equal(result.exitCode, 0, result.stderr.toString());
    return JSON.parse(result.stdout.toString()) !== null;
  };
  assert.equal(fixtureGate(declared), true);
  assert.equal(fixtureGate(committed), true);
  console.log(`Reconstructed 11 Oct refusal: declared=${declared}/${reconstruction.aggregateCpuLimit} fixture=${declaredProbe.decision} gate=REFUSE; estimated=${committed}/${reconstruction.aggregateCpuLimit} fixture=${estimatedProbe.decision} gate=REFUSE; heavies stay committed at their reservation (15 Oct load 129).`);
  const estimateOverload = reconstruction.holding.reduce((sum: number, holder: any) => sum + runningCpuCommitment(holder.class, 1, 4, holder.cpus, holder.cpus).cpus, reconstruction.unleasedHeavyCores);
  assert.equal(fixtureProbe(estimateOverload).decision, 'DEFER_CPU_CAPACITY');
  assert.equal(fixtureGate(estimateOverload), true);
  console.log(`Synthetic estimate overload: committed=${estimateOverload}/${reconstruction.aggregateCpuLimit}; fixture=DEFER_CPU_CAPACITY gate=REFUSE.`);
  const overloaded = snapshot(0, true);
  assert.equal(overloaded.probe.reason, 'DEFER_CPU_CAPACITY');
  assert.equal(overloaded.denied, true);
  console.log(`Synthetic measured-use overload: moderate at its quota, unleased=${fixture.aggregateCpuLimit} committed=${overloaded.status.committedBatchCpus}; helper and gate refuse.`);
  const protectedLoad = snapshot(25);
  assert.equal(protectedLoad.probe.reason, 'DEFER_INTERACTIVE_PRESSURE');
  assert.equal(protectedLoad.denied, true);
  console.log('Synthetic protected pressure: some avg10=25; helper and gate refuse.');
  assert.deepEqual(runningCpuCommitment('heavy', null, 6, 6, 6), { cpus: 6, basis: 'scope-quota' });
  console.log('No-history heavy commits its 6-CPU scope quota.');
} finally { rmSync(scratch, { recursive: true }); }
