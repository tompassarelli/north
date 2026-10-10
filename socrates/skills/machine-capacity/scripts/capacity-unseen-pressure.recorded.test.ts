import { strict as assert } from 'node:assert';
import { mkdtempSync, mkdirSync, writeFileSync, rmSync } from 'node:fs';
import { join } from 'node:path';
import { tmpdir } from 'node:os';
import { Database } from 'bun:sqlite';

const here = import.meta.dir;
const fixture = await Bun.file(join(here, 'capacity-unseen-pressure.recorded.json')).json();
const scratch = mkdtempSync(join(tmpdir(), 'capacity-unseen-pressure-'));
const helper = Bun.argv[2] ?? join(here, 'machine-capacity.mjs');
const env = {
  ...process.env, XDG_STATE_HOME: join(scratch, 'state'), AGENT_CAPACITY_DB: join(scratch, 'capacity.db'), AGENT_CAPACITY_USAGE_LOG: join(scratch, 'usage.jsonl'),
  XDG_RUNTIME_DIR: scratch, AGENT_CAPACITY_CGROUP_ROOT: join(scratch, 'cgroup'),
  AGENT_CAPACITY_GPU_BUSY: join(scratch, 'gpu'), AGENT_CAPACITY_GPU_CLIENTS: '0',
};
delete env.AGENT_CAPACITY_CPU_PRESSURE;
function invoke(args: string[]) {
  const result = Bun.spawnSync([process.execPath, helper, ...args], { cwd: here, env });
  assert.ok([0, 75].includes(result.exitCode), result.stderr.toString());
  return JSON.parse(result.stdout.toString());
}
function estimate(command: string[]) { return invoke(['estimate', '--json', '--', ...command]); }
function psi(path: string, value: number) {
  mkdirSync(path, { recursive: true });
  writeFileSync(join(path, 'cpu.pressure'), `some avg10=${value} avg60=0 avg300=0 total=0\n`);
}
try {
  const cheap = estimate(['python3', 'x.py']);
  assert.equal(cheap.class, 'moderate');
  assert.equal(cheap.confidence, 'low');
  assert.equal(cheap.sampleCount, 0);
  const db = new Database(env.AGENT_CAPACITY_DB);
  try {
    const insert = db.query(`INSERT INTO capacity_runs
      (source_id, at, family, repo, package, class, signature_exact, meanCores, p90Cores, peakMemoryMiB)
      VALUES (?, ?, ?, ?, ?, ?, 1, 0.4, 0.5, 128)`);
    for (const item of fixture.unseen) {
      for (let index = 0; index < 3; index += 1) {
        insert.run(`${item.family}:${index}`, `2026-10-11T00:00:0${index}Z`, item.family, cheap.signature.repo, 'already-seen', 'heavy');
      }
      const empty = estimate(item.command);
      assert.equal(empty.signature.family, item.family);
      assert.equal(empty.class, item.class);
      assert.equal(empty.confidence, 'low');
      assert.equal(empty.sampleCount, 0);
      assert.equal(empty.matchLevel, 'default');
      assert.equal(empty.cores.p90, item.class === 'native' ? 2 : 6);
      console.log(`Unseen ${item.command.join(' ')} with unrelated family history: ${empty.class}, ${empty.confidence}, p90=${empty.cores.p90}.`);
    }
    insert.run('known-cargo-target', '2026-10-11T00:00:04Z', 'cargo build', cheap.signature.repo, 'known-target', 'heavy');
    const known = estimate(['cargo', 'build', '-p', 'known-target']);
    assert.equal(known.matchLevel, 'family-repo');
    assert.equal(known.confidence, 'medium');
    assert.equal(known.cores.p90, 0.5);
    insert.run('cheap-family:0', '2026-10-11T00:00:00Z', 'other', cheap.signature.repo, 'seen.py', 'moderate');
    insert.run('cheap-family:1', '2026-10-11T00:00:01Z', 'other', cheap.signature.repo, 'seen.py', 'moderate');
    insert.run('cheap-family:2', '2026-10-11T00:00:02Z', 'other', cheap.signature.repo, 'seen.py', 'moderate');
    assert.equal(estimate(['python3', 'x.py']).matchLevel, 'family-repo');
    console.log('Unseen Python: moderate/low with empty history; known Cargo and cheap family retain nearest matching.');
  } finally { db.close(); }

  const pressure = fixture.pressure;
  const cgroups = env.AGENT_CAPACITY_CGROUP_ROOT;
  const manager = join(cgroups, `user.slice/user-${process.getuid()}.slice/user@${process.getuid()}.service`);
  psi(cgroups, pressure.systemSomeAvg10);
  psi(join(cgroups, 'system.slice'), pressure.nonLeasedSomeAvg10);
  for (const slice of ['app', 'background', 'session', 'native']) psi(join(manager, `${slice}.slice`), pressure.nonLeasedSomeAvg10);
  psi(join(manager, 'agent.slice'), pressure.leasedScopeSomeAvg10);
  psi(join(manager, 'agent.slice/agent-capacity.slice'), pressure.leasedScopeSomeAvg10);
  psi(join(manager, 'agent.slice/agent-capacity.slice/agent-capacity-recorded.scope'), pressure.leasedScopeSomeAvg10);
  const unleased = join(manager, 'agent.slice/agent-bounded.slice');
  psi(unleased, pressure.nonLeasedSomeAvg10);
  writeFileSync(join(scratch, 'gpu'), '0\n');
  for (const kind of ['moderate', 'heavy']) {
    const baseline = invoke(['probe', '--class', kind]);
    assert.equal(baseline.reason, 'RUN');
    assert.equal(baseline.cpuSomeAvg10, pressure.nonLeasedSomeAvg10);
    assert.equal(baseline.queuedAhead, 0);
  }
  console.log(`Recorded quota stall: leased=${pressure.leasedScopeSomeAvg10}, system=${pressure.systemSomeAvg10}, non-leased=${pressure.nonLeasedSomeAvg10}; moderate/heavy RUN, queued=0.`);
  psi(unleased, pressure.unleasedOverloadSomeAvg10);
  assert.equal(invoke(['probe', '--class', 'heavy']).reason, 'DEFER_CPU_PRESSURE');
  console.log(`Injected unleased agent overload: ${pressure.unleasedOverloadSomeAvg10}; heavy DEFER_CPU_PRESSURE.`);
  psi(unleased, 30);
  assert.equal(invoke(['probe', '--class', 'heavy']).reason, 'RUN');
  psi(unleased, 30.01);
  assert.equal(invoke(['probe', '--class', 'heavy']).reason, 'DEFER_CPU_PRESSURE');
  psi(unleased, pressure.nonLeasedSomeAvg10);
  psi(join(manager, 'session.slice'), pressure.protectedOverloadSomeAvg10);
  assert.equal(invoke(['probe', '--class', 'heavy']).reason, 'DEFER_INTERACTIVE_PRESSURE');
  console.log('Queue boundary remains 30%; protected pressure 25% still defers.');
} finally { rmSync(scratch, { recursive: true }); }
