// Standalone proof of the SDK real-time interrupt: an agent in streaming-input mode
// that a peer can ping mid-run — the ping is injected into the LIVE query, no re-arm.
//
//   AGENT_ID=demo1 MAX_PINGS=1 bun run src/reachable-demo.ts
//   # then, from anywhere:
//   bb cli/north-listen... no — to PING it:
//   bb <north-runtime>/cli/msg-cli.clj 7977 send tester demo1 "URGENT" "look at flake.bnix"
import { resolve } from "node:path";
import { harnessOptions } from "./harness";
import { inputChannel, subscribeFeed } from "./coordination";
import { makeExecutionFold } from "./execution-fold";
import { anthropicProvider } from "./providers/anthropic";
import { resolveStrugglePolicy } from "./struggle";
import { StreamWriter } from "./stream-writer";
import { newRunId } from "./telemetry";
import {
  WireEventWriter, wireQueryRoute, wireRunId, type WireQuery, type WireTerminationInput,
} from "./wire";

const self = process.env.AGENT_ID ?? `sdk-reachable-${Date.now().toString(36).slice(-6)}`;
const maxPings = Number(process.env.MAX_PINGS ?? 1);
const repo = resolve(import.meta.dir, "../..");

const ch = inputChannel(
  `You are north coordination agent "${self}". Reply with ONE short line acknowledging you are live and listening. ` +
  `Then stay idle. When you receive a message tagged [north real-time ping ...], that is a peer reaching you in ` +
  `real time mid-run — reply with ONE line: who pinged you and what they want.`,
);

let results = 0;
const stop = subscribeFeed(self, (m) => {
  console.log(`\n>>> peer ping arriving — injecting into the RUNNING agent:\n${m}\n`);
  ch.push(m);
});

const writer = new WireEventWriter({ runId: wireRunId(newRunId(self)) });
const stream = await StreamWriter.open(self);
const fold = makeExecutionFold(resolveStrugglePolicy("worker", {}));
let persisted = 0;
let failure: unknown;
let q: WireQuery | undefined;
const retainFailure = (error: unknown, message: string): void => {
  failure = failure === undefined ? error : new AggregateError([failure, error], message);
};
const flush = async (): Promise<void> => {
  const events = writer.events();
  while (persisted < events.length) {
    await stream.writeWireEvent(events[persisted]!);
    persisted += 1;
  }
};
try {
  const started = writer.append({ kind: "run.started", lifecycle: "running", owner: self });
  fold.observe(started);
  await flush();
  const options = harnessOptions({ self, model: "haiku", extraTools: ["Bash"] });
  q = anthropicProvider.query({
    input: ch.stream(),
    options,
    context: {
      writer,
      route: wireQueryRoute({
        model: { provider: "anthropic", tier: "economy", capabilityClass: "unknown" },
        effort: options.effort ?? "low",
        attempt: 1,
      }),
    },
  });

  console.log(`[reachable] @${self} live. Ping it:\n  bb ${repo}/cli/msg-cli.clj 7977 send tester ${self} "URGENT" "<msg>"\n`);
  for await (const event of q) {
    const observation = fold.observe(event);
    await flush();
    if (event.essential && event.kind === "message.recorded"
        && event.role === "assistant" && event.stage === "completed") {
      const output = observation.state.lastCompletedAssistantOutput;
      if (output?.trim()) console.log(`[${self}] ${output.trim()}`);
    }
    if (event.essential && event.kind === "model-call.completed") {
      results++;
      if (event.status !== "succeeded" || results >= 1 + maxPings) {
        ch.end();
        break;
      }
    }
  }
} catch (error) {
  retainFailure(error, "reachable demo execution failed");
} finally {
  ch.end();
  try { await q?.close?.(); }
  catch (error) { retainFailure(error, "reachable demo execution and query close failed"); }
  try { await stop(); }
  catch (error) { retainFailure(error, "reachable demo execution and feed close failed"); }
  const snapshot = writer.snapshot();
  if (snapshot && (snapshot.lifecycle === "running" || snapshot.lifecycle === "waiting")) {
    const terminal: WireTerminationInput = failure === undefined
      ? { lifecycle: "completed", reason: { code: "completed" } }
      : {
          lifecycle: "failed",
          reason: { code: "provider_error", detail: "provider_error" },
        };
    try {
      for (const event of writer.terminate(terminal)) fold.observe(event);
      await flush();
    } catch (error) {
      retainFailure(error, "reachable demo execution and terminal persistence failed");
    }
  }
  try {
    await stream.close();
  } catch (error) {
    retainFailure(error, "reachable demo execution and stream close failed");
  }
}
if (failure !== undefined) throw failure;
console.log(`[reachable] done after ${results} turn(s)`);
