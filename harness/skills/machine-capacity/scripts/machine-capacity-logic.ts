import registry from '../../../defaults.toml';

export const DEFAULTS: Record<string, any> = Object.fromEntries(Object.entries(registry as Record<string, { value: unknown }>).map(([name, entry]) => [name, entry.value]));

export interface Resources {
  cpus: number;
  memoryMiB: number;
}

export function reservedCpus(profile: string, cores: number): number {
  return profile === 'attended' ? (cores > 8 ? 4 : cores > 4 ? 2 : 1) : 0;
}

export function aggregateCpus(profile: string, cores: number): number {
  return cores - reservedCpus(profile, cores);
}

export function grantableMaximum(profile: string, cores: number, memoryTotalMiB: number): Resources {
  return { cpus: aggregateCpus(profile, cores), memoryMiB: Math.floor(memoryTotalMiB * 3 / 4) };
}

export interface Clamped {
  cpus?: [number, number];
  memoryMiB?: [number, number];
}

export function clampToGrantable(request: Resources, maximum: Resources): { resources: Resources; clamped: Clamped | null } {
  const cpus = Math.min(request.cpus, maximum.cpus);
  const memoryMiB = Math.min(request.memoryMiB, maximum.memoryMiB);
  const clamped: Clamped = {};
  if (cpus !== request.cpus) clamped.cpus = [request.cpus, cpus];
  if (memoryMiB !== request.memoryMiB) clamped.memoryMiB = [request.memoryMiB, memoryMiB];
  return { resources: { cpus, memoryMiB }, clamped: Object.keys(clamped).length > 0 ? clamped : null };
}

export function resourceClass(kind: string, cores: number): Resources {
  switch (kind) {
    case 'agent': return { cpus: 0, memoryMiB: 768 };
    case 'native': return { cpus: cores > 1 ? 2 : 1, memoryMiB: 4096 };
    case 'moderate': return { cpus: cores > 1 ? 2 : 1, memoryMiB: 2048 };
    case 'tom': return { cpus: cores > 1 ? 2 : 1, memoryMiB: 2048 };
    case 'heavy': return { cpus: cores < 8 ? cores : cores > 8 ? 6 : 3, memoryMiB: 8192 };
    case 'gpu': return { cpus: 1, memoryMiB: 2048 };
    case 'critical': return { cpus: cores < 8 ? cores : cores - reservedCpus('attended', cores), memoryMiB: 16384 };
    case 'exclusive': return { cpus: cores, memoryMiB: 16384 };
    default: throw new Error('Unknown resource class');
  }
}

export function reserveClass(kind: string): boolean {
  return kind === 'agent';
}

export function nativeMemoryRequest(kind: string, cores: number, memoryGiB: number): Resources {
  const base = resourceClass(kind, cores);
  if (kind !== 'native' || !(memoryGiB > 0)) {
    throw new Error('--memory-gib requires a positive native memory request');
  }
  return { cpus: base.cpus, memoryMiB: memoryGiB * 1024 };
}

export function batchClass(kind: string): boolean {
  return ['moderate', 'heavy', 'exclusive', 'gpu', 'critical', 'tom'].includes(kind);
}

export function maximumSeconds(kind: string): number {
  return kind === 'exclusive' ? 900 : 3600;
}

export function sessionSeconds(kind: string): number {
  return kind === 'exclusive' ? 900 : batchClass(kind) ? 1800 : 0;
}

export function gpuBusyLimitPercent(): number {
  return 85;
}

export function gpuBusyObservation(now: number, busyPercent: number, since: number | null) {
  const gpuBusySince = busyPercent >= gpuBusyLimitPercent() ? since ?? now : null;
  return { gpuBusySince, gpuBusySeconds: gpuBusySince === null ? 0 : Math.max(0, now - gpuBusySince) / 1000 };
}

export interface GpuClient {
  client: string;
  pid: number;
  start: string;
  command: string;
  cgroup: string;
  nanoseconds: number;
}

export interface GpuProcessSample {
  at: number;
  clients: readonly GpuClient[];
}

export function topUnleasedGpuProcess(before: GpuProcessSample, after: GpuProcessSample, leasedCgroups: readonly string[]) {
  if (after.at <= before.at) return null;
  const leased = (client: GpuClient) => leasedCgroups.some(group => client.cgroup === group || client.cgroup.startsWith(`${group}/`));
  const leasedClients = new Set(after.clients.filter(leased).map(client => client.client));
  const previous = new Map(before.clients.map(client => [client.client, client]));
  const clients = new Map<string, GpuClient>();
  for (const client of after.clients) {
    if (!leasedClients.has(client.client) && (!clients.has(client.client) || client.nanoseconds > clients.get(client.client)!.nanoseconds)) {
      clients.set(client.client, client);
    }
  }
  const processes = new Map<number, { pid: number; command: string; nanoseconds: number }>();
  for (const client of clients.values()) {
    const older = previous.get(client.client);
    if (!older || older.pid !== client.pid || older.start !== client.start) continue;
    const delta = Math.max(0, client.nanoseconds - older.nanoseconds);
    const process = processes.get(client.pid) ?? { pid: client.pid, command: client.command, nanoseconds: 0 };
    process.nanoseconds += delta;
    processes.set(client.pid, process);
  }
  const top = [...processes.values()].filter(process => process.nanoseconds > 0)
    .sort((left, right) => right.nanoseconds - left.nanoseconds || left.pid - right.pid)[0];
  return top === undefined ? null : {
    pid: top.pid, command: top.command,
    gpuBusyPercent: Math.round(Math.min(100, top.nanoseconds / ((after.at - before.at) * 1e6) * 100) * 100) / 100,
  };
}

export function sizingHeadroom(): number {
  return 1.25;
}

export function sizedClass(gpuShare: number, cpus: number, memoryMiB: number): string {
  return gpuShare >= 0.2 ? 'gpu' : cpus <= 2 && memoryMiB <= 2048 ? 'moderate' : 'heavy';
}

export function sizedResources(cpus: number, memoryMiB: number, cores: number): Resources {
  return { cpus: Math.max(1, Math.min(cpus, resourceClass('heavy', cores).cpus)), memoryMiB: Math.max(512, memoryMiB) };
}

export function p90(values: readonly number[]): number {
  let best = -1;
  for (const value of values) {
    const below = values.reduce((count, other) => count + (other <= value ? 1 : 0), 0);
    if (below * 10 >= values.length * 9 && (best < 0 || value < best)) best = value;
  }
  return best;
}

export function learnedClass(kind: string): boolean {
  return ['moderate', 'heavy', 'native', 'gpu'].includes(kind);
}

export function learnedMinimumRuns(): number {
  return 3;
}

export function chargedResources(kind: string, cores: number, runCores: readonly number[], runMemoryMiB: readonly number[]): Resources {
  const declared = resourceClass(kind, cores);
  if (!learnedClass(kind) || runCores.length < learnedMinimumRuns() || runMemoryMiB.length < learnedMinimumRuns()) return declared;
  const cpus = Math.max(1, Math.min(p90(runCores), declared.cpus * 1.5));
  return { cpus: kind === 'heavy' ? Math.max(cpus, declared.cpus) : cpus, memoryMiB: p90(runMemoryMiB) * DEFAULTS["capacity_memory_margin"] };
}

export interface TaskSignature {
  family: string;
  repo: string | null;
  package: string | null;
  class: string;
  exact: boolean;
}

export function taskSignature(argv: readonly string[], cwd: string | null, kind: string, exact = true): TaskSignature {
  let words = [...argv];
  if (words[0]?.split('/').at(-1) === 'nix' && words[1] === 'develop' && words.includes('-c')) {
    words = words.slice(words.indexOf('-c') + 1);
  }
  if (words[0]?.split('/').at(-1) === 'env') {
    words = words.slice(1);
    while (words[0]?.includes('=')) words.shift();
  }
  const program = words[0]?.split('/').at(-1) ?? '';
  const repo = cwd?.match(/^(?:~|\/home\/[^/]+)\/code\/([^/]+)(?:\/|$)/)?.[1] ?? null;
  let family = 'other';
  if (program === 'cargo' && ['build', 'test'].includes(words[1])) family = `cargo ${words[1]}`;
  else if (program === 'bun' && words[1] === 'test') family = 'bun test';
  else if (program === 'bun' && words[1] === 'wisp' && words[2] === 'map' && words[3] === 'build') family = 'bun wisp map build';
  else if ((program === 'bun' && words[1] === 'wisp' && (words[2] === 'view' || (words[2] === 'headless' && words.includes('--render')))) || program === 'wisp-render') family = 'wisp render';
  else if (['codex', 'claude'].includes(program)) family = `${program} agent`;
  else if (kind === 'native' || ['wine', 'wine64', 'proton', 'steam-run'].includes(program) || /\.exe$/i.test(program)) family = 'native client';
  else if (program === 'nixos-rebuild' || ((repo === 'firn' || repo === 'nixos-config' || program === 'firn') && words.some(word => ['build', 'validate', 'rebuild'].includes(word)))) {
    family = `firn ${program === 'nixos-rebuild' ? 'rebuild' : words.find(word => ['build', 'validate', 'rebuild'].includes(word))}`;
  }
  let target: string | null = null;
  for (let index = 1; index < words.length; index += 1) {
    const word = words[index];
    if (program === 'cargo' && ['-p', '--package', '--manifest-path'].includes(word)) target = words[index + 1] ?? null;
    else if (program === 'cargo' && word.startsWith('--package=')) target = word.slice(10);
    else if (program === 'cargo' && word.startsWith('--manifest-path=')) target = word.slice(16);
    else if (program === 'cargo' && word.startsWith('-p') && word.length > 2) target = word.slice(2);
    else if (!['codex', 'claude'].includes(program) && !word.startsWith('-') && !/\s/.test(word) && (word.includes('/') || /\.(?:ts|js|mjs|toml)$/.test(word))) target ??= word.replace(/^\.\//, '');
  }
  return { family, repo, package: exact ? target : null, class: kind, exact };
}

export interface CapacityRun {
  at: string;
  signature: TaskSignature;
  owner?: string | null;
  shape?: string | null;
  meanCores: number;
  p90Cores: number | null;
  peakMemoryMiB: number;
  ioReadBytes: number | null;
  ioWriteBytes: number | null;
  ioPressureSomeUsec: number | null;
  diskBusyShare: number | null;
  wallSeconds?: number | null;
}

export const estimateRuns = DEFAULTS["capacity_sizing_runs"];

export const ownerPrefix = (owner: string): string => owner.match(/^[A-Za-z]*/)![0];
const pooledFamily = (family: string) => family === 'other' ? null : family;

export interface CapacityQuery {
  owner?: string | null;
  shape?: string | null;
}

function quantile(values: readonly number[], share: number): number | null {
  if (values.length === 0) return null;
  const sorted = [...values].sort((a, b) => a - b);
  return sorted[Math.ceil(sorted.length * share) - 1];
}

const sameExact = (signature: TaskSignature, run: CapacityRun) => signature.exact && run.signature.exact
  && run.signature.family === signature.family && run.signature.repo === signature.repo
  && run.signature.package === signature.package && run.signature.class === signature.class;

export function estimateDuration(signature: TaskSignature, history: readonly CapacityRun[]) {
  const seconds = history.filter(run => sameExact(signature, run) && typeof run.wallSeconds === 'number'
    && Number.isFinite(run.wallSeconds) && run.wallSeconds > 0 && (run.exitCode ?? 0) === 0)
    .sort((a, b) => a.at.localeCompare(b.at)).slice(-estimateRuns).map(run => run.wallSeconds as number);
  const minimum = learnedMinimumRuns();
  return seconds.length >= minimum
    ? { p75Seconds: quantile(seconds, 0.75), sampleCount: seconds.length, basis: 'exact' }
    : { p75Seconds: null, sampleCount: seconds.length, basis: `uncalibrated: ${seconds.length} exact runs < ${minimum}` };
}

export function estimateCapacity(signature: TaskSignature, history: readonly CapacityRun[], cores: number, query: CapacityQuery = {}) {
  const valid = history.filter(run => Number.isFinite(run.p90Cores) && run.p90Cores !== null && run.p90Cores >= 0).sort((a, b) => a.at.localeCompare(b.at));
  const conservativeClass = signature.family === 'native client' ? 'native'
    : ['cargo build', 'cargo test', 'bun test', 'wisp render', 'firn rebuild'].includes(signature.family) ? 'heavy' : null;
  const unseenExpensive = conservativeClass !== null && signature.exact && !valid.some(run => run.signature.exact
    && run.signature.family === signature.family && run.signature.repo === signature.repo && run.signature.package === signature.package);
  const levels = [
    ['exact', (run: CapacityRun) => sameExact(signature, run)],
    ['owner-shape', (run: CapacityRun) => typeof query.owner === 'string' && typeof query.shape === 'string' && run.owner === query.owner && run.shape === query.shape],
    ['owner-prefix-shape-class', (run: CapacityRun) => typeof query.owner === 'string' && typeof query.shape === 'string' && typeof run.owner === 'string' && ownerPrefix(run.owner) === ownerPrefix(query.owner) && run.shape === query.shape && run.signature.class === signature.class],
    ['family-repo', (run: CapacityRun) => signature.repo !== null && run.signature.family === signature.family && run.signature.repo === signature.repo],
    ['family', (run: CapacityRun) => run.signature.family === pooledFamily(signature.family)],
  ] as const;
  const duration = estimateDuration(signature, history);
  for (const [matchLevel, matches] of levels) {
    if (unseenExpensive) break;
    const runs = valid.filter(matches).slice(-estimateRuns);
    if (runs.length < learnedMinimumRuns()) continue;
    const values = (key: keyof CapacityRun) => runs.map(run => run[key]).filter((value): value is number => typeof value === 'number' && Number.isFinite(value));
    const distribution = (key: keyof CapacityRun) => ({ p50: quantile(values(key), 0.5), p90: quantile(values(key), 0.9) });
    return {
      signature, class: signature.class, sampleCount: runs.length, matchLevel,
      confidence: matchLevel === 'exact' && runs.length >= DEFAULTS["capacity_high_confidence_runs"] ? 'high' : 'medium',
      cores: { p50: quantile(values('meanCores'), 0.5), p90: quantile(values('p90Cores'), 0.9) },
      peakMemoryMiB: distribution('peakMemoryMiB'), ioReadBytes: distribution('ioReadBytes'),
      ioWriteBytes: distribution('ioWriteBytes'), diskBusyShare: distribution('diskBusyShare'),
      diskBusyBasis: 'host maximum physical whole-disk busy fraction',
      ioSampleCount: values('ioReadBytes').length, duration,
    };
  }
  const kind = conservativeClass ?? signature.class;
  const declared = resourceClass(kind, cores);
  return {
    signature, class: kind, sampleCount: 0, matchLevel: 'default', confidence: 'low',
    cores: { p50: declared.cpus, p90: declared.cpus },
    peakMemoryMiB: { p50: declared.memoryMiB, p90: declared.memoryMiB },
    ioReadBytes: { p50: null, p90: null }, ioWriteBytes: { p50: null, p90: null },
    diskBusyShare: { p50: null, p90: null }, diskBusyBasis: 'host maximum physical whole-disk busy fraction', ioSampleCount: 0, duration,
  };
}

export function diskBusyShare(before: Readonly<Record<string, number>>, after: Readonly<Record<string, number>>, wallMilliseconds: number): number | null {
  const deltas = Object.entries(after).flatMap(([disk, ticks]) =>
    before[disk] !== undefined && ticks >= before[disk] ? [ticks - before[disk]] : []);
  return wallMilliseconds > 0 && deltas.length > 0 ? Math.min(1, Math.max(...deltas) / wallMilliseconds) : null;
}

export const scopeCpuQuota = (kind: string, chargedCpus: number, declaredCpus: number | null): number | null => kind === 'native' ? null : declaredCpus ?? chargedCpus;
export function runningCpuCommitment(kind: string, measuredCores: number | null, estimateP50: number | null, reserved: number, quota: number | null) {
  const measured = measuredCores !== null && Number.isFinite(measuredCores) && measuredCores >= 0 ? measuredCores : null;
  const predicted = estimateP50 !== null && Number.isFinite(estimateP50) && estimateP50 >= 0 ? estimateP50 : reserved;
  const cpus = kind === 'native' && measured !== null ? measured * 1.25 : kind === 'heavy' ? Math.max((measured ?? predicted) * 1.25, reserved) : Math.max(measured ?? 0, predicted) * 1.25;
  return quota !== null && cpus > quota ? { cpus: quota, basis: 'scope-quota' }
    : { cpus, basis: measured === null ? 'estimate-missing-measurement' : kind === 'native' ? 'native-measured' : 'measured-or-estimate' };
}

export const sessionProviders = new Set(['claude', 'codex']);

export function sessionFromArgs(args: readonly string[]): string | null {
  for (let index = 0; index < args.length - 1; index += 1) {
    if (['--session-id', '--resume', 'resume'].includes(args[index]) && /^[0-9a-f-]{32,36}$/.test(args[index + 1])) return args[index + 1];
  }
  return null;
}
