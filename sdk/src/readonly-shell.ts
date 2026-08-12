import {
  spawn, spawnSync, type ChildProcessByStdio, type SpawnSyncReturns,
} from "node:child_process";
import {
  accessSync, constants, realpathSync, statSync,
} from "node:fs";
import { homedir } from "node:os";
import { delimiter, isAbsolute, join, resolve } from "node:path";
import type { Readable } from "node:stream";

const DEFAULT_TIMEOUT_MS = 30_000;
const MAX_TIMEOUT_MS = 120_000;
const MAX_OUTPUT_BYTES = 1_048_576;
const PROCESS_GROUP_REAP_MS = 1_500;
const PROCESS_GROUP_POLL_MS = 20;
export const READONLY_SHELL_SUMMARY_MAX_BYTES = 16 * 1024;
const SUMMARY_MARKER_RESERVE_BYTES = 256;
const SUMMARY_CONTENT_BYTES = READONLY_SHELL_SUMMARY_MAX_BYTES - SUMMARY_MARKER_RESERVE_BYTES;
const SUMMARY_HEAD_BYTES = Math.floor(SUMMARY_CONTENT_BYTES / 2);
const SUMMARY_TAIL_BYTES = SUMMARY_CONTENT_BYTES - SUMMARY_HEAD_BYTES;
export const MAX_READONLY_COMMAND_BYTES = 64 * 1024;
export const READONLY_SHELL_SERVER = "north-readonly-shell";
export const READONLY_SHELL_TOOL = `mcp__${READONLY_SHELL_SERVER}__run`;

export interface ReadonlyShellPrerequisites {
  bwrap: string;
  bash: string;
  cwd: string;
  home: string;
  path: string;
  lang: string;
  northBin?: string;
}

export interface ReadonlyShellResult {
  ok: boolean;
  exitCode: number | null;
  signal: NodeJS.Signals | null;
  timedOut: boolean;
  cancelled: boolean;
  outputLimitExceeded: boolean;
  stdout: string;
  stderr: string;
  stdoutBytes: ReadonlyShellOutputBytes;
  stderrBytes: ReadonlyShellOutputBytes;
}

export interface ReadonlyShellOutputBytes {
  totalBytes: number;
  capturedBytes: number;
  omittedBytes: number;
}

export class ReadonlyShellUnavailableError extends Error {
  readonly code = "readonly_shell_preflight_failed";
  constructor(message: string, options?: ErrorOptions) {
    super(message, options);
    this.name = "ReadonlyShellUnavailableError";
  }
}

const BPF_LD_W_ABS = 0x20;
const BPF_JMP_JEQ_K = 0x15;
const BPF_JMP_JGE_K = 0x35;
const BPF_RET_K = 0x06;
const SECCOMP_RET_KILL_PROCESS = 0x80000000;
const SECCOMP_RET_ERRNO_EPERM = 0x00050001;
const SECCOMP_RET_ALLOW = 0x7fff0000;

interface SeccompArchitecture {
  auditArch: number;
  socketSyscall: number;
  ioUringSetupSyscall: number;
  rejectsX32: boolean;
}

function seccompArchitecture(): SeccompArchitecture {
  if (process.platform !== "linux")
    throw new ReadonlyShellUnavailableError("readonly_shell_seccomp_requires_linux");
  if (process.arch === "x64")
    return {
      auditArch: 0xc000003e, socketSyscall: 41, ioUringSetupSyscall: 425, rejectsX32: true,
    };
  if (process.arch === "arm64")
    return {
      auditArch: 0xc00000b7, socketSyscall: 198, ioUringSetupSyscall: 425, rejectsX32: false,
    };
  throw new ReadonlyShellUnavailableError(
    `readonly_shell_seccomp_unsupported_architecture:${process.arch}`,
  );
}

/**
 * Classic BPF consumed directly by bubblewrap's --seccomp FD. Denying socket(2)
 * and io_uring_setup(2) is the load-bearing boundary: a read-only bind can still
 * contain mutable host Unix sockets, and IORING_OP_SOCKET otherwise bypasses a
 * filter that covers only the traditional syscall.
 */
export function readonlyShellSeccompProgram(): Buffer {
  const {
    auditArch, socketSyscall, ioUringSetupSyscall, rejectsX32,
  } = seccompArchitecture();
  const instructions: ReadonlyArray<readonly [number, number, number, number]> = [
    [BPF_LD_W_ABS, 0, 0, 4],                    // seccomp_data.arch
    [BPF_JMP_JEQ_K, 1, 0, auditArch],           // reject an unexpected ABI
    [BPF_RET_K, 0, 0, SECCOMP_RET_KILL_PROCESS],
    [BPF_LD_W_ABS, 0, 0, 0],                    // seccomp_data.nr
    ...(rejectsX32
      ? [
        // AUDIT_ARCH_X86_64 also covers x32. Its syscall bit would turn
        // socket into 0x40000029 and evade a native-number equality check.
        [BPF_JMP_JGE_K, 0, 1, 0x40000000] as const,
        [BPF_RET_K, 0, 0, SECCOMP_RET_KILL_PROCESS] as const,
      ]
      : []),
    [BPF_JMP_JEQ_K, 1, 0, socketSyscall],
    [BPF_JMP_JEQ_K, 0, 1, ioUringSetupSyscall],
    [BPF_RET_K, 0, 0, SECCOMP_RET_ERRNO_EPERM],
    [BPF_RET_K, 0, 0, SECCOMP_RET_ALLOW],
  ] as const;
  const program = Buffer.alloc(instructions.length * 8);
  instructions.forEach(([code, jt, jf, value], index) => {
    const offset = index * 8;
    program.writeUInt16LE(code, offset);
    program.writeUInt8(jt, offset + 2);
    program.writeUInt8(jf, offset + 3);
    program.writeUInt32LE(value >>> 0, offset + 4);
  });
  return program;
}

function encodedSocketDenyFilter(): string {
  return [...readonlyShellSeccompProgram()]
    .map((byte) => `\\x${byte.toString(16).padStart(2, "0")}`)
    .join("");
}

function executable(candidate: string): string | undefined {
  try {
    const path = realpathSync(candidate);
    accessSync(path, constants.X_OK);
    return path;
  } catch {
    return undefined;
  }
}

function resolveExecutable(name: string, override: string | undefined, path: string): string {
  const requested = override?.trim() || name;
  const direct = isAbsolute(requested) || requested.includes("/")
    ? executable(resolve(requested))
    : undefined;
  if (direct) return direct;
  if (!isAbsolute(requested) && !requested.includes("/")) {
    for (const directory of path.split(delimiter).filter(Boolean)) {
      const found = executable(join(directory, requested));
      if (found) return found;
    }
  }
  throw new ReadonlyShellUnavailableError(`${name}_executable_unavailable`);
}

function canonicalSandboxPath(path: string): string {
  return [...new Set(path.split(delimiter).filter(Boolean).flatMap((directory) => {
    try {
      const canonical = realpathSync(directory);
      return statSync(canonical).isDirectory() ? [canonical] : [];
    } catch {
      return [];
    }
  }))].join(delimiter);
}

function sandboxArguments(prerequisites: ReadonlyShellPrerequisites, seccompFd: number): string[] {
  return [
    "--die-with-parent",
    "--unshare-all",
    "--unshare-user",
    "--disable-userns",
    "--assert-userns-disabled",
    "--new-session",
    "--ro-bind", "/", "/",
    "--dev", "/dev",
    "--proc", "/proc",
    // Defense in depth for the common D-Bus/Podman sockets. The seccomp rule is
    // still required because sockets can live anywhere in the read-only tree.
    "--tmpfs", "/run",
    "--tmpfs", "/tmp",
    "--dir", "/tmp/north-home",
    "--clearenv",
    // Keep canonical ~/ paths usable for repo/global instructions. The root
    // bind remains read-only; only cache/state locations below are ephemeral.
    "--setenv", "HOME", prerequisites.home,
    "--setenv", "TMPDIR", "/tmp",
    "--setenv", "XDG_CACHE_HOME", "/tmp/north-home/.cache",
    "--setenv", "XDG_CONFIG_HOME", "/tmp/north-home/.config",
    "--setenv", "XDG_DATA_HOME", "/tmp/north-home/.local/share",
    "--setenv", "PATH", prerequisites.path,
    "--setenv", "LANG", prerequisites.lang,
    ...(prerequisites.northBin
      ? ["--setenv", "NORTH_BIN", prerequisites.northBin]
      : []),
    "--chdir", prerequisites.cwd,
    "--seccomp", String(seccompFd),
  ];
}

function seccompLaunchArguments(
  prerequisites: ReadonlyShellPrerequisites,
  command: readonly string[],
): string[] {
  // Never pass a numeric fd through the long-lived Bun host: Bun can close it
  // asynchronously after the number has been reused for an unrelated pipe.
  // The immutable Bash path materializes the fixed BPF through a private pipe
  // and opens fd 3 immediately before exec, confining descriptor ownership to
  // the short-lived sandbox process tree. The canonical hex form has no shell
  // metacharacters and the constant `%b` format is not caller-controlled.
  return [
    "--noprofile", "--norc", "-c",
    'set -eu; exec 3< <(printf "%b" "$1"); shift; exec "$@"',
    "north-readonly-seccomp",
    encodedSocketDenyFilter(),
    prerequisites.bwrap,
    ...sandboxArguments(prerequisites, 3),
    ...command,
  ];
}

/**
 * Prove the provider adapter can supply a read-only shell before a model turn is
 * accepted. The checkout is a read-only bind, the only writable mount is an
 * ephemeral /tmp, and --unshare-all gives the command no network namespace.
 */
export function preflightReadonlyShell(
  cwd: string,
  environment: NodeJS.ProcessEnv = process.env,
): ReadonlyShellPrerequisites {
  let canonicalCwd: string;
  try {
    canonicalCwd = realpathSync(resolve(cwd));
    if (!statSync(canonicalCwd).isDirectory()) throw new Error("not a directory");
  } catch (cause) {
    throw new ReadonlyShellUnavailableError("readonly_shell_cwd_unavailable", { cause });
  }
  if (canonicalCwd === "/tmp" || canonicalCwd.startsWith("/tmp/")) {
    throw new ReadonlyShellUnavailableError("readonly_shell_cwd_hidden_by_ephemeral_tmp");
  }
  const path = environment.PATH ?? "/usr/bin:/bin";
  const sandboxPath = canonicalSandboxPath(path);
  if (!sandboxPath)
    throw new ReadonlyShellUnavailableError("readonly_shell_executable_path_unavailable");
  let home: string;
  try {
    home = realpathSync(resolve(environment.HOME ?? homedir()));
    if (!statSync(home).isDirectory()) throw new Error("not a directory");
  } catch (cause) {
    throw new ReadonlyShellUnavailableError("readonly_shell_home_unavailable", { cause });
  }
  if (home === "/tmp" || home.startsWith("/tmp/"))
    throw new ReadonlyShellUnavailableError("readonly_shell_home_hidden_by_ephemeral_tmp");
  const prerequisites = {
    bwrap: resolveExecutable("bwrap", environment.NORTH_BWRAP_BIN, path),
    bash: resolveExecutable("bash", environment.NORTH_BASH_BIN, path),
    cwd: canonicalCwd,
    home,
    path: sandboxPath,
    lang: environment.LANG ?? "C.UTF-8",
    ...(environment.NORTH_BIN ? { northBin: environment.NORTH_BIN } : {}),
  };
  const probeName = `.north-readonly-preflight-${process.pid}`;
  let probe: SpawnSyncReturns<string>;
  probe = spawnSync(prerequisites.bash, seccompLaunchArguments(prerequisites, [
    prerequisites.bash, "--noprofile", "--norc", "-lc",
    `if ( : > ${JSON.stringify(probeName)} ) 2>/dev/null; then rm -f -- ${JSON.stringify(probeName)}; exit 41; fi; p=$(mktemp /tmp/north-shell.XXXXXX) && test -f "$p" && rm -f -- "$p"`,
  ]), {
    encoding: "utf8",
    timeout: 3_000,
    maxBuffer: 64 * 1024,
    stdio: ["ignore", "pipe", "pipe"],
  });
  if (probe.error || probe.status !== 0) {
    throw new ReadonlyShellUnavailableError(
      probe.status === 41
        ? "readonly_shell_checkout_is_writable"
        : `readonly_shell_sandbox_unavailable${probe.stderr?.trim() ? `: ${probe.stderr.trim()}` : ""}`,
      probe.error ? { cause: probe.error } : undefined,
    );
  }
  return prerequisites;
}

function trailingUtf8Boundary(buffer: Buffer): number {
  if (buffer.length === 0) return 0;
  let sequenceStart = buffer.length - 1;
  while (sequenceStart >= 0 && (buffer[sequenceStart] & 0xc0) === 0x80)
    sequenceStart--;
  if (sequenceStart < 0) return 0;
  const lead = buffer[sequenceStart];
  const expected = lead <= 0x7f ? 1
    : lead >= 0xc2 && lead <= 0xdf ? 2
      : lead >= 0xe0 && lead <= 0xef ? 3
        : lead >= 0xf0 && lead <= 0xf4 ? 4 : 1;
  return buffer.length - sequenceStart < expected ? sequenceStart : buffer.length;
}

function leadingUtf8Boundary(buffer: Buffer): number {
  let offset = 0;
  while (offset < buffer.length && (buffer[offset] & 0xc0) === 0x80) offset++;
  return offset;
}

interface StreamSummary {
  text: string;
  bytes: ReadonlyShellOutputBytes;
}

class StreamSummaryCollector {
  #totalBytes = 0;
  #head = Buffer.alloc(0);
  #tail = Buffer.alloc(0);

  push(chunk: Buffer): void {
    this.#totalBytes += chunk.length;
    if (this.#head.length < SUMMARY_CONTENT_BYTES) {
      const remaining = SUMMARY_CONTENT_BYTES - this.#head.length;
      this.#head = Buffer.concat([
        this.#head,
        Buffer.from(chunk.subarray(0, remaining)),
      ]);
    }
    if (chunk.length >= SUMMARY_TAIL_BYTES) {
      this.#tail = Buffer.from(chunk.subarray(chunk.length - SUMMARY_TAIL_BYTES));
      return;
    }
    const combined = Buffer.concat([this.#tail, chunk]);
    this.#tail = combined.length <= SUMMARY_TAIL_BYTES
      ? combined
      : Buffer.from(combined.subarray(combined.length - SUMMARY_TAIL_BYTES));
  }

  finish(): StreamSummary {
    if (this.#totalBytes <= SUMMARY_CONTENT_BYTES) {
      return {
        text: this.#head.toString("utf8"),
        bytes: {
          totalBytes: this.#totalBytes,
          capturedBytes: this.#totalBytes,
          omittedBytes: 0,
        },
      };
    }
    const rawHead = this.#head.subarray(0, SUMMARY_HEAD_BYTES);
    const head = rawHead.subarray(0, trailingUtf8Boundary(rawHead));
    const tail = this.#tail.subarray(leadingUtf8Boundary(this.#tail));
    const capturedBytes = head.length + tail.length;
    const omittedBytes = this.#totalBytes - capturedBytes;
    const marker = `\n[north: ${omittedBytes} bytes omitted; captured ${capturedBytes} of ${this.#totalBytes} bytes]\n`;
    return {
      text: `${head.toString("utf8")}${marker}${tail.toString("utf8")}`,
      bytes: { totalBytes: this.#totalBytes, capturedBytes, omittedBytes },
    };
  }
}

function cancelledResult(): ReadonlyShellResult {
  const bytes = { totalBytes: 0, capturedBytes: 0, omittedBytes: 0 };
  return {
    ok: false,
    exitCode: null,
    signal: null,
    timedOut: false,
    cancelled: true,
    outputLimitExceeded: false,
    stdout: "",
    stderr: "",
    stdoutBytes: bytes,
    stderrBytes: { ...bytes },
  };
}

function systemCode(error: unknown): string | undefined {
  return typeof error === "object" && error !== null && "code" in error
    ? String((error as { code?: unknown }).code)
    : undefined;
}

function processGroupExists(pgid: number): boolean {
  try {
    process.kill(-pgid, 0);
    return true;
  } catch (error) {
    return systemCode(error) !== "ESRCH";
  }
}

async function waitForProcessGroupGone(pgid: number): Promise<boolean> {
  const deadline = Date.now() + PROCESS_GROUP_REAP_MS;
  while (processGroupExists(pgid)) {
    const remaining = deadline - Date.now();
    if (remaining <= 0) return false;
    await Bun.sleep(Math.min(PROCESS_GROUP_POLL_MS, remaining));
  }
  return true;
}

export async function runReadonlyShell(
  command: string,
  cwd: string,
  timeoutMs = DEFAULT_TIMEOUT_MS,
  environment: NodeJS.ProcessEnv = process.env,
  abortSignal?: AbortSignal,
): Promise<ReadonlyShellResult> {
  if (!command.trim()) throw new Error("readonly shell command must be nonblank");
  if (Buffer.byteLength(command, "utf8") > MAX_READONLY_COMMAND_BYTES)
    throw new Error("readonly shell command exceeds 65536 UTF-8 bytes");
  if (!Number.isFinite(timeoutMs))
    throw new Error("readonly shell timeout must be finite");
  const boundedTimeout = Math.max(100, Math.min(MAX_TIMEOUT_MS, Math.trunc(timeoutMs)));
  if (abortSignal?.aborted) return cancelledResult();
  const prerequisites = preflightReadonlyShell(cwd, environment);
  if (abortSignal?.aborted) return cancelledResult();
  let child: ChildProcessByStdio<null, Readable, Readable>;
  try {
    child = spawn(prerequisites.bash, seccompLaunchArguments(prerequisites, [
      prerequisites.bash, "--noprofile", "--norc", "-lc", command,
    ]), {
      // A separate process group gives timeout/output enforcement one kill target
      // for bwrap and every descendant. The PID namespace also collapses when its
      // init dies, but the host-side group kill is the explicit backstop.
      detached: true,
      stdio: ["ignore", "pipe", "pipe"],
    }) as ChildProcessByStdio<null, Readable, Readable>;
  } catch (cause) {
    throw new ReadonlyShellUnavailableError("readonly_shell_process_unavailable", { cause });
  }
  // Node reports some launch failures asynchronously. Observe both terminal
  // paths before validating ownership so even an invalid/missing PID cannot
  // leave an unhandled child-process error behind.
  const terminalPromise = new Promise<{
    exitCode: number | null;
    signal: NodeJS.Signals | null;
    spawnError?: Error;
  }>((resolveTerminal) => {
    child.once("error", (spawnError) => resolveTerminal({
      exitCode: null, signal: null, spawnError,
    }));
    child.once("close", (exitCode, signal) => resolveTerminal({ exitCode, signal }));
  });
  const pgid = child.pid;
  if (!Number.isSafeInteger(pgid) || pgid === undefined || pgid <= 1
      || pgid === process.pid || pgid === process.ppid) {
    try { child.kill("SIGKILL"); } catch { /* invalid process ownership */ }
    throw new ReadonlyShellUnavailableError("readonly_shell_process_group_invalid");
  }
  const stdout = new StreamSummaryCollector();
  const stderr = new StreamSummaryCollector();
  let totalOutputBytes = 0;
  let timedOut = false;
  let cancelled = false;
  let outputLimitExceeded = false;
  const terminate = () => {
    try {
      process.kill(-pgid, "SIGKILL");
    } catch (error) {
      if (systemCode(error) !== "ESRCH") {
        try { child.kill("SIGKILL"); } catch { /* reap proof remains authoritative */ }
      }
    }
  };
  const onAbort = () => {
    cancelled = true;
    terminate();
  };
  const timer = setTimeout(() => {
    timedOut = true;
    terminate();
  }, boundedTimeout);
  child.stdout.on("data", (value: Buffer) => {
    stdout.push(value);
    totalOutputBytes += value.length;
    if (totalOutputBytes > MAX_OUTPUT_BYTES && !outputLimitExceeded) {
      outputLimitExceeded = true;
      terminate();
    }
  });
  child.stderr.on("data", (value: Buffer) => {
    stderr.push(value);
    totalOutputBytes += value.length;
    if (totalOutputBytes > MAX_OUTPUT_BYTES && !outputLimitExceeded) {
      outputLimitExceeded = true;
      terminate();
    }
  });
  child.once("exit", () => {
    if (processGroupExists(pgid)) terminate();
  });
  if (abortSignal) {
    abortSignal.addEventListener("abort", onAbort, { once: true });
    if (abortSignal.aborted) onAbort();
  }
  const terminal = await terminalPromise;
  clearTimeout(timer);
  if (abortSignal) abortSignal.removeEventListener("abort", onAbort);
  if (processGroupExists(pgid)) terminate();
  if (!await waitForProcessGroupGone(pgid))
    throw new ReadonlyShellUnavailableError("readonly_shell_process_group_reap_failed");
  if (terminal.spawnError) {
    throw new ReadonlyShellUnavailableError("readonly_shell_process_unavailable", {
      cause: terminal.spawnError,
    });
  }
  const stdoutSummary = stdout.finish();
  const stderrSummary = stderr.finish();
  return {
    ok: terminal.exitCode === 0 && !timedOut && !cancelled && !outputLimitExceeded,
    exitCode: terminal.exitCode,
    signal: terminal.signal,
    timedOut,
    cancelled,
    outputLimitExceeded,
    stdout: stdoutSummary.text,
    stderr: stderrSummary.text,
    stdoutBytes: stdoutSummary.bytes,
    stderrBytes: stderrSummary.bytes,
  };
}
