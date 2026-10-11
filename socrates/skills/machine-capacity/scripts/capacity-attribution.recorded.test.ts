import { strict as assert } from 'node:assert';
import { mkdtempSync, mkdirSync, writeFileSync, rmSync } from 'node:fs';
import { join } from 'node:path';
import { tmpdir } from 'node:os';

const here = import.meta.dir;
const fixture = await Bun.file(join(here, 'capacity-attribution.recorded.json')).json();
const helper = Bun.argv[2] ?? join(here, 'machine-capacity.mjs');
const uid = process.getuid!();
const manager = `/user.slice/user-${uid}.slice/user@${uid}.service`;

function scenario(newer: any[][]) {
  const scratch = mkdtempSync(join(tmpdir(), 'capacity-attribution-'));
  const proc = join(scratch, 'proc');
  const cgroups = join(scratch, 'cgroup');
  const env: Record<string, string | undefined> = {
    ...process.env, XDG_RUNTIME_DIR: scratch, XDG_STATE_HOME: join(scratch, 'state'), AGENT_CAPACITY_CGROUP_ROOT: cgroups,
    AGENT_CAPACITY_PROC_ROOT: proc, AGENTS_ORG_FILE: join(scratch, 'org.json'), AGENT_CAPACITY_CORES: '20',
    AGENT_CAPACITY_DB: join(scratch, 'capacity.db'), AGENT_CAPACITY_USAGE_LOG: join(scratch, 'usage.jsonl'),
  };
  delete env.AGENT_CAPACITY_CPU_PRESSURE;
  writeFileSync(env.AGENTS_ORG_FILE!, JSON.stringify({ version: 1, nodes: fixture.org }));
  for (const name of ['scope', 'lease']) mkdirSync(join(cgroups, manager, fixture[name]), { recursive: true });
  const sample = (rows: any[][], usageSeconds: number, now: number) => {
    rmSync(proc, { recursive: true, force: true });
    for (const [pid, ppid, comm, start, ticks, childTicks, cgroup, args] of rows) {
      const fields = ['S', ppid, 0, 0, 0, 0, 0, 0, 0, 0, 0, ticks, 0, childTicks, 0, 0, 0, 0, 0, start];
      mkdirSync(join(proc, String(pid)), { recursive: true });
      writeFileSync(join(proc, String(pid), 'stat'), `${pid} (${comm}) ${fields.join(' ')}\n`);
      writeFileSync(join(proc, String(pid), 'cgroup'), `0::${manager}/${fixture[cgroup]}\n`);
      writeFileSync(join(proc, String(pid), 'cmdline'), `${args.join('\0')}\0`);
    }
    writeFileSync(join(cgroups, manager, fixture.scope, 'cpu.stat'), `usage_usec ${usageSeconds * 1e6}\nuser_usec 0\nsystem_usec 0\n`);
    const result = Bun.spawnSync([process.execPath, helper, 'sample-unleased'], { env: { ...env, AGENT_CAPACITY_NOW: String(now) } });
    assert.equal(result.exitCode, 0, result.stderr.toString());
    return JSON.parse(result.stdout.toString());
  };
  try {
    sample(fixture.older, fixture.scopeUsageSeconds[0], 1_000_000);
    return sample(newer, fixture.scopeUsageSeconds[1], 1_000_000 + fixture.windowMilliseconds);
  } finally {
    rmSync(scratch, { recursive: true, force: true });
  }
}

function check(result: any) {
  const scope = result.heavy.find((entry: any) => entry.cgroup === fixture.scope);
  assert.equal(result.heavy.length, 1, 'the lease scope is never reported as unleased');
  assert.deepEqual([scope.top[0].org, scope.top[0].session], ['telemetry-lead', fixture.org[1].session]);
  assert.equal(result.sessions[0].org, 'telemetry-lead');
  const attributed = scope.top.reduce((sum: number, owner: any) => sum + owner.cores, 0);
  const sessions = result.sessions.reduce((sum: number, owner: any) => sum + owner.cores, 0);
  for (const total of [attributed, sessions]) assert.ok(Math.abs(total - scope.cores) <= 0.05, `attributed ${total} cores disagree with the scope's ${scope.cores}`);
}

check(scenario(fixture.newer));
console.log('Recorded shared scope: the busy cargo tree is credited to its own session, leased work to none.');
const unmarkedLeaseHost = fixture.newer.map((row: any[]) => row[0] === 300 ? [...row.slice(0, 7), ['bun', 'host.mjs']] : row);
assert.throws(() => check(scenario(unmarkedLeaseHost)), /disagree/);
console.log('Injected fault: leased CPU reaped into an unrecognised host breaks attribution agreement.');
