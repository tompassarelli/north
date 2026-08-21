import { afterAll, beforeAll, beforeEach, describe, expect, test } from "bun:test";
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import {
  admitManagedDispatchAuthority,
  DeliveryLivenessDispatchError,
  ManagedDispatchAuthorityError,
} from "../src/execution-admission";
import { dispatch } from "../src/dispatch";
import { spawn } from "../src/spawn";

const scratch = mkdtempSync(join(tmpdir(), "north-dispatch-authority-"));
const state = join(scratch, "state", "harness.conf");
const priorState = process.env.NORTH_HARNESS_STATE;
const priorHome = process.env.HOME;
const priorDeliveryLivenessRequired = process.env.NORTH_DELIVERY_LIVENESS_REQUIRED;
const priorTelemetryPartition = process.env.NORTH_TELEMETRY_PARTITION;
const priorTelemetryPort = process.env.NORTH_TELEMETRY_PORT;

function writeMode(mode: string): void {
  mkdirSync(join(scratch, "state"), { recursive: true });
  writeFileSync(state, `dispatch=${mode}\n`);
}

function writeLiveness(overrides: Record<string, unknown> = {}): void {
  const path = join(scratch, ".local/state/firn/delivery-liveness.json");
  mkdirSync(join(scratch, ".local/state/firn"), { recursive: true });
  writeFileSync(path, `${JSON.stringify({
    version: 1,
    observed_at: new Date().toISOString(),
    freshness_seconds: 60,
    buildable: true,
    failing_check: null,
    inputs: { nixos_config: "a".repeat(40) },
    firn: { current: "/nix/store/current", candidate: "/nix/store/candidate" },
    ...overrides,
  })}\n`);
}

function environment(mode: string): NodeJS.ProcessEnv {
  writeMode(mode);
  return {
    ...process.env,
    HOME: scratch,
    NORTH_DELIVERY_LIVENESS_REQUIRED: "1",
    NORTH_HARNESS_STATE: state,
  };
}

beforeAll(() => {
  process.env.NORTH_HARNESS_STATE = state;
  process.env.HOME = scratch;
  process.env.NORTH_DELIVERY_LIVENESS_REQUIRED = "1";
  // The canonical suite may itself run in a partitioned managed lane. This
  // fixture exercises only dispatch admission, so do not inherit an unrelated
  // wrapper precondition after hermetic-preload removes the ambient telemetry
  // log.
  delete process.env.NORTH_TELEMETRY_PARTITION;
  delete process.env.NORTH_TELEMETRY_PORT;
});

beforeEach(() => writeLiveness());

afterAll(() => {
  if (priorState === undefined) delete process.env.NORTH_HARNESS_STATE;
  else process.env.NORTH_HARNESS_STATE = priorState;
  if (priorHome === undefined) delete process.env.HOME;
  else process.env.HOME = priorHome;
  if (priorDeliveryLivenessRequired === undefined) delete process.env.NORTH_DELIVERY_LIVENESS_REQUIRED;
  else process.env.NORTH_DELIVERY_LIVENESS_REQUIRED = priorDeliveryLivenessRequired;
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

  test("an unactivated source does not change managed dispatch", () => {
    writeLiveness({ buildable: false, failing_check: "firn build" });
    const unactivated = { ...environment("managed"), NORTH_DELIVERY_LIVENESS_REQUIRED: "0" };
    expect(() => admitManagedDispatchAuthority(unactivated)).not.toThrow();
  });

  test("unknown persisted values fail closed with the parser diagnostic", () => {
    expect(() => admitManagedDispatchAuthority(environment("surprise")))
      .toThrow("managed_dispatch_authority_unavailable: north: invalid dispatch mode: surprise");
  });

  test("feature admission consumes only a fresh, true Firn floor fact", () => {
    const path = join(scratch, ".local/state/firn/delivery-liveness.json");
    expect(() => admitManagedDispatchAuthority(environment("managed"))).not.toThrow();

    writeLiveness({ buildable: false, failing_check: "firn build" });
    expect(() => admitManagedDispatchAuthority(environment("managed")))
      .toThrow("delivery_liveness_build_not_buildable:firn build");

    rmSync(path);
    expect(() => admitManagedDispatchAuthority(environment("managed")))
      .toThrow("delivery_liveness_authority_missing");

    writeLiveness({ observed_at: "2020-01-01T00:00:00.000Z" });
    expect(() => admitManagedDispatchAuthority(environment("managed")))
      .toThrow("delivery_liveness_authority_stale");

    writeFileSync(path, "{not json}\n");
    expect(() => admitManagedDispatchAuthority(environment("managed")))
      .toThrow("delivery_liveness_authority_malformed:json");
  });

  test("an explicitly classified repair remains available when feature authority is false", () => {
    writeLiveness({ buildable: false, failing_check: "firn build" });
    expect(() => admitManagedDispatchAuthority(environment("managed"), "repair")).not.toThrow();
    expect(() => admitManagedDispatchAuthority(environment("managed")))
      .toThrow(DeliveryLivenessDispatchError);
  });

  test("spawn and dispatch reject native before request or provider admission", async () => {
    writeMode("native");
    await expect(spawn({} as any))
      .rejects.toBeInstanceOf(ManagedDispatchAuthorityError);
    await expect(dispatch("not-a-thread", {} as any))
      .rejects.toBeInstanceOf(ManagedDispatchAuthorityError);
  });

  test("spawn and dispatch reject a false feature authority before request or provider admission", async () => {
    writeMode("managed");
    writeLiveness({ buildable: false, failing_check: "firn build" });
    await expect(spawn({} as any))
      .rejects.toBeInstanceOf(DeliveryLivenessDispatchError);
    await expect(dispatch("not-a-thread", {} as any))
      .rejects.toBeInstanceOf(DeliveryLivenessDispatchError);
  });
});
