import {
  appendFileSync,
  closeSync,
  existsSync,
  fstatSync,
  mkdirSync,
  openSync,
  readFileSync,
  readdirSync,
  readSync,
  realpathSync,
  rmSync,
  statSync,
  unlinkSync,
  watch,
  writeFileSync,
} from 'node:fs';
import { cpus } from 'node:os';
import { join } from 'node:path';
import { pathToFileURL } from 'node:url';
import { Database, constants } from 'bun:sqlite';

import {
  DEFAULTS,
  resourceClass,
  nativeMemoryRequest,
  reserveClass,
  aggregateCpus,
  batchClass,
  clampToGrantable,
  grantableMaximum,
  maximumSeconds,
  sessionSeconds,
  sizedClass,
  runningCpuCommitment, scopeCpuQuota,
  chargedResources,
  taskSignature,
  estimateCapacity,
  ownerPrefix,
  diskBusyShare,
  gpuBusyObservation,
  gpuBusyLimitPercent,
  topUnleasedGpuProcess,
  sessionFromArgs,
  sessionProviders,
} from './machine-capacity-logic.ts';
const subslice = process.env.AGENT_CAPACITY_SUBSLICE ? `-${process.env.AGENT_CAPACITY_SUBSLICE}` : '';
const aggregateSlice = `agent-capacity${subslice}.slice`;
// Sibling of session.slice (300), app.slice (100) and agent.slice (20): game
// clients outrank terminals and batch work but never the compositor.
const nativeSlice = `native${subslice}.slice`;
// systemd nests a-b.slice inside a.slice.
const slicePath = slice => slice.slice(0, -6).split('-').map((_, index, parts) => `${parts.slice(0, index + 1).join('-')}.slice`).join('/');
const nativeCpuWeight = 200;
const gpuSampleMilliseconds = 500;
const nativeGpuSamples = 10;
const classNames = new Set(['agent', 'native', 'moderate', 'heavy', 'exclusive', 'gpu', 'critical', 'tom']);
const frozenClasses = new Set(['heavy', 'exclusive', 'gpu']);
const nativeWaitingMilliseconds = 60000;
const modes = new Map([['present', 'attended'], ['away', 'unattended'], ['auto', null]]);
const lockPatienceMilliseconds = 60000;
const runLeaseGraceMilliseconds = 5000;
const queuePollMilliseconds = 1000;
const terminationSignals = ['SIGINT', 'SIGTERM', 'SIGHUP'];
const northCommand = process.env.AGENT_CAPACITY_NORTH ?? join(process.env.HOME ?? '', '.local/state/north/current/north');

function fail(message, status = 2) {
  writeFileSync(2, `machine-capacity: ${message}\n`);
  process.exit(status);
}

function parsePositiveInteger(text, option, maximum = Number.MAX_SAFE_INTEGER) {
  const value = Number(text);
  if (!Number.isSafeInteger(value) || value < 1 || value > maximum) {
    fail(`${option} must be an integer from 1 to ${maximum}`);
  }
  return value;
}

function parseNonnegativeInteger(text, option) {
  const value = Number(text);
  if (!Number.isSafeInteger(value) || value < 0) {
    fail(`${option} must be a nonnegative integer`);
  }
  return value;
}

function parseKeyValues(argv, start, allowed, flags = new Set()) {
  const values = new Map();
  let separator = argv.length;
  for (let index = start; index < argv.length; index += flags.has(argv[index]) ? 1 : 2) {
    if (argv[index] === '--') {
      separator = index;
      break;
    }
    const option = argv[index];
    const value = flags.has(option) ? 'true' : argv[index + 1];
    if (!option?.startsWith('--') || value === undefined) {
      fail(`invalid argument at position ${index + 1}`);
    }
    if (!allowed.has(option)) fail(`unknown option: ${option}`);
    if (values.has(option)) fail(`duplicate option: ${option}`);
    values.set(option, value);
  }
  return { values, separator };
}

function required(values, option) {
  const value = values.get(option);
  if (!value) fail(`missing ${option}`);
  return value;
}

function parseClass(values, cores) {
  const name = required(values, '--class');
  if (!classNames.has(name)) fail('--class must be agent, native, moderate, heavy, exclusive, gpu, critical, or tom');
  let resources = resourceClass(name, cores);
  if (values.has('--memory-gib')) {
    const memoryGiB = Number(required(values, '--memory-gib'));
    if (!Number.isFinite(memoryGiB)) fail('--memory-gib must be finite');
    try {
      resources = nativeMemoryRequest(name, cores, memoryGiB);
    } catch (error) {
      fail(error.message);
    }
    if (!Number.isSafeInteger(resources.memoryMiB)) {
      fail('--memory-gib must represent a whole, safe number of MiB');
    }
  }
  if (!Number.isFinite(resources.cpus) || !Number.isFinite(resources.memoryMiB)) {
    fail(`policy rejected resource class: ${name}`);
  }
  const count = (option, fallback) => !values.has(option) ? fallback : name === 'native' ? parseNonnegativeInteger(values.get(option), option) : fail(`${option} declares a native request's ${option.slice(6)}`), newClients = count('--new-clients', Number(name === 'native')), newDesktops = count('--new-desktops', 0);
  return { name, ...resources, newClients, newDesktops, ...(values.has('--wait-seconds') ? { waitSeconds: newClients + newDesktops > 0 ? parsePositiveInteger(values.get('--wait-seconds'), '--wait-seconds') : fail('--wait-seconds queues a native client or desktop start (--new-clients or --new-desktops above 0)') } : {}) };
}

function parsePsi(path, kind) {
  const line = readFileSync(path, 'utf8').split('\n').find(value => value.startsWith(`${kind} `));
  const match = line?.match(/avg10=([0-9]+(?:\.[0-9]+)?)/);
  if (!match) fail(`cannot read ${kind} avg10 from ${path}`);
  return Math.round(Number(match[1]) * 100);
}

// Tests point this at fixture slices to check which slices count as pressure.
const cgroupRoot = process.env.AGENT_CAPACITY_CGROUP_ROOT ?? '/sys/fs/cgroup';
const hostCoreCount = Number(process.env.AGENT_CAPACITY_CORES) || cpus().length;

function userManagerCgroup() {
  return `${cgroupRoot}/user.slice/user-${process.getuid()}.slice/user@${process.getuid()}.service`;
}

function slicePressure(slice) {
  const path = process.env.AGENT_CAPACITY_CPU_PRESSURE ?? join(userManagerCgroup(), slice, 'cpu.pressure');
  return existsSync(path) ? parsePsi(path, 'some') : 0;
}

function unleasedAgentPressure() {
  const path = join(userManagerCgroup(), 'agent.slice');
  if (!existsSync(path)) return 0;
  return Math.max(0, ...readdirSync(path, { withFileTypes: true })
    .filter(entry => entry.isDirectory() && entry.name !== slicePath(aggregateSlice).split('/')[1] && !/^agent-capacity-.*\.scope$/.test(entry.name))
    .map(entry => slicePressure(join('agent.slice', entry.name))));
}

function readSignals() {
  const fields = new Map();
  for (const line of readFileSync(process.env.AGENT_CAPACITY_MEMINFO ?? '/proc/meminfo', 'utf8').split('\n')) {
    const match = line.match(/^(MemTotal|MemAvailable):\s+([0-9]+) kB$/);
    if (match) fields.set(match[1], Math.floor(Number(match[2]) / 1024));
  }
  if (fields.size < 2) {
    fail('cannot read MemTotal and MemAvailable from /proc/meminfo');
  }
  return {
    cores: hostCoreCount,
    memoryTotalMiB: fields.get('MemTotal'),
    memoryAvailableMiB: fields.get('MemAvailable'),
    cpuSomeAvg10BasisPoints: process.env.AGENT_CAPACITY_CPU_PRESSURE
      ? parsePsi(process.env.AGENT_CAPACITY_CPU_PRESSURE, 'some')
      : Math.max(
        parsePsi(join(cgroupRoot, 'system.slice', 'cpu.pressure'), 'some'),
        slicePressure('app.slice'), slicePressure('background.slice'),
        slicePressure('session.slice'), slicePressure(slicePath(nativeSlice)), unleasedAgentPressure()),
    protectedCpuSomeAvg10BasisPoints: Math.max(slicePressure('session.slice'), slicePressure(slicePath(nativeSlice))),
    memoryFullAvg10BasisPoints: parsePsi('/proc/pressure/memory', 'full'),
  };
}

function gpuBusyPath() {
  if (process.env.AGENT_CAPACITY_GPU_BUSY) return process.env.AGENT_CAPACITY_GPU_BUSY;
  const card = readdirSync('/sys/class/drm').find(name => /^card[0-9]+$/.test(name)
    && existsSync(`/sys/class/drm/${name}/device/gpu_busy_percent`));
  return card === undefined ? null : `/sys/class/drm/${card}/device/gpu_busy_percent`;
}

function northWarcraftClients() {
  const counted = Bun.spawnSync([northCommand, 'capacity', 'warcraft-clients'], { stdout: 'pipe' });
  if (counted.exitCode !== 0) fail('north capacity warcraft-clients failed', 75);
  return counted.stdout.toString();
}

function readGpu(samples) {
  const path = gpuBusyPath();
  let total = 0;
  for (let index = 0; path !== null && index < samples; index += 1) {
    if (index > 0) Bun.sleepSync(gpuSampleMilliseconds);
    total += Number(readFileSync(path, 'utf8').trim()) || 0;
  }
  const clients = Number(process.env.AGENT_CAPACITY_GPU_CLIENTS ?? northWarcraftClients());
  return { gpuBusyPercent: path === null ? 0 : Math.round(total / samples), gpuClients: clients };
}

function runtimeRoot() {
  const base = process.env.XDG_RUNTIME_DIR ?? `/run/user/${process.getuid()}`;
  if (!base.startsWith('/')) fail('XDG_RUNTIME_DIR must be absolute');
  return join(base, 'agent-capacity-v1');
}

function readMode(root) {
  const mode = readText(join(root, 'mode'))?.trim();
  return modes.has(mode) ? mode : 'auto';
}

function activeProfile(root) {
  const mode = readMode(root);
  return { mode, profile: modes.get(mode) ?? 'attended' };
}

// The manager's private socket reports peer pid 0 inside a PID namespace and fails with ENODATA; without XDG_RUNTIME_DIR the client uses the user bus.
const systemdClientEnv = () => ({ ...Object.fromEntries(Object.entries(process.env).filter(([name]) => name !== 'XDG_RUNTIME_DIR' || !process.env.DBUS_SESSION_BUS_ADDRESS)), ...(process.env.DBUS_SESSION_BUS_ADDRESS && { DBUS_SESSION_BUS_ADDRESS: `unix:path=/run/user/${process.getuid()}/bus` }) });
const systemctl = (stderr, ...args) => Bun.spawnSync(['systemctl', '--user', ...args], { stdin: 'ignore', stdout: 'pipe', stderr, env: systemdClientEnv() });
function setSlice(slice, ...properties) {
  return systemctl('inherit', 'set-property', '--runtime', slice, ...properties).exitCode === 0;
}

function applyProfile(profile, cores) {
  return setSlice(aggregateSlice, `CPUQuota=${aggregateCpus(profile, cores) * 100}%`)
    && setSlice(nativeSlice, `CPUWeight=${nativeCpuWeight}`, `IOWeight=${nativeCpuWeight}`);
}

function ensureProfile(root, force = false) {
  const { profile } = activeProfile(root);
  const marker = join(root, 'applied-profile');
  if (!force && readText(marker) === profile) return true;
  ensureState(root);
  if (!applyProfile(profile, readSignals().cores)) return false;
  writeFileSync(marker, profile, { mode: 0o600 });
  return true;
}

function ensureState(root) {
  mkdirSync(join(root, 'leases'), { recursive: true, mode: 0o700 });
  mkdirSync(join(root, 'queue'), { recursive: true, mode: 0o700 });
}

function withLock(root, action) {
  ensureState(root);
  const lock = new Database(join(root, 'admission.lock'));
  try {
    lock.exec(`PRAGMA busy_timeout = ${lockPatienceMilliseconds}`);
    lock.exec('BEGIN EXCLUSIVE');
  } catch (error) {
    lock.close();
    if (error?.code !== 'SQLITE_BUSY') throw error;
    fail('shared admission lock remained busy', 75);
  }
  try {
    return action();
  } finally {
    lock.close();
  }
}

function processStart(pid) {
  const stat = readText(`/proc/${pid}/stat`);
  if (stat === null) return null;
  const fields = stat.slice(stat.lastIndexOf(')') + 2).split(' ');
  return fields[0] === 'Z' ? null : fields[19];
}

// /proc/self is the pid the host procfs resolves; process.pid is namespace-local inside run-bounded.
function wrapperIdentity() {
  const pid = Number(realpathSync('/proc/self').split('/').at(-1));
  return { wrapperPid: pid, wrapperStart: processStart(pid) };
}

function scopeUnit(id) {
  return `agent-capacity-${id.replaceAll('-', '')}.scope`;
}

function scopeLive(id) {
  const state = systemctl('ignore', 'show', scopeUnit(id), '--property=ActiveState', '--value').stdout.toString().trim();
  // Unknown manager state must not free a possibly live resource allowance.
  return !['inactive', 'failed'].includes(state);
}

const killScope = id => systemctl('ignore', 'kill', '--signal=SIGKILL', scopeUnit(id)).exitCode === 0;

function stateRoot() {
  return process.env.XDG_STATE_HOME ?? join(process.env.HOME ?? '/tmp', '.local', 'state');
}

function usageLogPath() {
  return process.env.AGENT_CAPACITY_USAGE_LOG ?? join(stateRoot(), 'agents', 'machine-capacity-usage.jsonl');
}

function readText(path) {
  try {
    return readFileSync(path, 'utf8');
  } catch (error) {
    if (['ENOENT', 'ESRCH', 'ENODEV', 'EACCES', 'ENOTDIR'].includes(error?.code)) return null;
    throw error;
  }
}

function capacityDatabase() {
  const path = process.env.AGENT_CAPACITY_DB ?? join(stateRoot(), 'agents', 'machine-capacity', 'capacity.db');
  mkdirSync(join(path, '..'), { recursive: true, mode: 0o700 });
  const db = new Database(path, constants.SQLITE_OPEN_READWRITE | constants.SQLITE_OPEN_CREATE | constants.SQLITE_OPEN_URI);
  db.exec('PRAGMA busy_timeout = 5000');
  let attached = false;
  try {
    db.transaction(() => {
      const firstOpen = !db.query("SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'capacity_runs'").get();
      const legacyPath = join(stateRoot(), 'threads', 'threads.db');
      if (firstOpen && existsSync(legacyPath)) {
        const legacyUrl = pathToFileURL(legacyPath);
        legacyUrl.searchParams.set('mode', 'ro');
        db.query('ATTACH DATABASE ? AS legacy_capacity').run(legacyUrl.href);
        attached = true;
      }
      db.exec(`CREATE TABLE IF NOT EXISTS capacity_runs (
    source_id TEXT PRIMARY KEY, at TEXT NOT NULL, family TEXT NOT NULL, repo TEXT,
    package TEXT, class TEXT NOT NULL, signature_exact INTEGER NOT NULL, cwd TEXT,
    owner TEXT, shape TEXT, reservedCpus REAL, reservedMemoryMiB REAL, chargedCpus REAL,
    chargedMemoryMiB REAL, cpuSeconds REAL, meanCores REAL, p90Cores REAL, peakCores REAL,
    peakMemoryMiB REAL, gpuSeconds REAL, wallSeconds REAL, exitCode INTEGER,
    ioReadBytes REAL, ioWriteBytes REAL, ioPressureSomeUsec REAL, diskBusyShare REAL);
    CREATE INDEX IF NOT EXISTS capacity_runs_signature ON capacity_runs(family, repo, package, class, at);
    CREATE TABLE IF NOT EXISTS capacity_imports (source TEXT PRIMARY KEY, rows INTEGER NOT NULL)`);
      if (attached) {
        for (const table of ['capacity_runs', 'capacity_imports']) {
          if (db.query("SELECT 1 FROM legacy_capacity.sqlite_master WHERE type = 'table' AND name = ?").get(table)) {
            db.exec(`INSERT OR IGNORE INTO ${table} SELECT * FROM legacy_capacity.${table}`);
          }
        }
      }
      if (!db.query('PRAGMA table_info(capacity_runs)').all().some(column => column.name === 'command')) {
        db.exec('ALTER TABLE capacity_runs ADD COLUMN command TEXT');
      }
    }).immediate();
  } catch (error) {
    db.close();
    throw error;
  }
  if (attached) db.exec('DETACH DATABASE legacy_capacity');
  const source = usageLogPath();
  if (!db.query('SELECT rows FROM capacity_imports WHERE source = ?').get(source)) {
    const text = readText(source) ?? '';
    db.transaction(() => {
      const lines = text.split('\n').filter(Boolean);
      for (const [index, line] of lines.entries()) {
        const entry = JSON.parse(line);
        const signature = taskSignature(entry.shape.split(/\s+/), null, entry.class, false);
        insertCapacityRun(db, `jsonl:${source}:${index}`, entry, signature, null);
      }
      db.query('INSERT INTO capacity_imports VALUES (?, ?)').run(source, lines.length);
    })();
  }
  return db;
}

const measurementColumns = ['owner', 'shape', 'reservedCpus', 'reservedMemoryMiB', 'chargedCpus',
  'chargedMemoryMiB', 'cpuSeconds', 'meanCores', 'p90Cores', 'peakCores', 'peakMemoryMiB',
  'gpuSeconds', 'wallSeconds', 'exitCode', 'ioReadBytes', 'ioWriteBytes', 'ioPressureSomeUsec', 'diskBusyShare', 'command'];

function insertCapacityRun(db, source, entry, signature, cwd) {
  const columns = ['source_id', 'at', 'family', 'repo', 'package', 'class', 'signature_exact', 'cwd', ...measurementColumns];
  db.query(`INSERT OR IGNORE INTO capacity_runs (${columns.join(',')}) VALUES (${columns.map(() => '?').join(',')})`)
    .run(source, entry.at, signature.family, signature.repo, signature.package, signature.class,
      Number(signature.exact), cwd,
      ...measurementColumns.map(key => Array.isArray(entry[key]) ? JSON.stringify(entry[key]) : entry[key] ?? null));
}

function capacityHistory(db) {
  return db.query('SELECT * FROM capacity_runs ORDER BY at, source_id').all().map(row => ({
    ...row, signature: { family: row.family, repo: row.repo, package: row.package, class: row.class, exact: Boolean(row.signature_exact) },
  }));
}

function physicalDiskBusyMilliseconds() {
  return Object.fromEntries((readText('/proc/diskstats') ?? '').trim().split('\n').flatMap(line => {
    const fields = line.trim().split(/\s+/);
    const disk = fields[2];
    const ticks = Number(fields[12]);
    return disk && existsSync(`/sys/block/${disk}/device`) && Number.isFinite(ticks) ? [[disk, ticks]] : [];
  }));
}

function cgroupIo(path) {
  const stat = readText(join(path, 'io.stat'));
  const pressure = readText(join(path, 'io.pressure'))?.match(/^some .*total=([0-9]+)/m);
  const total = key => stat === null ? null : [...stat.matchAll(new RegExp(`\\b${key}=([0-9]+)`, 'g'))]
    .reduce((sum, match) => sum + Number(match[1]), 0);
  return { ioReadBytes: total('rbytes'), ioWriteBytes: total('wbytes'), ioPressureSomeUsec: pressure ? Number(pressure[1]) : null };
}

function cgroupUsageUsec(path) {
  const match = readText(join(path, 'cpu.stat'))?.match(/^usage_usec ([0-9]+)$/m);
  return match ? Number(match[1]) : null;
}

const shellCommands = new Set(['bash', 'sh', 'zsh', 'dash']);
const shellBuiltins = new Set(['cd', 'wait', 'echo', 'printf', 'export', 'set', 'true', 'exit', 'source', '.']);
const wrapperCommands = new Set(['exec', 'env', 'nice', 'ionice', 'timeout', 'time', 'nohup', 'stdbuf']);

// A stable key for "the same kind of work": the program and its subcommand words.
const secretFlag = /^--?[\w-]*(?:token|secret|password|passwd|api-?key|auth|credential)[\w-]*$/i;

function redactWord(word) {
  return word
    .replace(/(:\/\/)[^/\s@]+@/g, '$1<redacted>@')
    .replace(/\b([A-Za-z_][A-Za-z0-9_-]*)=("[^"]*"|'[^']*'|\S+)/g, '$1=<redacted>')
    .replace(/(--?[\w-]*(?:token|secret|password|passwd|api-?key|auth|credential)[\w-]*)\s+("[^"]*"|'[^']*'|\S+)/gi, '$1 <redacted>')
    .replace(/[A-Za-z0-9+/_-]{32,}={0,2}/g, '<redacted>');
}

function recordedCommand(command) {
  return command.map((word, index) => index > 0 && secretFlag.test(command[index - 1]) ? '<redacted>' : redactWord(word).slice(0, 2000));
}

function commandShape(command) {
  let words = [...command];
  let unwrappedShell = false;
  for (let guard = 0; guard < 8 && words.length > 0; guard += 1) {
    const program = words[0].split('/').at(-1);
    if (!unwrappedShell && shellCommands.has(program) && words[1] === '-c' && words[2] !== undefined) {
      unwrappedShell = true;
      words = words[2].split(/[;&|\n]+/).map(part => part.trim().split(/\s+/))
        .find(part => part[0] !== '' && !shellBuiltins.has(part[0])) ?? [];
    } else if (program === 'nix' && ['develop', 'shell'].includes(words[1])) {
      const marker = words.findIndex(word => word === '-c' || word === '--command');
      if (marker < 0) break;
      words = words.slice(marker + 1);
    } else if (wrapperCommands.has(program) || /^[A-Za-z_][A-Za-z0-9_]*=/.test(words[0])) {
      words = words.slice(1);
      while (words.length > 0 && (words[0].startsWith('-') || /^[A-Za-z_][A-Za-z0-9_]*=/.test(words[0]) || /^[0-9.]+[smhd]?$/.test(words[0]))) {
        words = words.slice(1);
      }
    } else break;
  }
  if (words.length === 0) return 'unknown';
  const shape = [words[0].split('/').at(-1)];
  for (const word of words.slice(1)) {
    if (shape.length >= 3 || !/^[a-z][a-z0-9:_-]*$/.test(word)) break;
    shape.push(word);
  }
  return shape.join(' ');
}

// Per-process GPU engine time from amdgpu fdinfo, summed once per DRM client.
function gpuEngineNanoseconds(pids, perClient, observe = null) {
  for (const pid of pids) {
    let fds;
    try {
      fds = readdirSync(`/proc/${pid}/fdinfo`);
    } catch {
      continue;
    }
    for (const fd of fds) {
      const text = readText(`/proc/${pid}/fdinfo/${fd}`);
      if (text === null || !text.includes('drm-driver:\tamdgpu')) continue;
      const client = text.match(/^drm-client-id:\s+([0-9]+)$/m)?.[1];
      if (client === undefined) continue;
      let total = 0;
      for (const match of text.matchAll(/^drm-engine-[a-z_]+:\s+([0-9]+) ns$/gm)) total += Number(match[1]);
      perClient.set(client, Math.max(perClient.get(client) ?? 0, total));
      if (observe !== null) observe(pid, text.match(/^drm-pdev:\s+(\S+)$/m)?.[1], client, total);
    }
  }
}

function takeGpuProcessSample() {
  const path = gpuBusyPath();
  const device = path === null || process.env.AGENT_CAPACITY_GPU_BUSY ? null : realpathSync(join(path, '..')).split('/').at(-1);
  const clients = [];
  const metadata = new Map();
  gpuEngineNanoseconds(readdirSync('/proc').filter(pid => /^[0-9]+$/.test(pid)), new Map(), (pid, pdev, client, nanoseconds) => {
    if (device !== null && pdev !== device) return;
    if (!metadata.has(pid)) {
      const start = processStart(pid);
      const words = (readText(`/proc/${pid}/cmdline`) ?? '').split('\0').filter(Boolean);
      if (words.length > 0) words[0] = words[0].split('/').at(-1);
      const command = recordedCommand(words).join(' ').slice(0, 120)
        || (readText(`/proc/${pid}/comm`) ?? '').trim();
      const cgroup = (readText(`/proc/${pid}/cgroup`) ?? '').match(/^0::(.*)$/m)?.[1];
      metadata.set(pid, start === null || cgroup === undefined ? null : { pid: Number(pid), start, command, cgroup });
    }
    const process = metadata.get(pid);
    if (process !== null) clients.push({ ...process, client: `${pdev ?? ''}:${client}`, nanoseconds });
  });
  return { at: Date.now(), clients };
}

function recordGpuBusy(root, busyPercent, firstSampleAt = Date.now()) {
  const path = join(root, 'gpu-busy-since');
  const previous = Number(readText(path));
  const observation = gpuBusyObservation(firstSampleAt, busyPercent, previous > 0 ? previous : null);
  if (observation.gpuBusySince === null) rmSync(path, { force: true });
  else writeFileSync(path, String(observation.gpuBusySince), { mode: 0o600 });
  return gpuBusyObservation(Date.now(), busyPercent, observation.gpuBusySince);
}

const scopeCgroup = (className, unit) => join(userManagerCgroup(), slicePath(leaseSlice(className)), unit);

const usageSampleMilliseconds = 1000;
const peakWindowMilliseconds = 5000;
const liveWindowMilliseconds = 120000;
const gpuEverySamples = 5;

// Samples the lease scope while it runs; the scope's cgroup disappears with its last process.
function measureScope(root, lease, cgroup) {
  const startedAt = Date.now();
  const disksBefore = physicalDiskBusyMilliseconds();
  const samples = [];
  const perClient = new Map();
  const recent = [];
  const liveSamples = [];
  const usage = { cpuSeconds: 0, meanCores: 0, peakCores: 0, recentCores: 0, peakMemoryMiB: 0, memoryMiB: 0, gpuSeconds: 0, ioReadBytes: null, ioWriteBytes: null, ioPressureSomeUsec: null };
  let count = 0;
  const sample = () => {
    const now = Date.now();
    const io = cgroupIo(cgroup);
    for (const [key, value] of Object.entries(io)) if (value !== null) usage[key] = value;
    const usec = cgroupUsageUsec(cgroup);
    if (usec === null) return;
    samples.push([now, usec]);
    liveSamples.push([now, usec]);
    while (liveSamples.length > 2 && now - liveSamples[1][0] >= liveWindowMilliseconds) liveSamples.shift();
    const [liveThen, liveBefore] = liveSamples[0];
    usage.liveCores = now > liveThen ? (usec - liveBefore) / 1000 / (now - liveThen) : null;
    usage.liveWindowSeconds = (now - liveThen) / 1000;
    while (samples.length > 2 && now - samples[1][0] >= peakWindowMilliseconds) samples.shift();
    const [then, before] = samples[0];
    if (now - then >= peakWindowMilliseconds / 2) {
      usage.recentCores = (usec - before) / 1000 / (now - then);
      usage.peakCores = Math.max(usage.peakCores, usage.recentCores);
      recent.push(usage.recentCores);
    }
    usage.cpuSeconds = usec / 1e6;
    usage.meanCores = usage.cpuSeconds / Math.max((now - startedAt) / 1000, 1);
    const peak = Number(readText(join(cgroup, 'memory.peak'))?.trim());
    const current = Number(readText(join(cgroup, 'memory.current'))?.trim());
    if (Number.isFinite(peak)) usage.peakMemoryMiB = Math.max(usage.peakMemoryMiB, peak / 1048576);
    if (Number.isFinite(current)) usage.memoryMiB = current / 1048576;
    if (count % gpuEverySamples === 0) {
      const pids = (readText(join(cgroup, 'cgroup.procs')) ?? '').split('\n').filter(Boolean);
      gpuEngineNanoseconds(pids, perClient);
      usage.gpuSeconds = [...perClient.values()].reduce((sum, value) => sum + value, 0) / 1e9;
    }
    count += 1;
    try {
      writeFileSync(join(root, 'leases', `${lease}.usage`), `${JSON.stringify({ ...usage, at: now })}\n`, { mode: 0o600 });
    } catch (error) {
      if (error?.code !== 'ENOENT') throw error;
    }
  };
  const timer = setInterval(sample, usageSampleMilliseconds);
  return {
    sample,
    finish() {
      clearInterval(timer);
      sample();
      return { ...usage, diskBusyShare: diskBusyShare(disksBefore, physicalDiskBusyMilliseconds(), Date.now() - startedAt), p90Cores: recent.length > 0 ? percentile(recent, 0.9) : null, wallSeconds: (Date.now() - startedAt) / 1000 };
    },
  };
}

const known = value => value ?? 'unknown';
const round = (value, digits = 2) => Math.round(value * 10 ** digits) / 10 ** digits;

function recordUsage(entry) {
  const path = usageLogPath();
  mkdirSync(join(path, '..'), { recursive: true, mode: 0o700 });
  appendFileSync(path, `${JSON.stringify(entry)}\n`, { mode: 0o600 });
}

const sizingRuns = DEFAULTS["capacity_sizing_runs"];

function percentile(values, share) {
  const sorted = [...values].sort((left, right) => left - right);
  return sorted[Math.min(sorted.length - 1, Math.ceil(share * sorted.length) - 1)];
}

function recentUsage(lines, matches) {
  return (readText(usageLogPath()) ?? '').split('\n').slice(-lines).flatMap(line => {
    try {
      const entry = JSON.parse(line);
      return matches(entry) ? [entry] : [];
    } catch { return []; }
  }).slice(-sizingRuns);
}

function estimateCommand(command, kind, owner, cores) {
  const db = capacityDatabase();
  try {
    return estimateCapacity(taskSignature(command, process.cwd(), kind), capacityHistory(db), cores,
      { owner, shape: commandShape(command) });
  } finally { db.close(); }
}

function sizeFromUsage(command, owner, cores) {
  const estimate = estimateCommand(command, 'moderate', owner, cores);
  const shape = commandShape(command);
  const gpuRuns = recentUsage(2000, entry => entry.shape === shape && entry.wallSeconds > 0);
  const gpuShare = gpuRuns.length >= 3 ? percentile(gpuRuns.map(run => (run.gpuSeconds ?? 0) / run.wallSeconds), 0.5) : 0;
  const resources = {
    cpus: Math.max(1, Math.ceil(estimate.cores.p90)),
    memoryMiB: Math.max(1, Math.ceil(estimate.peakMemoryMiB.p90 * DEFAULTS["capacity_memory_margin"])),
  };
  return {
    name: estimate.class === 'native' ? 'native' : sizedClass(gpuShare, resources.cpus, estimate.peakMemoryMiB.p90), ...resources, estimate, newClients: Number(estimate.class === 'native'), newDesktops: 0,
    sizing: { shape: commandShape(command), runs: estimate.sampleCount, basis: estimate.matchLevel,
      confidence: estimate.confidence, gpuShare: round(gpuShare), p90Cores: estimate.cores.p90, p90MemoryMiB: estimate.peakMemoryMiB.p90,
      p75Seconds: estimate.duration.p75Seconds, durationRuns: estimate.duration.sampleCount, durationBasis: estimate.duration.basis },
  };
}

function grantRequest(profile, cores, memoryTotalMiB, requested) {
  const maximum = grantableMaximum(profile, cores, memoryTotalMiB);
  const charged = clampToGrantable(requested, maximum);
  const declared = requested.declared ? clampToGrantable(requested.declared, maximum) : null;
  return {
    ...requested,
    cpus: charged.resources.cpus,
    memoryMiB: charged.resources.memoryMiB,
    ...(charged.clamped ? { clamped: charged.clamped } : {}),
    ...(declared ? { declared: declared.resources, ...(declared.clamped ? { declaredClamped: declared.clamped } : {}) } : {}),
  };
}

// Admission charges a declared class what this owner prefix's runs of the command measured.
function chargeFromUsage(requested, owner, command, cores) {
  const shape = commandShape(command);
  const prefix = ownerPrefix(owner);
  const runs = recentUsage(5000, entry => entry.class === requested.name && entry.shape === shape && typeof entry.owner === 'string'
    && ownerPrefix(entry.owner) === prefix && entry.wallSeconds > 0);
  const charged = chargedResources(requested.name, cores,
    runs.map(entry => entry.p90Cores ?? entry.peakCores), runs.map(entry => entry.peakMemoryMiB));
  return {
    ...requested, cpus: round(charged.cpus), memoryMiB: Math.ceil(charged.memoryMiB),
    declared: { cpus: requested.cpus, memoryMiB: requested.memoryMiB }, evidence: { key: `${prefix} ${shape}`, runs: runs.length },
  };
}

function readUsage(root, id) {
  try {
    return JSON.parse(readFileSync(join(root, 'leases', `${id}.usage`), 'utf8'));
  } catch {
    return null;
  }
}

const unleasedCoreThreshold = 1;
const unleasedStaleMilliseconds = 90000;
const unleasedSkipped = /^(session\.slice|native\.slice|init\.scope|agent\.slice\/agent-capacity\.slice)(\/|$)/;

function lastLine(path) {
  let fd;
  try {
    fd = openSync(path, 'r');
  } catch {
    return null;
  }
  try {
    const size = fstatSync(fd).size;
    const length = Math.min(size, 65536);
    const buffer = Buffer.alloc(length);
    readSync(fd, buffer, 0, length, size - length);
    return buffer.toString('utf8').trimEnd().split('\n').at(-1) ?? null;
  } finally {
    closeSync(fd);
  }
}

function readUnleased() {
  const directory = process.env.CAPACITY_WATCHDOG_DIR ?? join(process.env.HOME ?? '', '.local/state/agents/watchdog');
  try {
    const sample = JSON.parse(lastLine(join(directory, 'samples.jsonl')) ?? 'null');
    const now = Number(process.env.AGENT_CAPACITY_NOW ?? Date.now());
    if (sample === null || now - Date.parse(sample.at) > unleasedStaleMilliseconds) return null;
    const heavy = sample.cgroups
      .filter(({ cgroup, cores }) => cores > unleasedCoreThreshold && !unleasedSkipped.test(cgroup)
        && !sample.cgroups.some(other => other.cgroup.startsWith(`${cgroup}/`)))
      .map(({ cgroup, cores }) => ({
        cgroup, cores: round(cores),
        top: (sample.top ?? []).filter(item => item.cgroup === cgroup).slice(0, 3)
          .map(({ pid, cores: used, command }) => ({ pid, cores: used, command })),
      }));
    const sessions = (sample.top ?? []).filter(item => item.cores >= 0.25
      && sessionProviders.has((item.command.split(' ')[0] ?? '').split('/').at(-1)))
      .slice(0, 5).map(({ pid, cores, command }) => ({ pid, session: sessionFromArgs(command.split(' ')), cores, command }));
    const agentResidual = Math.max(0, (sample.cgroups.find(entry => entry.cgroup === 'agent.slice')?.cores ?? 0) - (sample.cgroups.find(entry => entry.cgroup === 'agent.slice/agent-capacity.slice')?.cores ?? 0) - heavy.filter(({ cgroup }) => cgroup.startsWith('agent.slice/')).reduce((sum, entry) => sum + entry.cores, 0));
    return { heavy, sessions, agentResidualCores: round(agentResidual), cores: round(heavy.reduce((sum, entry) => sum + entry.cores, 0) + agentResidual), windowSeconds: Math.round(sample.intervalSeconds) };
  } catch {
    return null;
  }
}

function readLeases(root, now) {
  const directory = join(root, 'leases');
  let reclaimed = 0;
  const active = [];
  for (const name of readdirSync(directory)) {
    if (!name.endsWith('.json')) continue;
    const path = join(directory, name);
    let lease;
    try {
      lease = JSON.parse(readFileSync(path, 'utf8'));
    } catch {
      fail(`malformed helper-owned lease: ${path}`);
    }
    const overdue = Number.isSafeInteger(lease.expiresAt) ? lease.expiresAt <= now && lease.frozenAt == null : lease.kind !== 'run';
    const wrapperGone = () => lease.wrapperStart == null
      ? now - lease.createdAt > runLeaseGraceMilliseconds : processStart(lease.wrapperPid) !== lease.wrapperStart;
    const expired = lease.kind !== 'run' ? overdue
      : overdue ? killScope(lease.id) || !scopeLive(lease.id)
      : wrapperGone() && !scopeLive(lease.id);
    if (expired) {
      unlinkSync(path);
      rmSync(path.replace(/\.json$/, '.usage'), { force: true });
      reclaimed += 1;
      continue;
    }
    active.push(lease);
  }
  return { active, reclaimed };
}

function totals(leases) {
  return leases.reduce((sum, lease) => ({
    cpus: sum.cpus + (lease.kind === 'run' ? lease.cpus : 0),
    batchCpus: sum.batchCpus + (lease.kind === 'run' && batchClass(lease.class) ? lease.cpus : 0),
    nativeCpus: sum.nativeCpus + (lease.kind === 'run' && lease.class === 'native' ? lease.cpus : 0),
    memoryMiB: sum.memoryMiB + lease.memoryMiB,
  }), { cpus: 0, batchCpus: 0, nativeCpus: 0, memoryMiB: 0 });
}

function cpuAccounting(root, leases, now) {
  const commitments = new Map();
  for (const lease of leases.filter(lease => lease.kind === 'run')) {
    const usage = readUsage(root, lease.id);
    const fresh = usage !== null && Number.isFinite(usage.at) && now - usage.at <= usageSampleMilliseconds * 3;
    const measured = fresh ? usage.liveCores ?? usage.recentCores ?? null : null;
    commitments.set(lease.id, runningCpuCommitment(lease.class, measured, lease.estimate?.cores.p50 ?? null, lease.cpus, scopeCpuQuota(lease.class, lease.cpus, lease.declared?.cpus)));
  }
  const unleased = readUnleased();
  const sum = filter => [...leases].filter(lease => lease.kind === 'run' && filter(lease))
    .reduce((total, lease) => total + commitments.get(lease.id).cpus, 0);
  return {
    commitments, unleased,
    committedBatchCpus: round(sum(lease => batchClass(lease.class)) + (unleased?.cores ?? 0)),
    committedNativeCpus: round(sum(lease => lease.class === 'native')),
  };
}

// Held batch wrappers wait in arrival order; a ticket whose wrapper died is dropped.
function enqueue(root, className, requestedClass, owner, timeoutSeconds, starts) {
  ensureState(root);
  const name = `${String(Date.now()).padStart(15, '0')}-${crypto.randomUUID()}.json`;
  writeFileSync(join(root, 'queue', name), `${JSON.stringify({
    ...wrapperIdentity(), class: className, requestedClass, owner, timeoutSeconds, ...starts,
  })}\n`, { encoding: 'utf8', mode: 0o600, flag: 'wx' });
  return name;
}

function dequeue(root, ticket) {
  rmSync(join(root, 'queue', ticket), { force: true });
}

function readQueue(root) {
  const directory = join(root, 'queue');
  const waiting = [];
  for (const name of readdirSync(directory).sort()) {
    if (!name.endsWith('.json')) continue;
    let ticket;
    try {
      ticket = JSON.parse(readFileSync(join(directory, name), 'utf8'));
    } catch {
      dequeue(root, name);
      continue;
    }
    if (processStart(ticket.wrapperPid) !== ticket.wrapperStart) {
      dequeue(root, name);
      continue;
    }
    waiting.push({ ...ticket, name, queuedAt: Number(name.slice(0, 15)) });
  }
  return waiting;
}

// Admission order: critical and tom, then exclusive, then the rest, each part in arrival order.
function priorityOrder(queue) {
  const rank = ticket => ['critical', 'tom'].includes(ticket.class) ? 0 : ticket.class === 'exclusive' ? 1 : 2;
  return [0, 1, 2].flatMap(level => queue.filter(ticket => rank(ticket) === level));
}

function leaseSlice(className) {
  return className === 'native' ? nativeSlice : aggregateSlice;
}

function nativeWaiting(root, now) {
  try {
    return now - statSync(join(root, 'native-waiting')).mtimeMs < nativeWaitingMilliseconds ? 1 : 0;
  } catch (error) {
    if (error?.code === 'ENOENT') return 0;
    throw error;
  }
}

function markNativeWaiting(root, waiting) {
  if (waiting) writeFileSync(join(root, 'native-waiting'), '', { mode: 0o600 });
  else rmSync(join(root, 'native-waiting'), { force: true });
}

const admittedClass = requested => requested.landing ? 'critical' : requested.name;

function admission(facts) {
  const result = Bun.spawnSync([northCommand, 'capacity', 'admission'], { stdin: Buffer.from(JSON.stringify(facts)), stdout: 'pipe', stderr: 'pipe' });
  if (result.exitCode !== 0) fail(`north capacity admission failed: ${result.stderr.toString().trim()}`, 75);
  return JSON.parse(result.stdout.toString());
}

function decision(root, requested, create, ticket = null) {
  const before = requested.name === 'native' && !process.env.AGENT_CAPACITY_GPU_BUSY ? takeGpuProcessSample() : null;
  const gpu = readGpu(requested.name === 'native' && !process.env.AGENT_CAPACITY_GPU_BUSY ? nativeGpuSamples : 1);
  const after = before !== null && gpu.gpuBusyPercent >= gpuBusyLimitPercent() ? takeGpuProcessSample() : null;
  return withLock(root, () => {
    const now = Date.now();
    const gpuBusy = recordGpuBusy(root, gpu.gpuBusyPercent, before?.at ?? now);
    const { active, reclaimed } = readLeases(root, now);
    const queue = readQueue(root);
    const runs = active.filter(lease => lease.kind === 'run');
    const drainEndsAt = Math.max(now, ...runs.filter(lease => batchClass(lease.class)).map(lease => lease.expiresAt));
    const beforeDrain = seconds => Number.isSafeInteger(seconds) && now + seconds * 1000 + runLeaseGraceMilliseconds <= drainEndsAt;
    const exclusiveWaiting = queue.filter(entry => entry.class === 'exclusive'
      && (requested.name !== 'exclusive' || ticket === null || entry.name < ticket)).length;
    // Gpu tickets wait for render slots and native tickets for client slots, each in their own line, so they hold no CPU work.
    const ahead = queue.filter(entry => (ticket === null || entry.name < ticket)
      && (entry.class === 'gpu') === (requested.name === 'gpu') && (entry.class === 'native') === (requested.name === 'native') && entry.class !== 'exclusive'
      && (exclusiveWaiting === 0 || entry.class !== 'heavy' || beforeDrain(entry.timeoutSeconds)));
    const leased = totals(active);
    const accounting = cpuAccounting(root, active, now);
    const { unleased, committedBatchCpus, committedNativeCpus } = accounting;
    const signals = readSignals();
    const { mode, profile } = activeProfile(root);
    const granted = grantRequest(profile, signals.cores, signals.memoryTotalMiB, requested);
    const quietOwners = runs.filter(lease => lease.class === 'native' && lease.quiet).map(lease => lease.owner);
    const { reason: code, ...advice } = admission({
      ...signals, ...gpu, profile, class: admittedClass(requested), gamingClass: requested.name, queuedAheadClasses: ahead.map(entry => entry.requestedClass), queuedAheadNewClients: ahead.map(entry => entry.newClients ?? 0), leasedMemoryMiB: leased.memoryMiB,
      leasedBatchCpus: round(committedBatchCpus - (unleased?.cores ?? 0)), leasedNativeCpus: committedNativeCpus, unleasedHeavyCores: unleased?.cores ?? 0,
      requestedCpus: granted.cpus, requestedMemoryMiB: granted.memoryMiB,
      peerBatchRuns: runs.filter(lease => batchClass(lease.class)).length,
      peerExclusiveRuns: runs.filter(lease => lease.class === 'exclusive').length,
      unboundedRuns: runs.filter(lease => lease.aggregateSlice !== leaseSlice(lease.class)).length,
      queuedAhead: ahead.length, exclusiveWaiting, beforeDrain: beforeDrain(create?.timeoutSeconds), newClients: requested.newClients, newDesktops: requested.newDesktops, gpuRuns: runs.filter(lease => lease.class === 'gpu').length,
      nativeWaiting: nativeWaiting(root, now), quietNativeRuns: quietOwners.length, besideNative: requested.besideNative !== undefined, quiet: requested.quiet === true, exclusiveLeases: runs.filter(lease => lease.class === 'exclusive').map(lease => lease.id), ...(ticket === null ? {} : { ticket: join(root, 'queue', ticket) }),
    });
    if (requested.name === 'native') markNativeWaiting(root, code !== 'RUN');
    const result = {
      decision: code === 'RUN' ? (create ? 'RESERVED' : 'RUN') : 'DEFER',
      reason: code,
      profile,
      mode,
      class: requested.name,
      requestedCpus: granted.cpus,
      requestedMemoryMiB: granted.memoryMiB,
      ...(granted.clamped ? { clamped: granted.clamped } : {}),
      ...(granted.declaredClamped ? { declaredClamped: granted.declaredClamped } : {}),
      scope: granted.declared ?? { cpus: granted.cpus, memoryMiB: granted.memoryMiB },
      leasedCpuCeilings: leased.cpus,
      leasedBatchCpus: leased.batchCpus,
      ...(requested.sizing ? { sizing: requested.sizing } : {}),
      ...(granted.declared ? { declaredCpus: granted.declared.cpus, declaredMemoryMiB: granted.declared.memoryMiB, evidence: requested.evidence } : {}),
      aggregateCpuLimit: aggregateCpus(profile, signals.cores),
      queuedAhead: ahead.length,
      exclusiveWaiting,
      leasedNativeCpus: leased.nativeCpus,
      leasedMemoryMiB: leased.memoryMiB,
      protectedCpuSomeAvg10: signals.protectedCpuSomeAvg10BasisPoints / 100,
      cpuSomeAvg10: signals.cpuSomeAvg10BasisPoints / 100,
      memoryFullAvg10: signals.memoryFullAvg10BasisPoints / 100,
      memoryAvailableMiB: signals.memoryAvailableMiB,
      ...gpu,
      ...(code === 'DEFER_GPU_BUSY' ? {
        ...gpuBusy,
        gpuProcess: after === null ? null : topUnleasedGpuProcess(before, after,
          runs.map(lease => scopeCgroup(lease.class, scopeUnit(lease.id)).slice(cgroupRoot.length))),
      } : {}),
      ...(code === 'DEFER_NATIVE_QUIET' ? { quietOwners } : {}), ...advice,
      gpuLeases: runs.filter(lease => lease.class === 'gpu').length,
      unleasedHeavyCores: unleased?.cores ?? null,
      committedBatchCpus,
      committedNativeCpus,
      reclaimed,
    };
    if (code !== 'RUN' || !create) return result;
    if (ticket !== null) dequeue(root, ticket);
    const id = crypto.randomUUID();
    const lease = {
      schema: 'agent-capacity-lease/v1',
      id,
      kind: create.kind,
      ...(requested.quiet ? { quiet: true } : {}),
      class: requested.name,
      aggregateSlice: create.kind === 'run' ? leaseSlice(requested.name) : null,
      owner: create.owner,
      cpus: granted.cpus,
      memoryMiB: granted.memoryMiB,
      ...(requested.estimate ? { estimate: requested.estimate } : {}),
      ...(granted.declared ? { declared: granted.declared, evidence: requested.evidence } : {}),
      createdAt: now,
      expiresAt: create.timeoutSeconds === null ? null : now + create.timeoutSeconds * 1000
        + (create.kind === 'run' ? runLeaseGraceMilliseconds : 0),
      ...(create.kind === 'run' ? wrapperIdentity() : {}),
    };
    writeFileSync(join(root, 'leases', `${id}.json`), `${JSON.stringify(lease)}\n`, {
      encoding: 'utf8',
      mode: 0o600,
      flag: 'wx',
    });
    return { ...result, lease: id, expiresAt: lease.expiresAt };
  });
}

function exactLease(root, id) {
  if (!/^[0-9a-f-]{36}$/.test(id)) fail('invalid lease identity');
  return join(root, 'leases', `${id}.json`);
}

function changeLease(root, id, owner, timeoutSeconds, settledRun = false) {
  return withLock(root, () => {
    const path = exactLease(root, id);
    if (!existsSync(path)) return settledRun ? { decision: 'RECLAIMED', lease: id } : fail(`unknown or expired lease: ${id}`, 75);
    const lease = JSON.parse(readFileSync(path, 'utf8'));
    if (lease.owner !== owner) fail(`lease owner mismatch: ${id}`);
    if (lease.kind === 'run' && !settledRun) fail('run leases belong to their foreground wrapper');
    if (timeoutSeconds === null) {
      unlinkSync(path);
      rmSync(path.replace(/\.json$/, '.usage'), { force: true });
      return { decision: 'RELEASED', lease: id };
    }
    if (lease.expiresAt <= Date.now()) {
      unlinkSync(path);
      fail(`lease already expired: ${id}`, 75);
    }
    lease.expiresAt = Date.now() + timeoutSeconds * 1000;
    writeFileSync(path, `${JSON.stringify(lease)}\n`, { encoding: 'utf8', mode: 0o600 });
    return { decision: 'RENEWED', lease: id, expiresAt: lease.expiresAt };
  });
}

function measuredView(usage) {
  if (usage === null) return null;
  return {
    cores: round(usage.recentCores), liveCores: usage.liveCores == null ? null : round(usage.liveCores), liveWindowSeconds: usage.liveWindowSeconds ?? null, peakCores: round(usage.peakCores), meanCores: round(usage.meanCores),
    memoryMiB: Math.round(usage.memoryMiB), peakMemoryMiB: Math.round(usage.peakMemoryMiB), gpuSeconds: round(usage.gpuSeconds),
  };
}

function status(root) {
  const gpu = readGpu(1);
  return withLock(root, () => {
    const now = Date.now();
    const gpuBusy = recordGpuBusy(root, gpu.gpuBusyPercent, now);
    const { active, reclaimed } = readLeases(root, now);
    const { mode, profile } = activeProfile(root);
    const signals = readSignals();
    const accounting = cpuAccounting(root, active, now);
    const seconds = milliseconds => Math.round(milliseconds / 1000);
    const draining = active.filter(lease => lease.kind === 'run' && batchClass(lease.class))
      .map(lease => ({ owner: lease.owner, class: lease.class, remainingSeconds: seconds(lease.expiresAt - now) }));
    return {
      profile,
      mode,
      ...gpu,
      ...gpuBusy,
      aggregateCpuLimit: aggregateCpus(profile, signals.cores),
      protectedCpuSomeAvg10: signals.protectedCpuSomeAvg10BasisPoints / 100,
      cpuSomeAvg10: signals.cpuSomeAvg10BasisPoints / 100,
      committedBatchCpus: accounting.committedBatchCpus,
      committedNativeCpus: accounting.committedNativeCpus,
      holding: active.sort((left, right) => left.createdAt - right.createdAt).map(lease => ({
        owner: lease.owner,
        class: lease.class,
        kind: lease.kind,
        cpus: lease.cpus,
        memoryMiB: lease.memoryMiB,
        declaredCpus: lease.declared?.cpus ?? lease.cpus,
        declaredMemoryMiB: lease.declared?.memoryMiB ?? lease.memoryMiB,
        evidenceRuns: lease.evidence?.runs ?? 0,
        estimate: lease.estimate ?? null,
        committedCpus: lease.kind === 'run' ? round(accounting.commitments.get(lease.id).cpus) : 0,
        cpuBasis: lease.kind === 'run' ? accounting.commitments.get(lease.id).basis : 'agent-memory-only',
        measured: lease.kind === 'run' ? measuredView(readUsage(root, lease.id)) : null,
        heldSeconds: seconds(now - lease.createdAt),
        remainingSeconds: lease.expiresAt === null ? null : seconds(lease.expiresAt - now),
      })),
      queued: priorityOrder(readQueue(root)).map((ticket, index) => ({
        position: index + 1,
        owner: ticket.owner ?? null,
        class: ticket.class ?? null, reason: ticket.reason ?? null,
        waitingSeconds: seconds(now - ticket.queuedAt), newClients: ticket.newClients, newDesktops: ticket.newDesktops,
        ...(ticket.class === 'exclusive' ? { waitingFor: draining } : {}),
      })),
      unleasedHeavy: accounting.unleased,
      reclaimed,
    };
  });
}

function print(result, fd = 1) {
  writeFileSync(fd, `${JSON.stringify(result)}\n`);
}

function parseOwner(values, option = '--owner') {
  const owner = required(values, option);
  if (!/^[A-Za-z0-9_.:@/-]{1,160}$/.test(owner)) {
    fail(`${option} must be a stable actor label without whitespace`);
  }
  return owner;
}

function leaseChange(root, milliseconds) {
  return new Promise(resolve => {
    const done = () => { clearTimeout(timer); watcher.close(); resolve(); };
    const watcher = watch(join(root, 'leases'), (_, name) => { if (name?.endsWith('.json')) done(); });
    const timer = setTimeout(done, milliseconds);
  });
}

// Batch work, and a native client or desktop start given --wait-seconds, is held in arrival order until admitted, its --wait-seconds pass (ending in its last DEFER), or a quiet native session outlasts its timeout (REFUSED).
async function admit(root, requested, create) {
  if (!batchClass(requested.name) && requested.waitSeconds === undefined) return decision(root, requested, create);
  const queuedAt = Date.now();
  const ticket = enqueue(root, admittedClass(requested), requested.name, create.owner, create.timeoutSeconds, { newClients: requested.newClients, newDesktops: requested.newDesktops });
  const leave = signal => () => {
    dequeue(root, ticket);
    process.exit(128 + ({ SIGHUP: 1, SIGINT: 2, SIGTERM: 15 })[signal]);
  };
  const handlers = terminationSignals.map(signal => [signal, leave(signal)]);
  for (const [signal, handler] of handlers) process.on(signal, handler);
  try {
    let reported = null;
    for (;;) {
      const result = { ...decision(root, requested, create, ticket), queuedSeconds: round((Date.now() - queuedAt) / 1000) };
      if (result.decision === 'RESERVED' || result.queuedSeconds >= (requested.waitSeconds ?? Infinity)) return result;
      if (result.reason === 'DEFER_NATIVE_QUIET' && create.timeoutSeconds !== null && result.queuedSeconds >= create.timeoutSeconds) return { ...result, decision: 'REFUSED' };
      if (result.reason !== reported) {
        print({ ...result, decision: 'QUEUED' }, 2);
        reported = result.reason;
      }
      await leaseChange(root, queuePollMilliseconds + Math.random() * queuePollMilliseconds);
    }
  } finally {
    dequeue(root, ticket);
    for (const [signal, handler] of handlers) process.removeListener(signal, handler);
  }
}

function nestedParent(root) {
  const scope = readText('/proc/self/cgroup')?.match(/\/agent-capacity-([0-9a-f]{8})([0-9a-f]{4})([0-9a-f]{4})([0-9a-f]{4})([0-9a-f]{12})\.scope(\/|$)/m);
  const ids = [process.env.AGENT_CAPACITY_LEASE, scope?.slice(1, 6).join('-')];
  return withLock(root, () => readLeases(root, Date.now()).active
    .find(lease => lease.kind === 'run' && batchClass(lease.class) && ids.includes(lease.id))?.id ?? null);
}

async function runScoped(root, requested, owner, timeoutSeconds, command) {
  if (requested.name === 'agent') fail('run --class must be moderate, heavy, exclusive, gpu, critical, tom, or native');
  const parent = batchClass(requested.name) ? nestedParent(root) : null;
  if (parent !== null) {
    print({ decision: 'NESTED', parent, class: requested.name, owner }, 2);
    return Bun.spawn([northCommand, 'capacity', 'deadline', exactLease(root, parent), `${timeoutSeconds}`, '--', ...command], { stdin: 'inherit', stdout: 'inherit', stderr: 'inherit' }).exited;
  }
  // The parent limit and weights must exist before any admitted command executes.
  if (!applyProfile(activeProfile(root).profile, readSignals().cores)) {
    fail('cannot establish aggregate CPU limit and native weight', 75);
  }
  const native = requested.name === 'native';
  const admitted = await admit(root, requested, { kind: 'run', owner, timeoutSeconds });
  print(admitted, 2);
  if (admitted.decision === 'REFUSED') fail(`refused after ${admitted.queuedSeconds}s: native evidence run ${admitted.quietOwners.join(', ')} is live; retry after it ends`, 75);
  if (admitted.decision !== 'RESERVED') return 75;
  const limits = admitted.scope;
  const unit = scopeUnit(admitted.lease);
  const stop = () => systemctl('ignore', 'stop', unit);
  try {
    const child = Bun.spawn([
      ...(process.env.NORTH_FLEET_SCOPE ? [northCommand, 'session-unit'] : ['systemd-run']), '--user', '--scope', '--quiet', '--collect', '--expand-environment=no', ...(process.env.XDG_RUNTIME_DIR && process.env.DBUS_SESSION_BUS_ADDRESS ? [`--setenv=XDG_RUNTIME_DIR=${process.env.XDG_RUNTIME_DIR}`, `--setenv=DBUS_SESSION_BUS_ADDRESS=${process.env.DBUS_SESSION_BUS_ADDRESS}`] : []),
      `--unit=${unit}`,
      `--slice=${leaseSlice(requested.name)}`,
      // A quota-throttled game client stalls for the rest of each period.
      ...(native ? [] : [`--property=CPUQuota=${limits.cpus * 100}%`]),
      `--property=MemoryHigh=${limits.memoryMiB}M`,
      '--property=KillMode=control-group',
      ...(frozenClasses.has(requested.name) ? ['--', northCommand, 'capacity', 'deadline', exactLease(root, admitted.lease), `${timeoutSeconds}`, '--'] : [`--property=RuntimeMaxSec=${timeoutSeconds === null ? 'infinity' : `${timeoutSeconds}s`}`, '--']), ...command,
    ], { stdin: 'inherit', stdout: 'inherit', stderr: 'inherit', env: { ...systemdClientEnv(), AGENT_CAPACITY_LEASE: admitted.lease } });
    const measurement = measureScope(root, admitted.lease, scopeCgroup(requested.name, unit));
    for (const signal of terminationSignals) process.once(signal, stop);
    const exitCode = await child.exited;
    const usage = measurement.finish();
    const rusage = child.resourceUsage();
    const cpuSeconds = Math.max(usage.cpuSeconds, Number(rusage?.cpuTime?.total ?? 0n) / 1e6);
    try {
      const entry = {
        at: new Date().toISOString(), owner, shape: commandShape(command), command: recordedCommand(command), class: requested.name,
        reservedCpus: limits.cpus, reservedMemoryMiB: limits.memoryMiB,
        chargedCpus: admitted.requestedCpus, chargedMemoryMiB: admitted.requestedMemoryMiB, exitCode,
        wallSeconds: round(usage.wallSeconds), cpuSeconds: round(cpuSeconds),
        meanCores: round(cpuSeconds / Math.max(usage.wallSeconds, 0.001)),
        p90Cores: round(usage.p90Cores ?? cpuSeconds / Math.max(usage.wallSeconds, 0.001)),
        peakCores: round(Math.max(usage.peakCores, cpuSeconds / Math.max(usage.wallSeconds, 0.001))),
        peakMemoryMiB: Math.round(Math.max(usage.peakMemoryMiB, (rusage?.maxRSS ?? 0) / 1024)),
        gpuSeconds: round(usage.gpuSeconds), queuedSeconds: admitted.queuedSeconds ?? 0,
        ...(requested.besideNative === undefined ? {} : { besideNative: requested.besideNative }),
      };
      try {
        const db = capacityDatabase();
        try {
          insertCapacityRun(db, `lease:${admitted.lease}`, { ...entry,
            ioReadBytes: usage.ioReadBytes, ioWriteBytes: usage.ioWriteBytes, ioPressureSomeUsec: usage.ioPressureSomeUsec, diskBusyShare: usage.diskBusyShare,
          }, taskSignature(command, process.cwd(), requested.name), process.cwd());
        } finally { db.close(); }
      } catch (error) {
        writeFileSync(2, `machine-capacity: capacity database failed: ${error?.message ?? String(error)}\n`);
      }
      recordUsage(entry);
    } catch (error) {
      writeFileSync(2, `machine-capacity: usage log failed: ${error?.message ?? String(error)}\n`);
    }
    return exitCode;
  } finally {
    stop();
    for (const signal of terminationSignals) process.removeListener(signal, stop);
    try {
      if (scopeLive(admitted.lease)) throw new Error(`scope still live; retained lease ${admitted.lease}`);
      print(changeLease(root, admitted.lease, owner, null, true), 2);
    } catch (error) {
      writeFileSync(2, `machine-capacity: lease cleanup failed: ${error?.message ?? String(error)}\n`);
    }
  }
}

async function main(argv) {
  const operation = argv[0];
  if (operation === 'estimate') {
    const json = argv[1] === '--json';
    const start = json ? 2 : 1;
    const parsed = parseKeyValues(argv, start, new Set(['--class', '--owner']));
    const command = argv.slice(parsed.separator + 1);
    if (parsed.separator === argv.length || command.length === 0) fail('estimate requires -- COMMAND ARG...');
    const kind = parsed.values.get('--class') ?? 'moderate';
    if (!classNames.has(kind)) fail('unknown resource class');
    const owner = parsed.values.has('--owner') ? parseOwner(parsed.values) : undefined;
    const estimate = estimateCommand(command, kind, owner, hostCoreCount);
    if (json) print(estimate);
    else process.stdout.write(`${estimate.signature.family} repo=${known(estimate.signature.repo)} package=${estimate.signature.package ?? 'none'} class=${estimate.class}\n`
      + `cores p50=${estimate.cores.p50} p90=${estimate.cores.p90}; peak memory MiB p50=${estimate.peakMemoryMiB.p50} p90=${estimate.peakMemoryMiB.p90}\n`
      + `I/O read bytes p50=${known(estimate.ioReadBytes.p50)} p90=${known(estimate.ioReadBytes.p90)}; write bytes p50=${known(estimate.ioWriteBytes.p50)} p90=${known(estimate.ioWriteBytes.p90)}; disk busy share p50=${known(estimate.diskBusyShare.p50)} p90=${known(estimate.diskBusyShare.p90)} (${estimate.diskBusyBasis})\n`
      + `samples=${estimate.sampleCount} match=${estimate.matchLevel} confidence=${estimate.confidence} I/O samples=${estimate.ioSampleCount}\n`);
    return 0;
  }
  if (operation === 'fixture') {
    const parsed = parseKeyValues(argv, 1, new Set([
      '--profile', '--class', '--cores', '--memory-total-mib', '--memory-available-mib',
      '--protected-cpu-some-avg10-basis-points', '--memory-full-avg10-basis-points',
      '--leased-cpus', '--leased-native-cpus', '--leased-memory-mib',
      '--peer-batch-runs', '--peer-exclusive-runs', '--unbounded-runs',
      '--memory-gib', '--cpu-some-avg10-basis-points', '--queued-ahead', '--exclusive-waiting',
      '--gpu-busy-percent', '--gpu-clients', '--new-clients', '--new-desktops', '--gpu-runs', '--native-waiting', '--quiet-native-runs', '--beside-native', '--run-queue-load',
    ]));
    if (parsed.separator !== argv.length) fail('fixture accepts no command');
    const cores = parsePositiveInteger(required(parsed.values, '--cores'), '--cores');
    const requested = parseClass(parsed.values, cores);
    const memoryFullAvg10 = parseNonnegativeInteger(
      required(parsed.values, '--memory-full-avg10-basis-points'), '--memory-full-avg10-basis-points',
    ) / 100;
    const leasedCpuCeilings = Number(required(parsed.values, '--leased-cpus'));
    if (!Number.isFinite(leasedCpuCeilings) || leasedCpuCeilings < 0) fail('--leased-cpus requires nonnegative finite cores');
    const profile = required(parsed.values, '--profile');
    if (profile !== 'attended' && profile !== 'unattended') fail('--profile must be attended or unattended');
    const memoryTotalMiB = parsePositiveInteger(required(parsed.values, '--memory-total-mib'), '--memory-total-mib');
    const granted = grantRequest(profile, cores, memoryTotalMiB, requested);
    const count = option => parseNonnegativeInteger(parsed.values.get(option) ?? '0', option);
    const { reason: code } = admission({
      profile, class: requested.name, cores, memoryTotalMiB, requestedCpus: granted.cpus, requestedMemoryMiB: granted.memoryMiB,
      memoryAvailableMiB: parsePositiveInteger(required(parsed.values, '--memory-available-mib'), '--memory-available-mib'),
      protectedCpuSomeAvg10BasisPoints: parseNonnegativeInteger(required(parsed.values, '--protected-cpu-some-avg10-basis-points'), '--protected-cpu-some-avg10-basis-points'),
      leasedMemoryMiB: parseNonnegativeInteger(required(parsed.values, '--leased-memory-mib'), '--leased-memory-mib'),
      leasedBatchCpus: leasedCpuCeilings, leasedNativeCpus: count('--leased-native-cpus'), cpuSomeAvg10BasisPoints: count('--cpu-some-avg10-basis-points'),
      peerBatchRuns: count('--peer-batch-runs'), peerExclusiveRuns: count('--peer-exclusive-runs'), unboundedRuns: count('--unbounded-runs'),
      queuedAhead: count('--queued-ahead'), exclusiveWaiting: count('--exclusive-waiting'), gpuBusyPercent: count('--gpu-busy-percent'),
      gpuClients: count('--gpu-clients'), newClients: count('--new-clients'), newDesktops: count('--new-desktops'), gpuRuns: count('--gpu-runs'), nativeWaiting: count('--native-waiting'),
      quietNativeRuns: count('--quiet-native-runs'), besideNative: parsed.values.has('--beside-native'), runQueueLoad: count('--run-queue-load'),
    });
    print({ decision: code, profile, class: requested.name, cpus: granted.cpus, memoryMiB: granted.memoryMiB,
      ...(granted.clamped ? { clamped: granted.clamped } : {}), memoryFullAvg10,
      leasedCpuCeilings, aggregateCpuLimit: aggregateCpus(profile, cores) });
    return code === 'RUN' ? 0 : 75;
  }
  const root = runtimeRoot();
  if (operation !== 'mode' && (process.env.XDG_RUNTIME_DIR === undefined || root.startsWith(`/run/user/${process.getuid()}/`))) {
    ensureProfile(root);
  }
  if (operation === 'mode') {
    if (argv.length > 2) fail('usage: mode [away|present|auto]');
    if (argv.length === 2) {
      if (!modes.has(argv[1])) fail('mode must be away, present, or auto');
      ensureState(root);
      writeFileSync(join(root, 'mode'), `${argv[1]}\n`, { encoding: 'utf8', mode: 0o600 });
    }
    const active = activeProfile(root);
    if (!ensureProfile(root, true)) fail('cannot apply capacity profile', 75);
    print({ ...active,
      aggregateCpuLimit: aggregateCpus(active.profile, readSignals().cores) });
    return 0;
  }
  if (operation === 'status') {
    if (argv.length !== 1) fail('status accepts no arguments');
    print(status(root));
    return 0;
  }
  if (operation === 'probe') {
    const { values, separator } = parseKeyValues(argv, 1, new Set(['--class', '--memory-gib', '--beside-native', '--new-clients', '--new-desktops']));
    if (separator !== argv.length) fail('probe accepts no command');
    const signals = readSignals();
    const besideNative = values.has('--beside-native') ? { besideNative: parseOwner(values, '--beside-native') } : {};
    const result = decision(root, { ...parseClass(values, signals.cores), ...besideNative }, null);
    print(result);
    return result.decision === 'RUN' ? 0 : 75;
  }
  if (operation === 'reserve') {
    const parsed = parseKeyValues(argv, 1, new Set(['--class', '--owner', '--timeout-seconds']));
    if (parsed.separator !== argv.length) fail('reserve accepts no command');
    if (!reserveClass(required(parsed.values, '--class'))) fail('reserve --class must be agent');
    const signals = readSignals();
    const requested = parseClass(parsed.values, signals.cores);
    const owner = parseOwner(parsed.values);
    const timeoutSeconds = parsePositiveInteger(
      required(parsed.values, '--timeout-seconds'), '--timeout-seconds', maximumSeconds('agent'),
    );
    const result = decision(root, requested, { kind: 'agent', owner, timeoutSeconds });
    print(result);
    return result.decision === 'RESERVED' ? 0 : 75;
  }
  if (operation === 'renew' || operation === 'release') {
    const allowed = operation === 'renew'
      ? new Set(['--lease', '--owner', '--timeout-seconds'])
      : new Set(['--lease', '--owner']);
    const parsed = parseKeyValues(argv, 1, allowed);
    if (parsed.separator !== argv.length) fail(`${operation} accepts no command`);
    const timeoutSeconds = operation === 'renew'
      ? parsePositiveInteger(required(parsed.values, '--timeout-seconds'), '--timeout-seconds', maximumSeconds('agent'))
      : null;
    print(changeLease(root, required(parsed.values, '--lease'), parseOwner(parsed.values), timeoutSeconds));
    return 0;
  }
  if (operation === 'run' || operation === 'session') {
    const parsed = parseKeyValues(argv, 1, new Set(['--class', '--owner', '--timeout-seconds', '--memory-gib', '--beside-native', '--new-clients', '--new-desktops', '--quiet', '--wait-seconds']), new Set(['--quiet']));
    const command = argv.slice(parsed.separator + 1);
    if (parsed.separator === argv.length || command.length === 0) fail(`${operation} requires -- COMMAND ARG...`);
    const signals = readSignals();
    if (!parsed.values.has('--class') && operation === 'session') fail('session requires --class');
    const owner = parseOwner(parsed.values);
    const requested = parsed.values.has('--memory-gib') ? parseClass(parsed.values, signals.cores)
      : parsed.values.has('--class') ? chargeFromUsage(parseClass(parsed.values, signals.cores), owner, command, signals.cores)
        : sizeFromUsage(command, owner, signals.cores);
    requested.estimate ??= estimateCommand(command, requested.name, owner, signals.cores);
    if (parsed.values.has('--beside-native')) requested.besideNative = parseOwner(parsed.values, '--beside-native');
    if (parsed.values.has('--quiet')) requested.quiet = requested.name === 'native' || fail('--quiet marks a native evidence session');
    if (operation === 'run' && owner === 'land-check' && process.env.SAFE_PUSH_LAND_HEAD && batchClass(requested.name)) requested.landing = true;
    // A session without one takes its class's default deadline; native sessions have none.
    const timeoutSeconds = parsed.values.has('--timeout-seconds') || operation === 'run'
      ? parsePositiveInteger(required(parsed.values, '--timeout-seconds'), '--timeout-seconds',
        maximumSeconds(requested.name))
      : sessionSeconds(requested.name) || null;
    return runScoped(root, requested, owner, timeoutSeconds, command);
  }
  fail('usage: estimate [--json] [--class CLASS] [--owner OWNER] -- CMD ARG...|status|probe|mode|reserve|renew|release|run|session [--new-clients N] [--new-desktops N] [--wait-seconds N] [--quiet] [--beside-native NATIVE-OWNER]; see machine-capacity');
}

process.exitCode = await main(Bun.argv.slice(2));
