import { afterAll, beforeAll, beforeEach, describe, expect, test } from "bun:test";
import { createHash } from "node:crypto";
import { execFileSync } from "node:child_process";
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import {
  admitManagedDispatchAuthority,
  DeliveryLivenessDispatchError,
  managedNorthMcpEnvironment,
  ManagedDispatchAuthorityError,
} from "../src/execution-admission";
import { deliveryDispatchClassForRouting } from "../src/delivery-liveness";
import { dispatch } from "../src/dispatch";
import { spawn } from "../src/spawn";

const scratch = mkdtempSync(join(tmpdir(), "north-dispatch-authority-"));
const state = join(scratch, "state", "harness.conf");
const activation = join(scratch, "delivery-liveness-required");
const firnRepo = join(scratch, "code/nixos-config/main");
mkdirSync(firnRepo, { recursive: true });
execFileSync("git", ["init", "-q", "-b", "main", firnRepo]);
execFileSync("git", ["-C", firnRepo, "-c", "user.name=test", "-c",
  "user.email=test@example.invalid", "commit", "--allow-empty", "-qm", "base"]);
const firnRevision = execFileSync(
  "git", ["-C", firnRepo, "rev-parse", "HEAD"], { encoding: "utf8" },
).trim();
const priorState = process.env.NORTH_HARNESS_STATE;
const priorHome = process.env.HOME;
const priorDeliveryLivenessRequired = process.env.NORTH_DELIVERY_LIVENESS_REQUIRED;
const priorDeliveryLivenessActivationPath = process.env.NORTH_DELIVERY_LIVENESS_ACTIVATION_PATH;
const priorTelemetryPartition = process.env.NORTH_TELEMETRY_PARTITION;
const priorTelemetryPort = process.env.NORTH_TELEMETRY_PORT;

function writeMode(mode: string): void {
  mkdirSync(join(scratch, "state"), { recursive: true });
  writeFileSync(state, `dispatch=${mode}\n`);
}

function writeRawLiveness(source: string): void {
  const path = join(scratch, ".local/state/firn/delivery-liveness.json");
  mkdirSync(join(scratch, ".local/state/firn"), { recursive: true });
  writeFileSync(path, source);
  writeFileSync(`${path}.sha256`,
    `${createHash("sha256").update(source).digest("hex")}  delivery-liveness.json\n`);
}

function writeLiveness(overrides: Record<string, unknown> = {}): void {
  writeRawLiveness(`${JSON.stringify({
    version: 1,
    observed_at: new Date().toISOString(),
    freshness_seconds: 60,
    buildable: true,
    failing_check: null,
    inputs: { nixos_config: firnRevision },
    firn: {
      current: "/home/tom/code/nixos-config/main/dotfiles/bin/firn",
      candidate: `/nix/store/${"b".repeat(32)}-candidate/bin/firn`,
    },
    ...overrides,
  })}\n`);
}

function environment(mode: string): NodeJS.ProcessEnv {
  writeMode(mode);
  writeFileSync(activation, "1\n");
  return {
    ...process.env,
    HOME: scratch,
    NORTH_DELIVERY_LIVENESS_REQUIRED: "1",
    NORTH_DELIVERY_LIVENESS_ACTIVATION_PATH: activation,
    NORTH_HARNESS_STATE: state,
  };
}

beforeAll(() => {
  process.env.NORTH_HARNESS_STATE = state;
  process.env.HOME = scratch;
  process.env.NORTH_DELIVERY_LIVENESS_REQUIRED = "1";
  process.env.NORTH_DELIVERY_LIVENESS_ACTIVATION_PATH = activation;
  writeFileSync(activation, "1\n");
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
  if (priorDeliveryLivenessActivationPath === undefined)
    delete process.env.NORTH_DELIVERY_LIVENESS_ACTIVATION_PATH;
  else process.env.NORTH_DELIVERY_LIVENESS_ACTIVATION_PATH = priorDeliveryLivenessActivationPath;
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

  test("a live absolute Firn path admits while a relative path fails closed", () => {
    expect(() => admitManagedDispatchAuthority(environment("managed"))).not.toThrow();
    writeLiveness({
      firn: { current: "dotfiles/bin/firn", candidate: null },
    });
    expect(() => admitManagedDispatchAuthority(environment("managed")))
      .toThrow("delivery_liveness_authority_malformed:firn.current");
  });

  test("native denies North-managed admission", () => {
    expect(() => admitManagedDispatchAuthority(environment("native")))
      .toThrow("managed_dispatch_denied_by_native");
  });

  test("an unactivated or rolled-back source does not change managed dispatch", () => {
    writeLiveness({ buildable: false, failing_check: "firn build" });
    const unactivated = { ...environment("managed"), NORTH_DELIVERY_LIVENESS_REQUIRED: "0" };
    delete unactivated.NORTH_DELIVERY_LIVENESS_ACTIVATION_PATH;
    expect(() => admitManagedDispatchAuthority(unactivated)).not.toThrow();
    const rolledBack = { ...unactivated, NORTH_DELIVERY_LIVENESS_REQUIRED: "1" };
    expect(() => admitManagedDispatchAuthority(rolledBack)).not.toThrow();
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

    writeLiveness({ observed_at: "2999-01-01T00:00:00.000Z" });
    expect(() => admitManagedDispatchAuthority(environment("managed")))
      .toThrow("delivery_liveness_authority_stale");

    writeRawLiveness("{not json}\n");
    expect(() => admitManagedDispatchAuthority(environment("managed")))
      .toThrow("delivery_liveness_authority_malformed:json");
  });

  test("feature admission verifies the atomic fact identity and calendar time", () => {
    const path = join(scratch, ".local/state/firn/delivery-liveness.json");
    writeFileSync(`${path}.sha256`, `${"0".repeat(64)}  delivery-liveness.json\n`);
    expect(() => admitManagedDispatchAuthority(environment("managed")))
      .toThrow("delivery_liveness_authority_input_changed");

    rmSync(`${path}.sha256`);
    expect(() => admitManagedDispatchAuthority(environment("managed")))
      .toThrow("delivery_liveness_authority_missing:sha256");

    writeLiveness();
    writeFileSync(`${path}.sha256`, "not a digest\n");
    expect(() => admitManagedDispatchAuthority(environment("managed")))
      .toThrow("delivery_liveness_authority_malformed:sha256");

    writeLiveness({ observed_at: "2026-02-31T00:00:00Z" });
    expect(() => admitManagedDispatchAuthority(environment("managed")))
      .toThrow("delivery_liveness_authority_malformed:observed_at");

    writeLiveness({ inputs: { nixos_config: "f".repeat(40) } });
    expect(() => admitManagedDispatchAuthority(environment("managed")))
      .toThrow("delivery_liveness_authority_input_changed");
  });

  test("a switched generation cannot be downgraded by ambient activation", () => {
    writeFileSync(activation, "1\n");
    writeLiveness({ buildable: false, failing_check: "firn build" });
    const downgraded = { ...environment("managed"), NORTH_DELIVERY_LIVENESS_REQUIRED: "0" };
    expect(() => admitManagedDispatchAuthority(downgraded, "feature", activation))
      .toThrow("delivery_liveness_build_not_buildable:firn build");
  });

  test("success, failure, and restart observations are reread without cached oscillation", () => {
    expect(() => admitManagedDispatchAuthority(environment("managed"))).not.toThrow();
    writeLiveness({ buildable: false, failing_check: "candidate_firn_build" });
    expect(() => admitManagedDispatchAuthority(environment("managed")))
      .toThrow("delivery_liveness_build_not_buildable:candidate_firn_build");
    writeLiveness();
    expect(() => admitManagedDispatchAuthority(environment("managed"))).not.toThrow();
  });

  test("an explicitly classified repair remains available when feature authority is false", () => {
    writeLiveness({ buildable: false, failing_check: "firn build" });
    expect(() => admitManagedDispatchAuthority(environment("managed"), "repair")).not.toThrow();
    expect(() => admitManagedDispatchAuthority(environment("managed")))
      .toThrow(DeliveryLivenessDispatchError);
  });

  test("only fully pinned explicit-human routing bypasses false feature authority", () => {
    writeLiveness({ buildable: false, failing_check: "firn build" });
    const evidence = {
      reasonCode: "explicit-human-request",
      pins: [
        { kind: "provider", value: "openai" },
        { kind: "account", value: "codex-work" },
        { kind: "model", value: "gpt-5.6-sol" },
      ],
    };
    const route = { provider: "openai", target: "codex-work", model: "gpt-5.6-sol" };
    const dispatchClass = deliveryDispatchClassForRouting(evidence, route, environment("managed"));
    expect(dispatchClass).toBe("explicit-human");
    expect(() => admitManagedDispatchAuthority(environment("managed"), dispatchClass)).not.toThrow();
    expect(deliveryDispatchClassForRouting(
      { ...evidence, pins: evidence.pins.slice(0, 2) }, route, environment("managed"),
    )).toBe("feature");
    expect(() => admitManagedDispatchAuthority(environment("managed")))
      .toThrow(DeliveryLivenessDispatchError);
  });

  test("sealed nested MCP propagation preserves activation and explicit repair class", () => {
    const nested = managedNorthMcpEnvironment({
      HOME: scratch,
      NORTH_DELIVERY_LIVENESS_REQUIRED: "1",
      NORTH_DELIVERY_DISPATCH_CLASS: "repair",
    });
    expect(nested.NORTH_DELIVERY_LIVENESS_REQUIRED).toBe("1");
    expect(nested.NORTH_DELIVERY_DISPATCH_CLASS).toBe("repair");
  });

  test("repair classification is closed and unavailable Store selectors cannot open feature admission", () => {
    const invalid = {
      ...environment("managed"),
      NORTH_DELIVERY_DISPATCH_CLASS: "feature-or-repair",
    };
    expect(() => admitManagedDispatchAuthority(invalid))
      .toThrow("delivery_liveness_dispatch_class_invalid");

    writeLiveness({ buildable: false, failing_check: "toplevel_build" });
    const unavailableStore = {
      ...environment("managed"),
      NORTH_PORT: "1",
      BEAGLE_STORE_SERVER_PORT: "1",
      BEAGLE_STORE_SPACE_ID: "unavailable",
    };
    expect(() => admitManagedDispatchAuthority(unavailableStore))
      .toThrow("delivery_liveness_build_not_buildable:toplevel_build");
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
