import { afterEach, expect, test } from "bun:test";
import { EventEmitter } from "node:events";
import { mkdtempSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { PassThrough } from "node:stream";
import {
  createAnthropicProcessLifecycle, settleAnthropicProcessOwner,
} from "../src/providers/anthropic-process";
import {
  createAnthropicQuery, disposeAnthropicSdkQuery,
} from "../src/providers/anthropic";
import { HostTerminationCoordinator } from "../src/host-termination";
import { ManagedQueryTermination } from "../src/query-lifecycle";

const fixture = join(import.meta.dir, "fixtures", "anthropic-process-tree.mjs");
const temporary: string[] = [];
const groups = new Set<number>();
const directPids = new Set<number>();

function code(error: unknown): string | undefined {
  return typeof error === "object" && error !== null && "code" in error
    ? String((error as { code?: unknown }).code) : undefined;
}

function groupGone(pgid: number): boolean {
  try { process.kill(-pgid, 0); return false; }
  catch (error) { return code(error) === "ESRCH"; }
}

function pidGone(pid: number): boolean {
  try { process.kill(pid, 0); return false; }
  catch (error) { return code(error) === "ESRCH"; }
}

async function eventually(predicate: () => boolean, label: string, timeoutMs = 2_000): Promise<void> {
  const deadline = Date.now() + timeoutMs;
  while (!predicate()) {
    if (Date.now() >= deadline) throw new Error(`timed out waiting for ${label}`);
    await new Promise((resolve) => setTimeout(resolve, 10));
  }
}

async function pidRecord(path: string): Promise<{ leader: number; descendant: number; pgid: number }> {
  let value: { leader: number; descendant: number; pgid: number } | undefined;
  await eventually(() => {
    try { value = JSON.parse(readFileSync(path, "utf8")); return true; }
    catch { return false; }
  }, "process-tree readiness");
  return value!;
}

function startTree(mode: "hold" | "natural", ignoreTerm = false) {
  const dir = mkdtempSync(join(tmpdir(), "north-anthropic-pgid-"));
  temporary.push(dir);
  const path = join(dir, "pids.json");
  const lifecycle = createAnthropicProcessLifecycle({
    graceMs: 20, termMs: 100, killMs: 1_000,
  });
  const forwarded = new AbortController();
  const child = lifecycle.spawnClaudeCodeProcess!({
    command: process.execPath,
    args: [fixture, mode],
    cwd: dir,
    env: {
      ...process.env,
      NORTH_PID_FILE: path,
      NORTH_IGNORE_TERM: ignoreTerm ? "1" : "0",
    },
    signal: forwarded.signal,
  });
  return { lifecycle, child, forwarded, path };
}

afterEach(() => {
  for (const pgid of groups) {
    if (!groupGone(pgid)) {
      try { process.kill(-pgid, "SIGKILL"); } catch { /* already gone */ }
    }
  }
  groups.clear();
  for (const pid of directPids) {
    if (!pidGone(pid)) {
      try { process.kill(pid, "SIGKILL"); } catch { /* already gone */ }
    }
  }
  directPids.clear();
  for (const path of temporary.splice(0)) rmSync(path, { recursive: true, force: true });
});

const posixTest = process.platform === "win32" ? test.skip : test;

posixTest("settle reaps the CLI leader and a SIGTERM-resistant descendant", async () => {
  const { lifecycle, child, path } = startTree("hold", true);
  const pids = await pidRecord(path);
  groups.add(pids.pgid);
  child.stdin.end();
  await lifecycle.settle();
  await eventually(() => pidGone(pids.leader), "leader PID disappearance");
  await eventually(() => pidGone(pids.descendant), "descendant PID disappearance");
  expect(groupGone(pids.pgid)).toBe(true);
  groups.delete(pids.pgid);
});

posixTest("natural leader termination still reaps its surviving process group", async () => {
  const { lifecycle, child, path } = startTree("natural", true);
  const pids = await pidRecord(path);
  groups.add(pids.pgid);
  if (child.exitCode === null)
    await new Promise<void>((resolve) => child.once("exit", () => resolve()));
  expect(pidGone(pids.leader)).toBe(true);
  expect(pidGone(pids.descendant)).toBe(false);
  await lifecycle.settle();
  await eventually(() => pidGone(pids.descendant), "natural-path descendant disappearance");
  expect(groupGone(pids.pgid)).toBe(true);
  groups.delete(pids.pgid);
});

posixTest("the SDK forwarded abort signal targets the owned group, not only its leader", async () => {
  const { lifecycle, forwarded, path } = startTree("hold");
  const pids = await pidRecord(path);
  groups.add(pids.pgid);
  forwarded.abort();
  await lifecycle.settle();
  await eventually(() => pidGone(pids.leader) && pidGone(pids.descendant), "forwarded-abort tree reap");
  expect(groupGone(pids.pgid)).toBe(true);
  groups.delete(pids.pgid);
});

test("Windows installs direct-child ownership without claiming a descendant process group", () => {
  const lifecycle = createAnthropicProcessLifecycle({ platform: "win32" });
  expect(typeof lifecycle.spawnClaudeCodeProcess).toBe("function");
  expect(lifecycle.started()).toBe(false);
  lifecycle.forceKill();
});

test("Windows-style wedged SDK disposal reaps its direct child with no negative PID signal", async () => {
  const dir = mkdtempSync(join(tmpdir(), "north-anthropic-direct-"));
  temporary.push(dir);
  const path = join(dir, "pid");
  let processGroupSignals = 0;
  const lifecycle = createAnthropicProcessLifecycle({
    platform: "win32",
    graceMs: 10,
    termMs: 100,
    killMs: 1_000,
    processSignal: () => { processGroupSignals++; return true; },
  });
  const forwarded = new AbortController();
  lifecycle.spawnClaudeCodeProcess({
    command: process.execPath,
    args: ["-e", `require('node:fs').writeFileSync(${JSON.stringify(path)}, String(process.pid)); setInterval(() => {}, 1000)`],
    cwd: dir,
    env: process.env,
    signal: forwarded.signal,
  });
  let pid = 0;
  await eventually(() => {
    try { pid = Number(readFileSync(path, "utf8")); return pid > 1; }
    catch { return false; }
  }, "direct-child readiness");
  directPids.add(pid);
  await expect(settleAnthropicProcessOwner({
    lifecycle,
    abortController: new AbortController(),
    dispose: () => new Promise<never>(() => {}),
    disposalGraceMs: 10,
  })).rejects.toThrow("anthropic_sdk_disposal_timeout");
  await eventually(() => pidGone(pid), "Windows-style direct-child reap");
  expect(processGroupSignals).toBe(0);
  directPids.delete(pid);
});

posixTest("an asynchronous ENOENT spawn is owned before pid validation and cannot emit unhandled error", async () => {
  const lifecycle = createAnthropicProcessLifecycle();
  expect(() => lifecycle.spawnClaudeCodeProcess!({
    command: `/definitely-missing-north-claude-${Date.now()}`,
    args: [], env: {}, signal: new AbortController().signal,
  })).toThrow("anthropic_owned_process_group_invalid");
  await new Promise((resolve) => setImmediate(resolve));
});

posixTest("a claimed PGID matching the host group is rejected before any negative-PID signal", () => {
  const fake = new EventEmitter() as any;
  fake.pid = 424_241;
  fake.stdin = new PassThrough();
  fake.stdout = new PassThrough();
  fake.killed = false;
  fake.exitCode = null;
  let directKills = 0;
  fake.kill = () => { directKills++; return true; };
  let groupSignals = 0;
  const lifecycle = createAnthropicProcessLifecycle({
    spawn: (() => fake) as any,
    currentProcessGroupId: () => fake.pid,
    processSignal: () => { groupSignals++; return true; },
  });
  expect(() => lifecycle.spawnClaudeCodeProcess!({
    command: "fake", args: [], env: {}, signal: new AbortController().signal,
  })).toThrow("anthropic_owned_process_group_invalid");
  expect(directKills).toBe(1);
  expect(groupSignals).toBe(0);
});

posixTest("group disappearance without a leader exit event fails the reap proof", async () => {
  const fake = new EventEmitter() as any;
  fake.pid = 424_242;
  fake.stdin = new PassThrough();
  fake.stdout = new PassThrough();
  fake.killed = false;
  fake.exitCode = null;
  fake.kill = () => true;
  const signals: Array<NodeJS.Signals | 0 | undefined> = [];
  const lifecycle = createAnthropicProcessLifecycle({
    spawn: (() => fake) as any,
    currentProcessGroupId: () => 999,
    graceMs: 0,
    killMs: 10,
    processSignal: (_pid, signal) => {
      signals.push(signal);
      const error = Object.assign(new Error("gone"), { code: "ESRCH" });
      throw error;
    },
  });
  lifecycle.spawnClaudeCodeProcess!({
    command: "fake", args: [], env: {}, signal: new AbortController().signal,
  });
  await expect(lifecycle.settle()).rejects.toThrow("anthropic_process_leader_reap_failed");
  fake.emit("exit", 0, null);
  lifecycle.forceKill();
  expect(signals).toContain(0);
});

posixTest("settled lifecycle is idempotent and never signals a reused unrelated PGID", async () => {
  const fake = new EventEmitter() as any;
  fake.pid = 424_243;
  fake.stdin = new PassThrough();
  fake.stdout = new PassThrough();
  fake.killed = false;
  fake.exitCode = 0;
  fake.kill = () => true;
  const signals: Array<NodeJS.Signals | 0 | undefined> = [];
  const lifecycle = createAnthropicProcessLifecycle({
    spawn: (() => fake) as any,
    currentProcessGroupId: () => 999,
    graceMs: 0,
    processSignal: (_pid, signal) => {
      signals.push(signal);
      throw Object.assign(new Error("gone"), { code: "ESRCH" });
    },
  });
  lifecycle.spawnClaudeCodeProcess!({
    command: "fake", args: [], env: {}, signal: new AbortController().signal,
  });
  fake.emit("exit", 0, null);
  await lifecycle.settle();
  await lifecycle.settle();
  lifecycle.forceKill();
  expect(signals).toEqual([0]);
});

test("wedged and rejected SDK disposal still settles the process boundary", async () => {
  for (const raw of [
    { return: () => new Promise<never>(() => {}) },
    { return: async () => { throw new Error("sdk disposal failed"); } },
  ]) {
    let settled = 0;
    const abort = new AbortController();
    await expect(disposeAnthropicSdkQuery(raw as any, {
      settle: async () => { settled++; },
      forceKill: () => {},
      started: () => true,
    }, abort, 10)).rejects.toThrow();
    expect(abort.signal.aborted).toBe(true);
    expect(settled).toBe(1);
  }
});

test("Anthropic construction and observer failures abort, dispose, and settle before rejection", async () => {
  for (const failure of ["construction", "observer"] as const) {
    let queryCalls = 0;
    let returns = 0;
    let settles = 0;
    let capturedAbort: AbortController | undefined;
    const lifecycle = {
      spawnClaudeCodeProcess: () => { throw new Error("not used"); },
      settle: async () => { settles++; },
      forceKill: () => {},
      started: () => false,
    };
    const raw = {
      return: async () => { returns++; return { done: true, value: undefined }; },
      interrupt: async () => {},
      setModel: async () => {},
      applyFlagSettings: async () => {},
      async *[Symbol.asyncIterator]() {},
    };
    const managed = createAnthropicQuery({ prompt: "x", options: {} as any }, true, {
      query: (({ options }: any) => {
        queryCalls++;
        capturedAbort = options.abortController;
        if (failure === "construction") throw new Error("construction failed");
        return raw;
      }) as any,
      observe: ((source: any) => {
        if (failure === "observer") throw new Error("observer failed");
        return source;
      }) as any,
      createLifecycle: (() => lifecycle) as any,
    });
    await expect(async () => {
      for await (const _ of managed) { /* no events */ }
    }).toThrow("anthropic_provider_execution_failed");
    expect(queryCalls).toBe(1);
    expect(capturedAbort?.signal.aborted).toBe(true);
    expect(settles).toBe(1);
    expect(returns).toBe(failure === "observer" ? 1 : 0);
  }
});

test("closing an uninitialized Anthropic lazy query constructs no SDK process", async () => {
  let queryCalls = 0;
  let lifecycleCalls = 0;
  const managed = createAnthropicQuery({ prompt: "x", options: {} as any }, true, {
    query: (() => { queryCalls++; throw new Error("must not construct"); }) as any,
    observe: ((source: any) => source) as any,
    createLifecycle: (() => {
      lifecycleCalls++;
      throw new Error("must not create lifecycle");
    }) as any,
  });
  await managed.close?.();
  const events: unknown[] = [];
  for await (const event of managed) events.push(event);
  expect(events).toEqual([]);
  expect(queryCalls).toBe(0);
  expect(lifecycleCalls).toBe(0);
});

test("close during slow Anthropic admission prevents late lifecycle and Query construction", async () => {
  let admitStarted!: () => void;
  const started = new Promise<void>((resolve) => { admitStarted = resolve; });
  let releaseAdmission!: () => void;
  const admission = new Promise<void>((resolve) => { releaseAdmission = resolve; });
  let queryCalls = 0;
  let lifecycleCalls = 0;
  const managed = createAnthropicQuery({ prompt: "x", options: {} as any }, false, {
    admit: async () => { admitStarted(); await admission; },
    query: (() => { queryCalls++; throw new Error("must not construct"); }) as any,
    observe: ((source: any) => source) as any,
    createLifecycle: (() => {
      lifecycleCalls++;
      throw new Error("must not create lifecycle");
    }) as any,
  });
  const iteration = (async () => {
    for await (const _ of managed) { /* no events */ }
  })();
  await started;
  const close = managed.close!();
  releaseAdmission();
  await Promise.all([iteration, close]);
  expect(queryCalls).toBe(0);
  expect(lifecycleCalls).toBe(0);
});

test("caller abort during slow Anthropic admission prevents a provider turn", async () => {
  const callerAbort = new AbortController();
  let admitStarted!: () => void;
  const started = new Promise<void>((resolve) => { admitStarted = resolve; });
  let releaseAdmission!: () => void;
  const admission = new Promise<void>((resolve) => { releaseAdmission = resolve; });
  let queryCalls = 0;
  let lifecycleCalls = 0;
  const managed = createAnthropicQuery({
    prompt: "x",
    options: { abortController: callerAbort } as any,
  }, false, {
    admit: async () => { admitStarted(); await admission; },
    query: (() => { queryCalls++; throw new Error("must not construct"); }) as any,
    observe: ((source: any) => source) as any,
    createLifecycle: (() => {
      lifecycleCalls++;
      throw new Error("must not create lifecycle");
    }) as any,
  });
  const iteration = (async () => {
    for await (const _ of managed) { /* no events */ }
  })();
  await started;
  callerAbort.abort(new Error("host terminated"));
  releaseAdmission();
  await iteration;
  expect(queryCalls).toBe(0);
  expect(lifecycleCalls).toBe(0);
});

test("initialized Anthropic close propagates source cleanup and reap failure", async () => {
  const raw = {
    interrupt: async () => {},
    return: async () => ({ done: true, value: undefined }),
    setModel: async () => {},
    applyFlagSettings: async () => {},
    async *[Symbol.asyncIterator]() {},
  };
  const managed = createAnthropicQuery({ prompt: "x", options: {} as any }, true, {
    query: (() => raw) as any,
    observe: ((source: any) => source) as any,
    createLifecycle: (() => ({
      spawnClaudeCodeProcess: () => { throw new Error("not used"); },
      settle: async () => { throw new Error("owned process reap failed"); },
      forceKill: () => {},
      started: () => false,
    })) as any,
  });
  for await (const _ of managed) { /* initialize */ }
  await expect(managed.close!()).rejects.toThrow("anthropic_provider_execution_failed");
});

function fakeHost() {
  const signals = new Map<string, Set<() => void>>([
    ["SIGTERM", new Set()], ["SIGINT", new Set()],
  ]);
  const exits = new Set<() => void>();
  const exitCodes: number[] = [];
  return {
    control: {
      onSignal: (signal: string, listener: () => void) => { signals.get(signal)!.add(listener); },
      offSignal: (signal: string, listener: () => void) => { signals.get(signal)!.delete(listener); },
      onExit: (listener: () => void) => { exits.add(listener); },
      offExit: (listener: () => void) => { exits.delete(listener); },
      exit: (value: number) => { exitCodes.push(value); },
    },
    emit: (signal: "SIGTERM" | "SIGINT") => {
      for (const listener of [...signals.get(signal)!]) listener();
    },
    emitExit: () => { for (const listener of [...exits]) listener(); },
    listenerCount: () => [...signals.values()].reduce((sum, set) => sum + set.size, 0) + exits.size,
    exitCodes,
  };
}

test("host signal listeners are installed only while a managed participant is live", () => {
  const host = fakeHost();
  const coordinator = new HostTerminationCoordinator(host.control as any);
  expect(host.listenerCount()).toBe(0);
  const participant = coordinator.register({ close: async () => {} });
  expect(host.listenerCount()).toBe(3);
  participant.publicationSettled();
  participant.cleanupSettled();
  participant.release();
  expect(host.listenerCount()).toBe(0);
});

test("a query constructed against a sticky host signal is force-closed before attachment fails", async () => {
  const host = fakeHost();
  const coordinator = new HostTerminationCoordinator(host.control as any);
  const termination = new ManagedQueryTermination(
    (options) => coordinator.register(options),
  );
  host.emit("SIGTERM");
  let forceCalls = 0;
  expect(() => termination.attachQuery({
    forceClose: () => { forceCalls++; },
    async *[Symbol.asyncIterator]() {},
  })).toThrow("host termination requested (SIGTERM)");
  expect(forceCalls).toBe(1);
  termination.publicationSettled();
  termination.cleanupSettled();
  termination.release();
  await eventually(() => host.exitCodes.length === 1, "sticky-signal attachment exit");
  expect(host.exitCodes).toEqual([143]);
});

posixTest("host signal reaps the real Anthropic process group before publication opens", async () => {
  const { lifecycle, path } = startTree("hold", true);
  const pids = await pidRecord(path);
  groups.add(pids.pgid);
  const host = fakeHost();
  const coordinator = new HostTerminationCoordinator(host.control as any);
  const termination = new ManagedQueryTermination(
    (options) => coordinator.register(options),
  );
  termination.attachQuery({
    close: () => lifecycle.settle(),
    forceClose: () => lifecycle.forceKill(),
    async *[Symbol.asyncIterator]() {},
  });
  host.emit("SIGTERM");
  await eventually(
    () => pidGone(pids.leader) && pidGone(pids.descendant) && groupGone(pids.pgid),
    "host-signal Anthropic ownership reap",
  );
  groups.delete(pids.pgid);
  expect(host.exitCodes).toEqual([]);
  termination.publicationSettled();
  termination.cleanupSettled();
  termination.release();
  await eventually(() => host.exitCodes.length === 1, "post-publication signal exit");
  expect(host.exitCodes).toEqual([143]);
});

test("first signal waits for close/reap and publication; a late participant joins the barrier", async () => {
  const host = fakeHost();
  const coordinator = new HostTerminationCoordinator(host.control as any);
  let resolveFirst!: () => void;
  const firstClose = new Promise<void>((resolve) => { resolveFirst = resolve; });
  let firstCalls = 0;
  const first = coordinator.register({ close: async () => { firstCalls++; await firstClose; } });
  host.emit("SIGTERM");
  await Promise.resolve();
  expect(first.signal()).toBe("SIGTERM");
  expect(firstCalls).toBe(1);
  expect(host.exitCodes).toEqual([]);

  let resolveLate!: () => void;
  const lateClose = new Promise<void>((resolve) => { resolveLate = resolve; });
  let lateCalls = 0;
  const late = coordinator.register({ close: async () => { lateCalls++; await lateClose; } });
  resolveFirst();
  first.publicationSettled();
  first.cleanupSettled();
  first.release();
  await Promise.resolve();
  await Promise.resolve();
  expect(late.signal()).toBe("SIGTERM");
  expect(lateCalls).toBe(1);
  expect(host.exitCodes).toEqual([]);
  resolveLate();
  late.publicationSettled();
  late.cleanupSettled();
  late.release();
  await eventually(() => host.exitCodes.length === 1, "host signal exit publication");
  expect(host.exitCodes).toEqual([143]);
});

test("first signal waits through outer cleanup after close and publication", async () => {
  const host = fakeHost();
  const coordinator = new HostTerminationCoordinator(host.control as any);
  let onSignal: string | undefined;
  const participant = coordinator.register({
    onSignal: (signal) => { onSignal = signal; },
    close: async () => {},
  });
  host.emit("SIGTERM");
  // onSignal is synchronous so preflight cannot advance to a provider turn
  // before the asynchronous close callback begins.
  expect(onSignal).toBe("SIGTERM");
  participant.publicationSettled();
  participant.release();
  await new Promise((resolve) => setImmediate(resolve));
  expect(host.exitCodes).toEqual([]);
  participant.cleanupSettled();
  await eventually(() => host.exitCodes.length === 1, "outer-cleanup signal exit");
  expect(host.exitCodes).toEqual([143]);
});

test("second signal force-closes only registered ownership and preserves the first signal exit code", async () => {
  const host = fakeHost();
  const coordinator = new HostTerminationCoordinator(host.control as any);
  let resolveClose!: () => void;
  const closeGate = new Promise<void>((resolve) => { resolveClose = resolve; });
  let forceCalls = 0;
  const participant = coordinator.register({
    close: async () => { await closeGate; },
    forceClose: () => { forceCalls++; },
  });
  host.emit("SIGTERM");
  await Promise.resolve();
  host.emit("SIGINT");
  expect(forceCalls).toBe(1);
  expect(host.exitCodes).toEqual([143]);
  resolveClose();
  participant.publicationSettled();
  participant.cleanupSettled();
  participant.release();
  await new Promise((resolve) => setImmediate(resolve));
});

test("first signal has a bounded publication deadline and force-closes before conventional exit", async () => {
  const host = fakeHost();
  const coordinator = new HostTerminationCoordinator(host.control as any, { shutdownMs: 20 });
  let forceCalls = 0;
  coordinator.register({
    close: () => new Promise<never>(() => {}),
    forceClose: () => { forceCalls++; },
  });
  host.emit("SIGINT");
  await eventually(() => host.exitCodes.length === 1, "bounded SIGINT exit");
  expect(forceCalls).toBe(1);
  expect(host.exitCodes).toEqual([130]);
});

test("a participant entering after graceful drain is sealed receives synchronous force-close", async () => {
  const host = fakeHost();
  let scheduledExit: (() => void) | undefined;
  const coordinator = new HostTerminationCoordinator(host.control as any, {
    shutdownMs: 1_000,
    scheduleExit: (exit) => { scheduledExit = exit; },
  });
  const first = coordinator.register({ close: async () => {} });
  host.emit("SIGTERM");
  first.publicationSettled();
  first.cleanupSettled();
  first.release();
  await eventually(() => scheduledExit !== undefined, "sealed host-exit transition");
  let closeCalls = 0;
  let forceCalls = 0;
  const late = coordinator.register({
    close: async () => { closeCalls++; },
    forceClose: () => { forceCalls++; },
  });
  expect(late.signal()).toBe("SIGTERM");
  expect(forceCalls).toBe(1);
  expect(closeCalls).toBe(0);
  scheduledExit!();
  expect(host.exitCodes).toEqual([143]);
  late.publicationSettled();
  late.cleanupSettled();
  late.release();
});

test("host exit fallback is synchronous force-only and disappears when idle", () => {
  const host = fakeHost();
  const coordinator = new HostTerminationCoordinator(host.control as any);
  let closeCalls = 0;
  let forceCalls = 0;
  const participant = coordinator.register({
    close: async () => { closeCalls++; },
    forceClose: () => { forceCalls++; },
  });
  host.emitExit();
  expect(forceCalls).toBe(1);
  expect(closeCalls).toBe(0);
  participant.publicationSettled();
  participant.cleanupSettled();
  participant.release();
  expect(host.listenerCount()).toBe(0);
});
