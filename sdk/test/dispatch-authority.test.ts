import { afterAll, beforeAll, describe, expect, test } from "bun:test";
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import {
  admitManagedDispatchAuthority,
  ManagedDispatchAuthorityError,
} from "../src/execution-admission";
import { dispatch } from "../src/dispatch";
import { spawn } from "../src/spawn";

const scratch = mkdtempSync(join(tmpdir(), "north-dispatch-authority-"));
const state = join(scratch, "state", "harness.conf");
const legacy = join(scratch, "legacy.conf");
const priorState = process.env.NORTH_HARNESS_STATE;
const priorLegacy = process.env.NORTH_LEGACY_HARNESS_STATE;
const priorTelemetryPartition = process.env.NORTH_TELEMETRY_PARTITION;
const priorTelemetryPort = process.env.NORTH_TELEMETRY_PORT;

function writeMode(mode: string): void {
  mkdirSync(join(scratch, "state"), { recursive: true });
  writeFileSync(state, `dispatch=${mode}\n`);
}

function environment(mode: string): NodeJS.ProcessEnv {
  writeMode(mode);
  return {
    ...process.env,
    HOME: scratch,
    NORTH_HARNESS_STATE: state,
    NORTH_LEGACY_HARNESS_STATE: legacy,
  };
}

beforeAll(() => {
  process.env.NORTH_HARNESS_STATE = state;
  process.env.NORTH_LEGACY_HARNESS_STATE = legacy;
  // The canonical suite may itself run in a partitioned managed lane. This
  // fixture exercises only dispatch admission, so do not inherit an unrelated
  // wrapper precondition after hermetic-preload removes the ambient telemetry
  // log.
  delete process.env.NORTH_TELEMETRY_PARTITION;
  delete process.env.NORTH_TELEMETRY_PORT;
});

afterAll(() => {
  if (priorState === undefined) delete process.env.NORTH_HARNESS_STATE;
  else process.env.NORTH_HARNESS_STATE = priorState;
  if (priorLegacy === undefined) delete process.env.NORTH_LEGACY_HARNESS_STATE;
  else process.env.NORTH_LEGACY_HARNESS_STATE = priorLegacy;
  if (priorTelemetryPartition === undefined) delete process.env.NORTH_TELEMETRY_PARTITION;
  else process.env.NORTH_TELEMETRY_PARTITION = priorTelemetryPartition;
  if (priorTelemetryPort === undefined) delete process.env.NORTH_TELEMETRY_PORT;
  else process.env.NORTH_TELEMETRY_PORT = priorTelemetryPort;
  rmSync(scratch, { recursive: true, force: true });
});

describe("managed dispatch authority", () => {
  test("managed and auto allow North-managed admission", () => {
    expect(() => admitManagedDispatchAuthority(environment("managed"))).not.toThrow();
    expect(() => admitManagedDispatchAuthority(environment("auto"))).not.toThrow();
  });

  test("native denies North-managed admission", () => {
    expect(() => admitManagedDispatchAuthority(environment("native")))
      .toThrow("managed_dispatch_denied_by_native");
  });

  test("unknown persisted values fail closed with the parser diagnostic", () => {
    expect(() => admitManagedDispatchAuthority(environment("surprise")))
      .toThrow(/managed_dispatch_authority_unavailable: invalid dispatch mode "surprise"/);
  });

  test("spawn and dispatch reject native before request or provider admission", async () => {
    writeMode("native");
    await expect(spawn({} as any))
      .rejects.toBeInstanceOf(ManagedDispatchAuthorityError);
    await expect(dispatch("not-a-thread", {} as any))
      .rejects.toBeInstanceOf(ManagedDispatchAuthorityError);
  });
});
