import { afterEach, expect, test } from "bun:test";
import {
  isTelemetrySubject,
  nativeRouteForSubject,
  telemetryPartitionEnabled,
} from "../src/coord-wire";

const original = {
  NORTH_PORT: process.env.NORTH_PORT,
  NORTH_TELEMETRY_PARTITION: process.env.NORTH_TELEMETRY_PARTITION,
  NORTH_TELEMETRY_PORT: process.env.NORTH_TELEMETRY_PORT,
  FRAM_SPACE_ID: process.env.FRAM_SPACE_ID,
  NORTH_TELEMETRY_SPACE_ID: process.env.NORTH_TELEMETRY_SPACE_ID,
};

afterEach(() => {
  for (const [key, value] of Object.entries(original)) {
    if (value === undefined) delete process.env[key];
    else process.env[key] = value;
  }
});

test("telemetry subjects route to their FRAMRPC space", () => {
  process.env.NORTH_PORT = "7977";
  process.env.NORTH_TELEMETRY_PARTITION = "1";
  process.env.NORTH_TELEMETRY_PORT = "7978";
  process.env.FRAM_SPACE_ID = "north-coordination";
  process.env.NORTH_TELEMETRY_SPACE_ID = "north-telemetry";

  expect(telemetryPartitionEnabled()).toBe(true);
  for (const subject of [
    "@run:019fa54e",
    "@run:019fa54e:event:00000000",
    "@session:019fa54e",
    "@mine:019fa54e",
    "@guard_denial:019fa54e",
  ]) {
    expect(isTelemetrySubject(subject)).toBe(true);
    expect(nativeRouteForSubject(subject)).toEqual({
      port: 7978,
      spaceId: "north-telemetry",
    });
  }
  expect(nativeRouteForSubject("@agent:019fa54e")).toEqual({
    port: 7977,
    spaceId: "north-coordination",
  });
});

test("disabled partition routes every subject to the coordination space", () => {
  process.env.NORTH_PORT = "7977";
  process.env.NORTH_TELEMETRY_PARTITION = "0";
  process.env.NORTH_TELEMETRY_PORT = "7978";
  process.env.FRAM_SPACE_ID = "north-coordination";
  process.env.NORTH_TELEMETRY_SPACE_ID = "north-telemetry";

  expect(telemetryPartitionEnabled()).toBe(false);
  expect(nativeRouteForSubject("@run:019fa54e")).toEqual({
    port: 7977,
    spaceId: "north-coordination",
  });
});

test("enabled partition requires a telemetry port", () => {
  process.env.NORTH_TELEMETRY_PARTITION = "1";
  delete process.env.NORTH_TELEMETRY_PORT;
  process.env.NORTH_TELEMETRY_SPACE_ID = "north-telemetry";
  expect(() => nativeRouteForSubject("@run:019fa54e")).toThrow(
    "NORTH_TELEMETRY_PORT must be an integer",
  );
});
