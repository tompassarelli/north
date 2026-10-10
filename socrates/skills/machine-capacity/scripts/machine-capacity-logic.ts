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

export function memoryFloorMiB(profile: string, memoryTotalMiB: number): number {
  return Math.max(profile === 'attended' ? memoryTotalMiB / 5 : memoryTotalMiB / 12, 8192);
}

export function resourceClass(kind: string, cores: number): Resources {
  switch (kind) {
    case 'agent': return { cpus: 0, memoryMiB: 768 };
    case 'native': return { cpus: cores > 1 ? 2 : 1, memoryMiB: 4096 };
    case 'moderate': return { cpus: cores > 1 ? 2 : 1, memoryMiB: 2048 };
    case 'heavy': return { cpus: cores > 8 ? 6 : cores > 4 ? 3 : 1, memoryMiB: 8192 };
    case 'gpu': return { cpus: 1, memoryMiB: 2048 };
    case 'critical': return { cpus: cores - reservedCpus('attended', cores), memoryMiB: 16384 };
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
  meanCores: number;
  p90Cores: number | null;
  peakMemoryMiB: number;
  ioReadBytes: number | null;
  ioWriteBytes: number | null;
  ioPressureSomeUsec: number | null;
  diskBusyShare: number | null;
}

export const estimateRuns = 20;

function quantile(values: readonly number[], share: number): number | null {
  if (values.length === 0) return null;
  const sorted = [...values].sort((a, b) => a - b);
  return sorted[Math.ceil(sorted.length * share) - 1];
}

export function estimateCapacity(signature: TaskSignature, history: readonly CapacityRun[], cores: number) {
  const valid = history.filter(run => Number.isFinite(run.p90Cores) && run.p90Cores !== null && run.p90Cores >= 0)
    .sort((a, b) => a.at.localeCompare(b.at));
  const levels = [
    ['exact', (other: TaskSignature) => signature.exact && other.exact && other.family === signature.family && other.repo === signature.repo && other.package === signature.package && other.class === signature.class],
    ['family-repo', (other: TaskSignature) => signature.repo !== null && other.family === signature.family && other.repo === signature.repo],
    ['family', (other: TaskSignature) => other.family === signature.family],
  ] as const;
  for (const [matchLevel, matches] of levels) {
    const runs = valid.filter(run => matches(run.signature)).slice(-estimateRuns);
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
  const declared = resourceClass(signature.class, cores);
  return {
    signature, class: signature.class, sampleCount: 0, matchLevel: 'default', confidence: 'low',
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
