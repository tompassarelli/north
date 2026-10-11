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

export function memoryFloorMiB(profile: string, memoryTotalMiB: number): number {
  if (memoryTotalMiB < 32768) return profile === 'attended' ? memoryTotalMiB / 5 : memoryTotalMiB / 12;
  return Math.min(Math.max(profile === 'attended' ? memoryTotalMiB / 5 : memoryTotalMiB / 12, 8192), memoryTotalMiB / 4);
}

export function resourceClass(kind: string, cores: number): Resources {
  switch (kind) {
    case 'agent': return { cpus: 0, memoryMiB: 768 };
    case 'native': return { cpus: cores > 1 ? 2 : 1, memoryMiB: 4096 };
    case 'moderate': return { cpus: cores > 1 ? 2 : 1, memoryMiB: 2048 };
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
  return ['moderate', 'heavy', 'exclusive', 'gpu', 'critical'].includes(kind);
}

export function maximumSeconds(kind: string): number {
  return kind === 'exclusive' ? 900 : 3600;
}

export function sessionSeconds(kind: string): number {
  return kind === 'exclusive' ? 900 : batchClass(kind) ? 1800 : 0;
}

export function batchPressureLimitBasisPoints(): number {
  return 3000;
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

export function gpuSlots(): number {
  return 2;
}

export function nativeGpuDecision(profile: string, gpuBusyPercent: number, gpuClients: number, gpuRuns: number): string {
  if (profile === 'attended' && gpuClients >= 4) return 'DEFER_GPU_CLIENTS';
  if (gpuBusyPercent >= gpuBusyLimitPercent() && gpuRuns === 0) return 'DEFER_GPU_BUSY';
  return 'RUN';
}

export function admissionDecision(
  profile: string,
  kind: string,
  cores: number,
  memoryTotalMiB: number,
  memoryAvailableMiB: number,
  cpuSomeAvg10BasisPoints: number,
  protectedCpuSomeAvg10BasisPoints: number,
  leasedMemoryMiB: number,
  leasedBatchCpus: number,
  leasedNativeCpus: number,
  requestedCpus: number,
  requestedMemoryMiB: number,
  peerBatchRuns: number,
  peerExclusiveRuns: number,
  unboundedRuns: number,
  queuedAhead: number,
  exclusiveWaiting: number,
  gpuBusyPercent: number,
  gpuClients: number,
  gpuRuns: number,
  nativeWaiting: number,
): string {
  if (profile !== 'attended' && profile !== 'unattended') throw new Error('Unknown capacity profile');
  if (cores <= 0 || memoryTotalMiB <= 0 || memoryAvailableMiB <= 0 || requestedCpus < 0 || requestedMemoryMiB <= 0) return 'INVALID';
  if (memoryAvailableMiB - requestedMemoryMiB < memoryFloorMiB(profile, memoryTotalMiB)) return 'DEFER_MEMORY_HEADROOM';
  if ((leasedMemoryMiB + requestedMemoryMiB) * 4 > memoryTotalMiB * 3) return 'DEFER_MEMORY_CAPACITY';
  if (kind === 'agent') return 'RUN';
  if (kind === 'native') {
    if (leasedNativeCpus + requestedCpus > aggregateCpus(profile, cores)) return 'DEFER_NATIVE_CPUS';
    return nativeGpuDecision(profile, gpuBusyPercent, gpuClients, gpuRuns);
  }
  if (unboundedRuns > 0) return 'DEFER_UNBOUNDED_PEER';
  if (peerExclusiveRuns > 0) return 'DEFER_EXCLUSIVE';
  if (kind === 'critical') {
    return profile === 'attended' && protectedCpuSomeAvg10BasisPoints >= 1000 ? 'DEFER_INTERACTIVE_PRESSURE' : 'RUN';
  }
  if (kind === 'gpu' && nativeWaiting > 0) return 'DEFER_NATIVE_WAITING';
  if (kind === 'gpu' && gpuRuns >= gpuSlots()) return 'DEFER_GPU_SLOTS';
  if (kind === 'exclusive') {
    if (exclusiveWaiting > 0) return 'DEFER_QUEUED';
    return peerBatchRuns > 0 ? 'DEFER_EXCLUSIVE' : 'RUN';
  }
  if (exclusiveWaiting > 0 && kind === 'heavy') return 'DEFER_EXCLUSIVE_QUEUED';
  if (profile === 'attended' && protectedCpuSomeAvg10BasisPoints >= 1000) return 'DEFER_INTERACTIVE_PRESSURE';
  if (queuedAhead > 0) return 'DEFER_QUEUED';
  if (leasedBatchCpus + requestedCpus > aggregateCpus(profile, cores)) return 'DEFER_CPU_CAPACITY';
  if (cpuSomeAvg10BasisPoints > batchPressureLimitBasisPoints()) return 'DEFER_CPU_PRESSURE';
  return 'RUN';
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
  const cpus = p90(runCores);
  return { cpus: Math.max(1, Math.min(cpus, declared.cpus * 1.5)), memoryMiB: p90(runMemoryMiB) * 1.2 };
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
}

export const estimateRuns = 20;

export const ownerPrefix = (owner: string): string => owner.match(/^[A-Za-z]*/)![0];

export interface CapacityQuery {
  owner?: string | null;
  shape?: string | null;
}

function quantile(values: readonly number[], share: number): number | null {
  if (values.length === 0) return null;
  const sorted = [...values].sort((a, b) => a - b);
  return sorted[Math.ceil(sorted.length * share) - 1];
}

export function estimateCapacity(signature: TaskSignature, history: readonly CapacityRun[], cores: number, query: CapacityQuery = {}) {
  const valid = history.filter(run => Number.isFinite(run.p90Cores) && run.p90Cores !== null && run.p90Cores >= 0)
    .sort((a, b) => a.at.localeCompare(b.at));
  const conservativeClass = signature.family === 'native client' ? 'native'
    : ['cargo build', 'cargo test', 'bun test', 'wisp render', 'firn rebuild'].includes(signature.family) ? 'heavy' : null;
  const unseenExpensive = conservativeClass !== null && signature.exact && !valid.some(run => run.signature.exact
    && run.signature.family === signature.family && run.signature.repo === signature.repo && run.signature.package === signature.package);
  const levels = [
    ['exact', (run: CapacityRun) => signature.exact && run.signature.exact && run.signature.family === signature.family && run.signature.repo === signature.repo && run.signature.package === signature.package && run.signature.class === signature.class],
    ['owner-shape', (run: CapacityRun) => typeof query.owner === 'string' && typeof query.shape === 'string' && run.owner === query.owner && run.shape === query.shape],
    ['owner-prefix-shape-class', (run: CapacityRun) => typeof query.owner === 'string' && typeof query.shape === 'string' && typeof run.owner === 'string' && ownerPrefix(run.owner) === ownerPrefix(query.owner) && run.shape === query.shape && run.signature.class === signature.class],
    ['family-repo', (run: CapacityRun) => signature.repo !== null && run.signature.family === signature.family && run.signature.repo === signature.repo],
    ['family', (run: CapacityRun) => run.signature.family === signature.family],
  ] as const;
  for (const [matchLevel, matches] of levels) {
    if (unseenExpensive) break;
    const runs = valid.filter(matches).slice(-estimateRuns);
    if (runs.length < learnedMinimumRuns()) continue;
    const values = (key: keyof CapacityRun) => runs.map(run => run[key]).filter((value): value is number => typeof value === 'number' && Number.isFinite(value));
    const distribution = (key: keyof CapacityRun) => ({ p50: quantile(values(key), 0.5), p90: quantile(values(key), 0.9) });
    return {
      signature, class: signature.class, sampleCount: runs.length, matchLevel,
      confidence: matchLevel === 'exact' && runs.length >= 10 ? 'high' : 'medium',
      cores: { p50: quantile(values('meanCores'), 0.5), p90: quantile(values('p90Cores'), 0.9) },
      peakMemoryMiB: distribution('peakMemoryMiB'), ioReadBytes: distribution('ioReadBytes'),
      ioWriteBytes: distribution('ioWriteBytes'), diskBusyShare: distribution('diskBusyShare'),
      diskBusyBasis: 'host maximum physical whole-disk busy fraction',
      ioSampleCount: values('ioReadBytes').length,
    };
  }
  const kind = conservativeClass ?? signature.class;
  const declared = resourceClass(kind, cores);
  return {
    signature, class: kind, sampleCount: 0, matchLevel: 'default', confidence: 'low',
    cores: { p50: declared.cpus, p90: declared.cpus },
    peakMemoryMiB: { p50: declared.memoryMiB, p90: declared.memoryMiB },
    ioReadBytes: { p50: null, p90: null }, ioWriteBytes: { p50: null, p90: null },
    diskBusyShare: { p50: null, p90: null }, diskBusyBasis: 'host maximum physical whole-disk busy fraction', ioSampleCount: 0,
  };
}

export function diskBusyShare(before: Readonly<Record<string, number>>, after: Readonly<Record<string, number>>, wallMilliseconds: number): number | null {
  const deltas = Object.entries(after).flatMap(([disk, ticks]) =>
    before[disk] !== undefined && ticks >= before[disk] ? [ticks - before[disk]] : []);
  return wallMilliseconds > 0 && deltas.length > 0 ? Math.min(1, Math.max(...deltas) / wallMilliseconds) : null;
}

export function runningCpuCommitment(kind: string, measuredCores: number | null, estimateP50: number | null, reserved: number) {
  const measured = measuredCores !== null && Number.isFinite(measuredCores) && measuredCores >= 0 ? measuredCores : null;
  const predicted = estimateP50 !== null && Number.isFinite(estimateP50) && estimateP50 >= 0 ? estimateP50 : reserved;
  return {
    cpus: (kind === 'native' && measured !== null ? measured : Math.max(measured ?? 0, predicted)) * 1.25,
    basis: measured === null ? 'estimate-missing-measurement' : kind === 'native' ? 'native-measured' : 'measured-or-estimate',
  };
}

export interface ProcessEntry {
  ppid: number;
  start: string;
  ticks: number;
  childTicks: number;
  comm: string;
  cgroup: string | null;
  args?: readonly string[];
  leaseHost?: boolean;
}

export interface ProcessSample {
  at: number;
  processes: Readonly<Record<string, ProcessEntry>>;
}

export interface OrgNode {
  id: string;
  session?: string | null;
  pid?: number | null;
}

export interface SessionOwner {
  pid: number;
  provider: string | null;
  session: string | null;
  org: string | null;
}

export const sessionProviders = new Set(['claude', 'codex']);

export function sessionFromArgs(args: readonly string[]): string | null {
  for (let index = 0; index < args.length - 1; index += 1) {
    if (['--session-id', '--resume', 'resume'].includes(args[index]) && /^[0-9a-f-]{32,36}$/.test(args[index + 1])) return args[index + 1];
  }
  return null;
}

// Children reaped by a process add their CPU to its childTicks, so a tree that lived between two samples still counts.
export function attributeUnleased(older: ProcessSample, newer: ProcessSample, countedCgroups: ReadonlySet<string>, org: readonly OrgNode[]) {
  const seconds = (newer.at - older.at) / 1000;
  const total = (entry: ProcessEntry) => entry.ticks + (entry.leaseHost ? 0 : entry.childTicks);
  const counted = (entry: ProcessEntry | undefined) => entry !== undefined && entry.cgroup !== null && countedCgroups.has(entry.cgroup);
  const same = (pid: string) => older.processes[pid] !== undefined && newer.processes[pid]?.start === older.processes[pid].start;
  const ownerOf = (sample: ProcessSample, pid: number): SessionOwner => {
    const chain: number[] = [];
    let root = pid;
    for (let current: number | undefined = pid; current !== undefined && current > 0 && !chain.includes(current);) {
      const entry: ProcessEntry | undefined = sample.processes[current];
      if (entry === undefined) break;
      chain.push(current);
      if (sessionProviders.has(entry.comm)) {
        const session = sessionFromArgs(entry.args ?? []);
        const node = org.find(item => session !== null && item.session === session)
          ?? org.find(item => item.pid != null && (chain.includes(item.pid) || ancestors(sample, current!).includes(item.pid)));
        return { pid: current, provider: entry.comm, session: session ?? node?.session ?? null, org: node?.id ?? null };
      }
      if (entry.cgroup === sample.processes[pid].cgroup) root = current;
      current = entry.ppid;
    }
    return { pid: root, provider: null, session: null, org: null };
  };
  const owners = new Map<number, { owner: SessionOwner; ticks: number; byCgroup: Map<string, number>; byPid: Map<number, number> }>();
  const credit = (sample: ProcessSample, pid: number, ticks: number) => {
    const owner = ownerOf(sample, pid);
    const record = owners.get(owner.pid) ?? { owner, ticks: 0, byCgroup: new Map(), byPid: new Map() };
    const cgroup = sample.processes[pid].cgroup!;
    record.ticks += ticks;
    record.byCgroup.set(cgroup, (record.byCgroup.get(cgroup) ?? 0) + ticks);
    record.byPid.set(pid, (record.byPid.get(pid) ?? 0) + ticks);
    owners.set(owner.pid, record);
  };
  for (const [pid, entry] of Object.entries(newer.processes)) {
    if (!counted(entry)) continue;
    credit(newer, Number(pid), total(entry) - (same(pid) ? total(older.processes[pid]) : 0));
  }
  for (const [pid, entry] of Object.entries(older.processes)) {
    if (!counted(entry) || newer.processes[pid]?.start === entry.start) continue;
    const parent = String(entry.ppid);
    if (same(parent) && counted(newer.processes[parent]) && !newer.processes[parent].leaseHost) credit(newer, entry.ppid, -total(entry));
  }
  const cores = (ticks: number) => Math.round(Math.max(0, ticks) / 100 / seconds * 100) / 100;
  return [...owners.values()]
    .map(record => ({
      ...record.owner,
      cores: cores(record.ticks),
      cgroups: Object.fromEntries([...record.byCgroup].map(([cgroup, ticks]) => [cgroup, cores(ticks)])),
      topPid: [...record.byPid].sort((left, right) => right[1] - left[1])[0][0],
    }))
    .filter(record => record.cores > 0)
    .sort((left, right) => right.cores - left.cores);
}

function ancestors(sample: ProcessSample, pid: number): number[] {
  const seen: number[] = [];
  for (let current = sample.processes[pid]?.ppid; current !== undefined && current > 0 && !seen.includes(current); current = sample.processes[current]?.ppid) seen.push(current);
  return seen;
}
