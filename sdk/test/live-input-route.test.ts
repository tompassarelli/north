import { EventEmitter } from "node:events";
import { expect, test } from "bun:test";
import {
  inputChannel,
  LiveFeedReapTimeoutError,
  LiveFeedStartupTimeoutError,
  subscribeFeed,
  type FeedSubscription,
  type FeedMail,
  type InputAdmission,
  type SubscriptionRuntime,
} from "../src/coordination";
import {
  ManagedLiveInputRoute,
  prepareManagedTerminalFollowUp,
  type ManagedRouteAxes,
} from "../src/live-input-route";
import { ProviderRetrySafeError } from "../src/providers/types";

function deferred<T = void>() {
  let resolve!: (value: T | PromiseLike<T>) => void;
  let reject!: (reason?: unknown) => void;
  const promise = new Promise<T>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
}

function subscription(
  ready: Promise<void>,
  stop: () => void | Promise<void> = () => {},
  drain: (frozenRouteEpoch: string) => Promise<void> = async () => {},
  caughtUp: Promise<void> = Promise.resolve(),
  replay: () => Promise<void> = () => caughtUp,
): FeedSubscription {
  const settle = async () => { await stop(); };
  return Object.assign(settle, {
    ready,
    caughtUp,
    replay,
    drain,
    isArmed: () => true,
  });
}

class RestartFeedStdin extends EventEmitter {
  destroyed = false;
  writable = true;
  writes: string[] = [];

  write(value: string | Buffer) {
    if (!this.writable || this.destroyed) throw new Error("stdin unavailable");
    this.writes.push(String(value));
    return true;
  }

  end() { this.writable = false; }
  destroy() { this.destroyed = true; this.writable = false; }
}

class RestartFeedStdout extends EventEmitter {
  destroyed = false;
  destroy() { this.destroyed = true; }
}

class RestartFeedChild extends EventEmitter {
  stdout = new RestartFeedStdout();
  stdin = new RestartFeedStdin();
  stderr = null;
  signals: Array<string | undefined> = [];

  kill(signal?: string) { this.signals.push(signal); return true; }
  unref() {}
}

interface RestartFeedTimer {
  callback: () => void;
  delayMs: number;
  cancelled: boolean;
}

function restartFeedHarness() {
  const children: RestartFeedChild[] = [];
  const timers: RestartFeedTimer[] = [];
  const runtime: SubscriptionRuntime = {
    spawn: (() => {
      const child = new RestartFeedChild();
      children.push(child);
      return child;
    }) as unknown as NonNullable<SubscriptionRuntime["spawn"]>,
    bbExecutable: "/nix/store/00000000000000000000000000000000-babashka-test/bin/bb",
    port: "7977",
    schedule: (callback, delayMs) => {
      const timer: RestartFeedTimer = {
        callback: () => {
          if (timer.cancelled) return;
          timer.cancelled = true;
          callback();
        },
        delayMs,
        cancelled: false,
      };
      timers.push(timer);
      return timer;
    },
    cancel: (timer) => { (timer as RestartFeedTimer).cancelled = true; },
    now: () => 0,
    initialBackoffMs: 100,
    maxBackoffMs: 1_000,
    healthyResetMs: 30_000,
    readyTimeoutMs: 5_000,
    startupTimeoutMs: 20_000,
    admissionTimeoutMs: 3_000,
    drainTimeoutMs: 45_000,
    stopKillMs: 250,
    stopReapMs: 5_000,
  };
  return {
    children,
    runtime,
    activeTimers: () => timers.filter(({ cancelled }) => !cancelled),
  };
}

function emitFeedLine(child: RestartFeedChild, value: object) {
  child.stdout.emit("data", Buffer.from(`${JSON.stringify(value)}\n`));
}

function readyFeed(child: RestartFeedChild, recipient: string) {
  emitFeedLine(child, {
    protocol: "north-live-feed-v1",
    type: "ready",
    recipient,
    cursor: 17,
  });
}

function caughtUpFeed(child: RestartFeedChild, recipient: string) {
  emitFeedLine(child, {
    protocol: "north-live-feed-v1",
    type: "caught_up",
    recipient,
  });
}

function wakeFeed(child: RestartFeedChild) {
  emitFeedLine(child, {
    protocol: "north-live-feed-v1",
    type: "mail",
    id: "@msg:wake-one",
    from: "peer",
    subject: "update",
    body: "continue once",
    wakeAttempt,
  });
}

async function settleFeed() {
  const settled = Promise.withResolvers<void>();
  setImmediate(settled.resolve);
  await settled.promise;
}

const initialRoute: ManagedRouteAxes = {
  provider: "anthropic",
  providerTarget: "claude-personal",
  liveInput: "streaming",
  model: "claude-opus-4-8",
  effort: "xhigh",
};

const turnMessagesRoute: ManagedRouteAxes = {
  ...initialRoute,
  provider: "openai",
  providerTarget: "codex-personal",
  liveInput: "turn-messages",
  model: "gpt-5.6-sol",
};

const inputAdmission = {
  consumed: Promise.resolve(true),
  cancel: () => {},
};

const wakeAttempt = `wake:${"a".repeat(64)}`;
const wakeMail = (summary: string, id = "@msg:wake-one"): FeedMail => ({
  id,
  wakeAttempt,
  summary,
});
const idleEvent = (suffix: string) => ({
  id: `event:idle-${suffix}`,
  kind: "model-call.completed" as const,
  modelCallId: `model-call:idle-${suffix}`,
});

test("turn-messages terminal replay delivers one late follow-up and keeps ownership until dequeue", async () => {
  const replayStarted = deferred();
  const replayCaughtUp = deferred();
  const channel = inputChannel("initial");
  const stream = channel.stream();
  await stream.next();
  let deliver: ((message: FeedMail) => InputAdmission) | undefined;
  let feeds = 0;
  const writes: string[] = [];
  const receipts: string[] = [];
  const route = new ManagedLiveInputRoute(
    "lane-turn-messages-follow-up",
    { kind: "lane" },
    turnMessagesRoute,
    (message) => channel.push(message),
    (_agentId, onMessage, runtime) => {
      feeds++;
      deliver = onMessage;
      expect(runtime).toMatchObject({ deferredStart: true });
      return subscription(
        Promise.resolve(),
        () => channel.end(),
        async () => {},
        replayCaughtUp.promise,
        () => {
          replayStarted.resolve();
          return replayCaughtUp.promise;
        },
      );
    },
    (_agentId, facts) => { writes.push(facts.liveInputState!); },
    undefined,
    (_context, phase, event, kind) => {
      receipts.push([phase, event, kind].filter(Boolean).join(":"));
      return "created";
    },
  );

  expect(route.initialProjection().liveInputState).toBe("pending");
  await route.activate(turnMessagesRoute);
  expect(feeds).toBe(0);
  await route.activate(turnMessagesRoute, true);
  expect(feeds).toBe(1);
  expect(writes).toEqual(["armed"]);

  route.observeCommittedEvent(idleEvent("follow-up"));
  const terminal = route.prepareTerminalFollowUp(
    new AbortController().signal,
  );
  await replayStarted.promise;
  let terminalSettled = false;
  void terminal.then(() => { terminalSettled = true; });
  await Promise.resolve();
  expect(terminalSettled).toBe(false);

  const first = deliver!(wakeMail("late follow-up"));
  expect(await terminal).toBe(true);
  expect(channel.pending()).toBe(1);
  let acknowledged = false;
  void first.consumed.then((value) => { acknowledged = value; });
  await Promise.resolve();
  expect(acknowledged).toBe(false);

  const second = deliver!(wakeMail("must remain replayable", "@msg:wake-two"));
  expect(await second.consumed).toBe(false);
  expect((await stream.next()).value?.text).toBe("late follow-up");
  await Promise.resolve();
  expect(acknowledged).toBe(false);
  route.observeCommittedEvent({
    id: "event:turn",
    kind: "model-call.started",
    modelCallId: "model-call:wake",
  });
  expect(await first.consumed).toBe(true);
  route.observeCommittedEvent({
    id: "event:action",
    kind: "message.recorded",
    role: "assistant",
    stage: "started",
    modelCallId: "model-call:wake",
  });
  expect(receipts).toEqual([
    "idle:event:idle-follow-up",
    "turn:event:turn",
    "action:event:action:assistant.message.recorded",
  ]);
  route.observeCommittedEvent({
    id: "event:turn-complete",
    kind: "model-call.completed",
    modelCallId: "model-call:wake",
  });
  expect(channel.liveMessagesReceived()).toBe(1);
  const durableAck = route.prepareTerminalFollowUp(new AbortController().signal);
  let durableAckSettled = false;
  void durableAck.then(() => { durableAckSettled = true; });
  await Promise.resolve();
  expect(durableAckSettled).toBe(false);
  replayCaughtUp.resolve();
  expect(await durableAck).toBe(false);
  await route.freezeAndUnbind();
  expect(writes).toEqual(["armed", "frozen"]);
});

test("pre-dequeue feed-child restart replays one retained wake and acks after one turn", async () => {
  const channel = inputChannel("initial");
  const stream = channel.stream();
  await stream.next();
  const harness = restartFeedHarness();
  const receiptWrites: string[] = [];
  let idleWrites = 0;
  const agentId = "lane-turn-messages-restart";
  const route = new ManagedLiveInputRoute(
    agentId,
    { kind: "lane" },
    turnMessagesRoute,
    (message) => channel.push(message),
    (recipient, onMessage, runtime) => subscribeFeed(
      recipient,
      onMessage,
      { ...harness.runtime, ...runtime },
    ),
    () => {},
    undefined,
    (_context, phase, event) => {
      receiptWrites.push(`${phase}:${event}`);
      if (phase === "idle") return idleWrites++ === 0 ? "created" : "unknown";
      return "created";
    },
  );
  const activation = route.activate(turnMessagesRoute, true);
  const first = harness.children[0]!;
  readyFeed(first, agentId);
  await activation;
  route.observeCommittedEvent(idleEvent("restart"));
  const replay = route.prepareTerminalFollowUp(new AbortController().signal);
  wakeFeed(first);
  await settleFeed();
  expect(await replay).toBe(true);
  expect(channel.pending()).toBe(1);
  expect(first.stdin.writes).not.toContain('{"type":"ack","id":"@msg:wake-one"}\n');

  first.stdin.writable = false;
  first.emit("close", 1);
  await settleFeed();
  expect(channel.pending()).toBe(0);
  expect(channel.liveMessagesReceived()).toBe(0);
  const retry = harness.activeTimers().find(({ delayMs }) => delayMs === 100);
  expect(retry).toBeDefined();
  retry!.callback();

  const replacement = harness.children[1]!;
  readyFeed(replacement, agentId);
  wakeFeed(replacement);
  await settleFeed();
  expect(channel.pending()).toBe(1);
  expect(replacement.stdin.writes).not.toContain(
    '{"type":"ack","id":"@msg:wake-one"}\n',
  );

  expect((await stream.next()).value?.text).toContain("continue once");
  await settleFeed();
  expect(channel.liveMessagesReceived()).toBe(1);
  expect(replacement.stdin.writes).not.toContain(
    '{"type":"ack","id":"@msg:wake-one"}\n',
  );
  route.observeCommittedEvent({
    id: "event:turn-after-restart",
    kind: "model-call.started",
    modelCallId: "model-call:turn-after-restart",
  });
  await settleFeed();
  expect(replacement.stdin.writes.filter(
    (line) => line === '{"type":"ack","id":"@msg:wake-one"}\n',
  )).toHaveLength(1);
  expect(receiptWrites).toEqual([
    "idle:event:idle-restart",
    "idle:event:idle-restart",
    "turn:event:turn-after-restart",
  ]);

  const frozen = route.freezeAndUnbind();
  await Promise.resolve();
  replacement.emit("close", 0);
  await frozen;
  channel.end();
});

test("fresh route nacks an idle-only wake without retained pre-dequeue proof", async () => {
  const channel = inputChannel("initial");
  const stream = channel.stream();
  await stream.next();
  const harness = restartFeedHarness();
  const agentId = "lane-turn-messages-ambiguous-restart";
  const route = new ManagedLiveInputRoute(
    agentId,
    { kind: "lane" },
    turnMessagesRoute,
    (message) => channel.push(message),
    (recipient, onMessage, runtime) => subscribeFeed(
      recipient,
      onMessage,
      { ...harness.runtime, ...runtime },
    ),
    () => {},
    undefined,
    () => "unknown",
  );
  const activation = route.activate(turnMessagesRoute, true);
  const child = harness.children[0]!;
  readyFeed(child, agentId);
  await activation;
  route.observeCommittedEvent(idleEvent("ambiguous-restart"));
  const replay = route.prepareTerminalFollowUp(new AbortController().signal);
  wakeFeed(child);
  await settleFeed();
  expect(channel.pending()).toBe(0);
  expect(channel.liveMessagesReceived()).toBe(0);
  expect(child.stdin.writes).toContain('{"type":"nack","id":"@msg:wake-one"}\n');
  expect(child.stdin.writes).not.toContain('{"type":"ack","id":"@msg:wake-one"}\n');
  caughtUpFeed(child, agentId);
  expect(await replay).toBe(false);

  const frozen = route.freezeAndUnbind();
  await Promise.resolve();
  child.emit("close", 0);
  await frozen;
  channel.end();
});

test("provider dequeue without canonical turn acceptance remains unacknowledged", async () => {
  const channel = inputChannel("initial");
  const stream = channel.stream();
  await stream.next();
  let deliver: ((message: FeedMail) => InputAdmission) | undefined;
  const receipts: string[] = [];
  const route = new ManagedLiveInputRoute(
    "lane-turn-messages-rejected-turn",
    { kind: "lane" },
    turnMessagesRoute,
    (message) => channel.push(message),
    (_agentId, onMessage) => {
      deliver = onMessage;
      return subscription(Promise.resolve(), () => channel.end());
    },
    () => {},
    undefined,
    (_context, phase, event) => {
      receipts.push(`${phase}:${event}`);
      return "created";
    },
  );
  await route.activate(turnMessagesRoute, true);
  route.observeCommittedEvent(idleEvent("rejected-turn"));
  void route.prepareTerminalFollowUp(new AbortController().signal);

  const admission = deliver!(wakeMail("turn/start will reject"));
  expect((await stream.next()).value?.text).toBe("turn/start will reject");
  let settled = false;
  void admission.consumed.then(() => { settled = true; });
  await Promise.resolve();
  expect(settled).toBe(false);
  admission.cancel();
  expect(await admission.consumed).toBe(false);
  expect(receipts).toEqual(["idle:event:idle-rejected-turn"]);
  await route.freezeAndUnbind();
});

test("turn receipt exceptions and non-commits nack and retry only the exact first turn", async () => {
  const channel = inputChannel("initial");
  const stream = channel.stream();
  await stream.next();
  let deliver: ((message: FeedMail) => InputAdmission) | undefined;
  const writes: string[] = [];
  let turnAvailable = false;
  let failedTurnWrites = 0;
  const route = new ManagedLiveInputRoute(
    "lane-turn-messages-receipt-failure",
    { kind: "lane" },
    turnMessagesRoute,
    (message) => channel.push(message),
    (_agentId, onMessage) => {
      deliver = onMessage;
      return subscription(Promise.resolve(), () => channel.end());
    },
    () => {},
    undefined,
    (_context, phase, event) => {
      writes.push(`${phase}:${event}`);
      if (phase === "turn" && !turnAvailable) {
        failedTurnWrites++;
        if (failedTurnWrites === 1) return "unknown";
        throw new Error(`${phase} unavailable`);
      }
      return "created";
    },
  );
  await route.activate(turnMessagesRoute, true);
  route.observeCommittedEvent(idleEvent("receipt-failure"));
  void route.prepareTerminalFollowUp(new AbortController().signal);
  const admission = deliver!(wakeMail("one durable attempt"));
  await stream.next();

  route.observeCommittedEvent({
    id: "event:reused-id",
    kind: "model-call.started",
    modelCallId: "model-call:idle-receipt-failure",
  });
  route.observeCommittedEvent({
    id: "event:first-turn",
    kind: "model-call.started",
    modelCallId: "model-call:first-turn",
  });
  expect(await admission.consumed).toBe(false);
  route.observeCommittedEvent({
    id: "event:later-turn",
    kind: "model-call.started",
    modelCallId: "model-call:later-turn",
  });
  route.observeCommittedEvent({
    id: "event:first-action",
    kind: "message.recorded",
    role: "assistant",
    modelCallId: "model-call:first-turn",
  });
  route.observeCommittedEvent({
    id: "event:later-action",
    kind: "tool.admitted",
    modelCallId: "model-call:first-turn",
  });
  const unavailableReplay = deliver!(wakeMail("one durable attempt"));
  expect(await unavailableReplay.consumed).toBe(false);
  turnAvailable = true;
  const replay = deliver!(wakeMail("one durable attempt"));
  expect(await replay.consumed).toBe(true);
  expect(channel.liveMessagesReceived()).toBe(1);
  expect(writes).toEqual([
    "idle:event:idle-receipt-failure",
    "turn:event:first-turn",
    "turn:event:first-turn",
    "turn:event:first-turn",
  ]);
  await route.freezeAndUnbind();
});

test("turn-messages terminal replay leaves a claimed follow-up unconsumed on abort", async () => {
  const replayStarted = deferred();
  const replayCaughtUp = deferred();
  const channel = inputChannel("initial");
  const stream = channel.stream();
  await stream.next();
  let deliver: ((message: FeedMail) => InputAdmission) | undefined;
  const route = new ManagedLiveInputRoute(
    "lane-turn-messages-abort",
    { kind: "lane" },
    turnMessagesRoute,
    (message) => channel.push(message),
    (_agentId, onMessage) => {
      deliver = onMessage;
      return subscription(
        Promise.resolve(),
        () => channel.end(),
        async () => {},
        replayCaughtUp.promise,
        () => {
          replayStarted.resolve();
          return replayCaughtUp.promise;
        },
      );
    },
    () => {},
    undefined,
    () => "created",
  );
  await route.activate(turnMessagesRoute, true);

  const abort = new AbortController();
  route.observeCommittedEvent(idleEvent("abort"));
  const terminal = route.prepareTerminalFollowUp(abort.signal);
  await replayStarted.promise;
  abort.abort();
  expect(await terminal).toBe(false);
  const admission = deliver!(wakeMail("retain after abort", "@msg:wake-abort"));
  await route.freezeAndUnbind();
  expect(await admission.consumed).toBe(false);
  expect(channel.liveMessagesReceived()).toBe(0);
});

test("the token tripwire runs before a turn-messages feed may claim follow-up mail", async () => {
  let replays = 0;
  const neverCaughtUp = deferred();
  const neverReplayed = deferred();
  const route = new ManagedLiveInputRoute(
    "lane-turn-messages-token-gate",
    { kind: "lane" },
    turnMessagesRoute,
    () => inputAdmission,
    () => subscription(
      Promise.resolve(),
      () => {},
      async () => {},
      neverCaughtUp.promise,
      () => {
        replays++;
        return neverReplayed.promise;
      },
    ),
    () => {},
  );
  await route.activate(turnMessagesRoute, true);

  const error = await prepareManagedTerminalFollowUp(route, {
    signal: new AbortController().signal,
    throwIfTerminated: () => { throw new Error("managed token tripwire blocked"); },
  }).catch((cause) => cause);

  expect((error as Error).message).toBe("managed token tripwire blocked");
  expect(replays).toBe(0);
  await route.freezeAndUnbind();
});

test("a hard deadline crossing terminal replay preserves provider queue ownership", async () => {
  const replayStarted = deferred();
  const replayCaughtUp = deferred();
  const channel = inputChannel("initial");
  const stream = channel.stream();
  await stream.next();
  let deliver: ((message: FeedMail) => InputAdmission) | undefined;
  const route = new ManagedLiveInputRoute(
    "lane-turn-messages-deadline-gate",
    { kind: "lane" },
    turnMessagesRoute,
    (message) => channel.push(message),
    (_agentId, onMessage) => {
      deliver = onMessage;
      return subscription(
        Promise.resolve(),
        () => channel.end(),
        async () => {},
        replayCaughtUp.promise,
        () => {
          replayStarted.resolve();
          return replayCaughtUp.promise;
        },
      );
    },
    () => {},
    undefined,
    () => "created",
  );
  await route.activate(turnMessagesRoute, true);
  route.observeCommittedEvent(idleEvent("deadline"));

  let gateChecks = 0;
  const terminal = prepareManagedTerminalFollowUp(route, {
    signal: new AbortController().signal,
    throwIfTerminated: () => {
      gateChecks++;
      if (gateChecks === 2) throw new Error("managed hard deadline crossed");
    },
  });
  await replayStarted.promise;
  const admission = deliver!(wakeMail("deadline-owned follow-up", "@msg:wake-deadline"));
  const error = await terminal.catch((cause) => cause);

  expect((error as Error).message).toBe("managed hard deadline crossed");
  expect(gateChecks).toBe(2);
  expect(channel.liveMessagesReceived()).toBe(0);
  await route.freezeAndUnbind();
  expect(await admission.consumed).toBe(false);
});

// Resumed continuation turn (thread 019f8ec5): a streaming orchestrator that
// ends its turn to open a fresh RESUMED provider turn re-enters provider
// admission, whose callback re-invokes activate on the SAME managed route. The
// turn-1 feed is still bound and armed; re-arming it once minted a second
// coordinator subscription and killed the lane with "already has a bound feed".
// The armed feed on an identical route IS the resumed turn's feed — reuse it,
// no second subscription, no throw.
test("activate is idempotent for an already-armed identical route (resumed turn)", async () => {
  let feeds = 0;
  const writes: string[] = [];
  const route = new ManagedLiveInputRoute(
    "lane-resume-rebind",
    { kind: "lane" },
    initialRoute,
    () => inputAdmission,
    () => {
      feeds++;
      return subscription(Promise.resolve());
    },
    (_agentId, facts) => { writes.push(facts.liveInputState!); },
  );
  await route.activate(initialRoute);
  // The resumed turn's admission callback activates the SAME route again.
  await route.activate(initialRoute);
  expect(feeds).toBe(1); // no second subscription minted
  expect(writes).toEqual(["armed"]); // no re-publish churn
});

// A genuinely different route while a feed is still bound is NOT a resume — it
// is a lifecycle bug (a real fallback freezes+unbinds before re-activating). The
// idempotent path must not mask it: a divergent route still throws.
test("activate still refuses a divergent route while a feed is bound", async () => {
  const route = new ManagedLiveInputRoute(
    "lane-resume-divergent",
    { kind: "lane" },
    initialRoute,
    () => inputAdmission,
    () => subscription(Promise.resolve()),
    () => {},
  );
  await route.activate(initialRoute);
  const error = await route.activate({
    ...initialRoute,
    providerTarget: "claude-work",
  }).catch((cause) => cause);
  expect(error).toBeInstanceOf(Error);
  expect((error as Error).message).toBe(
    "managed live-input route already has a bound feed",
  );
});

test("terminal unbind waits for direct feed-child settlement", async () => {
  const stopGate = deferred();
  const events: string[] = [];
  const route = new ManagedLiveInputRoute(
    "lane-await-reap",
    { kind: "lane" },
    initialRoute,
    () => inputAdmission,
    () => subscription(
      Promise.resolve(),
      async () => {
        events.push("stop");
        await stopGate.promise;
        events.push("reaped");
      },
      async () => { events.push("drain"); },
    ),
    (_agentId, facts) => { events.push(`write:${facts.liveInputState}`); },
  );
  await route.activate(initialRoute);
  let settled = false;
  const terminal = route.freezeAndUnbind().then(() => { settled = true; });
  await Promise.resolve();
  await Promise.resolve();
  expect(events).toEqual(["write:armed", "write:frozen", "drain", "stop"]);
  expect(settled).toBe(false);
  stopGate.resolve();
  await terminal;
  expect(events).toEqual([
    "write:armed",
    "write:frozen",
    "drain",
    "stop",
    "reaped",
  ]);
});

test("terminal reap rejection is shared by repeated freeze calls with one stop", async () => {
  const reapTimeout = new LiveFeedReapTimeoutError(5_000);
  const stopSettlement = Promise.reject(reapTimeout);
  void stopSettlement.catch(() => {});
  let stops = 0;
  const route = new ManagedLiveInputRoute(
    "lane-sticky-reap-timeout",
    { kind: "lane" },
    initialRoute,
    () => inputAdmission,
    () => subscription(
      Promise.resolve(),
      () => {
        stops++;
        return stopSettlement;
      },
    ),
    () => {},
  );
  await route.activate(initialRoute);

  const first = await route.freezeAndUnbind().catch((error) => error);
  const second = await route.freezeAndUnbind().catch((error) => error);

  expect(first).toBe(reapTimeout);
  expect(second).toBe(first);
  expect((second as LiveFeedReapTimeoutError).code)
    .toBe("NORTH_LIVE_FEED_REAP_TIMEOUT");
  expect(stops).toBe(1);
});

test("successful terminal unbind remains idempotent across repeated freeze calls", async () => {
  let stops = 0;
  const route = new ManagedLiveInputRoute(
    "lane-idempotent-terminal-unbind",
    { kind: "lane" },
    initialRoute,
    () => inputAdmission,
    () => subscription(
      Promise.resolve(),
      () => { stops++; },
    ),
    () => {},
  );
  await route.activate(initialRoute);

  await route.freezeAndUnbind();
  await route.freezeAndUnbind();

  expect(stops).toBe(1);
});

test("streaming route publishes armed only after feed readiness", async () => {
  const gate = deferred();
  const writes: any[] = [];
  const route = new ManagedLiveInputRoute(
    "lane-ready",
    { kind: "lane" },
    initialRoute,
    () => inputAdmission,
    () => subscription(gate.promise),
    (_agentId, facts) => { writes.push(facts); },
  );
  expect(route.initialProjection().liveInputState).toBe("pending");
  const activation = route.activate(initialRoute);
  await Promise.resolve();
  expect(writes).toHaveLength(0);
  gate.resolve();
  await activation;
  expect(writes).toHaveLength(1);
  expect(writes[0]).toMatchObject({
    liveInput: "streaming",
    liveInputState: "armed",
  });
  expect(writes[0].liveInputEpoch).toMatch(
    /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/,
  );
});

test("readiness failure is retry-safe before route publication", async () => {
  let stops = 0;
  const writes: any[] = [];
  const route = new ManagedLiveInputRoute(
    "lane-timeout",
    { kind: "lane" },
    initialRoute,
    () => inputAdmission,
    () => subscription(
      Promise.reject(new LiveFeedStartupTimeoutError(30_000)),
      () => { stops++; },
    ),
    (_agentId, facts) => { writes.push(facts); },
  );
  const error = await route.activate(initialRoute).catch((cause) => cause);
  expect(error).toBeInstanceOf(ProviderRetrySafeError);
  expect(error.message).toBe("live_input_feed_unavailable_before_acceptance");
  expect(writes).toHaveLength(0);
  expect(stops).toBe(1);
});

test("armed publication failure is fatal and stops the uncommitted feed", async () => {
  let stops = 0;
  const route = new ManagedLiveInputRoute(
    "lane-publication-fails",
    { kind: "lane" },
    initialRoute,
    () => inputAdmission,
    () => subscription(Promise.resolve(), () => { stops++; }),
    () => { throw new Error("route commit failed"); },
  );
  const error = await route.activate(initialRoute).catch((cause) => cause);
  expect(error).not.toBeInstanceOf(ProviderRetrySafeError);
  expect(error.message).toBe("route commit failed");
  expect(stops).toBe(1);
});

test("lost-ack recovery commits armed locally and preserves the already-ready feed", async () => {
  let stops = 0;
  const states: string[] = [];
  const route = new ManagedLiveInputRoute(
    "lane-lost-ack-recovered",
    { kind: "lane" },
    initialRoute,
    () => inputAdmission,
    () => subscription(
      Promise.resolve(),
      () => { stops++; },
      async () => {},
    ),
    (_agentId, facts) => {
      states.push(facts.liveInputState!);
      return {
        status: "committed",
        operationId: `test-operation-${states.length}`,
        reason: states.length === 1 ? "exact_replay" : undefined,
      };
    },
  );
  await route.activate(initialRoute);
  expect(states).toEqual(["armed"]);
  expect(stops).toBe(0);
  await route.freezeAndUnbind();
  expect(states).toEqual(["armed", "frozen"]);
  expect(stops).toBe(1);
});

test("armed streaming route permanently refuses unsupported fallback", async () => {
  const events: string[] = [];
  const route = new ManagedLiveInputRoute(
    "lane-no-downgrade",
    { kind: "lane" },
    initialRoute,
    () => inputAdmission,
    () => subscription(Promise.resolve(), () => { events.push("stop"); }),
    (_agentId, facts) => { events.push(`write:${facts.liveInputState}`); },
  );
  await route.activate(initialRoute);
  const error = await route.beforeFallback(
    {
      fromTarget: "claude-personal",
      fromProvider: "anthropic",
      fromLiveInput: "streaming",
      toTarget: "codex-personal",
      toProvider: "openai",
      toLiveInput: "unsupported",
    },
    async () => { events.push("reserve"); },
  ).catch((cause) => cause);
  expect(error).toBeInstanceOf(ProviderRetrySafeError);
  expect(error.message).toBe(
    "live_input_fallback_refused_after_streaming_route_armed",
  );
  expect(events).toEqual(["write:armed"]);
});

test("streaming sibling fallback freezes before unbind and re-arms a fresh epoch", async () => {
  const events: string[] = [];
  const epochs: string[] = [];
  let subscriptionNumber = 0;
  const route = new ManagedLiveInputRoute(
    "lane-streaming-sibling",
    { kind: "lane" },
    initialRoute,
    () => inputAdmission,
    () => {
      const current = ++subscriptionNumber;
      return subscription(
        Promise.resolve(),
        () => { events.push(`stop:${current}`); },
        async () => { events.push(`drain:${current}`); },
      );
    },
    (_agentId, facts) => {
      events.push(`write:${facts.liveInputState}:${facts.providerTarget}`);
      epochs.push(facts.liveInputEpoch!);
    },
  );
  const initialEpoch = route.initialProjection().liveInputEpoch;
  await route.activate(initialRoute);
  await route.beforeFallback(
    {
      fromTarget: "claude-personal",
      fromProvider: "anthropic",
      fromLiveInput: "streaming",
      toTarget: "claude-work",
      toProvider: "anthropic",
      toLiveInput: "streaming",
    },
    async () => { events.push("reserve"); },
  );
  await route.activate({
    ...initialRoute,
    providerTarget: "claude-work",
  });
  await route.freezeAndUnbind();
  expect(events).toEqual([
    "write:armed:claude-personal",
    "write:frozen:claude-personal",
    "drain:1",
    "stop:1",
    "reserve",
    "write:armed:claude-work",
    "write:frozen:claude-work",
    "drain:2",
    "stop:2",
  ]);
  expect(new Set([initialEpoch, ...epochs]).size).toBe(5);
});

test("two failed durable freeze attempts still unbind the live transport exactly once", async () => {
  let stops = 0;
  let frozenAttempts = 0;
  const route = new ManagedLiveInputRoute(
    "lane-double-freeze-failure",
    { kind: "lane" },
    initialRoute,
    () => inputAdmission,
    () => subscription(Promise.resolve(), () => { stops++; }),
    (_agentId, facts) => {
      if (facts.liveInputState === "frozen") {
        frozenAttempts++;
        throw new Error(`freeze commit failed ${frozenAttempts}`);
      }
    },
  );
  await route.activate(initialRoute);

  const first = await (async () => {
    try {
      await route.freezeAndUnbind();
      return undefined;
    } catch (error) {
      return error;
    }
  })();
  const second = await (async () => {
    try {
      await route.freezeAndUnbind();
      return undefined;
    } catch (error) {
      return error;
    }
  })();

  expect(first).toBeInstanceOf(Error);
  expect((first as Error).message).toBe("freeze commit failed 1");
  expect(second).toBeInstanceOf(Error);
  expect((second as Error).message).toBe("freeze commit failed 2");
  expect(frozenAttempts).toBe(2);
  expect(stops).toBe(1);
});

test("a failed drain retry earns a fresh settlement-feed barrier", async () => {
  const events: string[] = [];
  let feeds = 0;
  const route = new ManagedLiveInputRoute(
    "lane-drain-retry",
    { kind: "lane" },
    initialRoute,
    () => inputAdmission,
    () => {
      const feed = ++feeds;
      return subscription(
        Promise.resolve(),
        () => { events.push(`stop:${feed}`); },
        async () => {
          events.push(`drain:${feed}`);
          if (feed === 1) throw new Error("first drain failed");
        },
      );
    },
    (_agentId, facts) => { events.push(`write:${facts.liveInputState}`); },
  );
  await route.activate(initialRoute);
  await expect(route.freezeAndUnbind()).rejects.toThrow("first drain failed");
  await route.freezeAndUnbind();
  expect(events).toEqual([
    "write:armed",
    "write:frozen",
    "drain:1",
    "stop:1",
    "drain:2",
    "stop:2",
  ]);
});

test("a freeze-write failure retry settles through a fresh feed", async () => {
  const events: string[] = [];
  let feeds = 0;
  let frozenWrites = 0;
  const route = new ManagedLiveInputRoute(
    "lane-freeze-retry",
    { kind: "lane" },
    initialRoute,
    () => inputAdmission,
    () => {
      const feed = ++feeds;
      return subscription(
        Promise.resolve(),
        () => { events.push(`stop:${feed}`); },
        async () => { events.push(`drain:${feed}`); },
      );
    },
    (_agentId, facts) => {
      events.push(`write:${facts.liveInputState}`);
      if (
        facts.liveInputState === "frozen"
        && ++frozenWrites === 1
      ) throw new Error("first freeze write failed");
    },
  );
  await route.activate(initialRoute);
  await expect(route.freezeAndUnbind()).rejects.toThrow(
    "first freeze write failed",
  );
  await route.freezeAndUnbind();
  expect(events).toEqual([
    "write:armed",
    "write:frozen",
    "stop:1",
    "write:frozen",
    "drain:2",
    "stop:2",
  ]);
});
