import { expect, test } from "bun:test";
import {
  NativeCommandActivityAccumulator, NORTH_BINARY_PROBE_SCRIPT,
} from "../src/native-command-activity";

const cwd = "/repo";
const north = "/nix/store/north/bin/north";
const probe = `/bin/bash -c '${NORTH_BINARY_PROBE_SCRIPT}'`;

function completion(id: string, command = probe, output = `${north}\n${north}\n`) {
  return {
    id, command, cwd, source: "agent" as const, status: "completed" as const,
    aggregatedOutput: output, exitCode: 0,
  };
}

test("native command evidence is unknown until exact turn settlement", () => {
  const activity = new NativeCommandActivityAccumulator(cwd, north);
  expect(activity.snapshot()).toMatchObject({
    coverage: "unknown", northBinaryProbe: "not_observed", completions: [],
  });
  expect(activity.start("turn-1:command-1")).toBe(true);
  expect(activity.observe(completion("turn-1:command-1"))).toBe(true);
  expect(activity.snapshot()).toMatchObject({
    coverage: "unknown", northBinaryProbe: "not_observed", completions: [],
  });
  expect(activity.complete()).toBe(true);
  expect(activity.snapshot()).toMatchObject({
    coverage: "exact", totalCommands: 1, successfulCommands: 1,
    northBinaryProbe: "passed",
  });
});

test("unfinished continuation cannot preserve an earlier passing probe", () => {
  const activity = new NativeCommandActivityAccumulator(cwd, north);
  activity.start("turn-1:command-1");
  activity.observe(completion("turn-1:command-1"));
  expect(activity.complete()).toBe(true);
  expect(activity.snapshot().northBinaryProbe).toBe("passed");
  activity.reopen();
  activity.start("turn-2:command-1");
  expect(activity.snapshot()).toMatchObject({
    coverage: "unknown", northBinaryProbe: "not_observed", completions: [],
  });
  expect(activity.complete()).toBe(false);
  expect(activity.snapshot()).toMatchObject({
    coverage: "partial", northBinaryProbe: "failed",
  });
});

test("native command evidence stays bounded and partial on overflow", () => {
  const activity = new NativeCommandActivityAccumulator(cwd, north);
  for (let index = 0; index < 33; index++) {
    const id = `turn-1:command-${index}`;
    activity.start(id);
    activity.observe(completion(id, `/bin/echo command-${index}`, `output-${index}\n`));
  }
  expect(activity.complete()).toBe(true);
  expect(activity.snapshot()).toMatchObject({
    coverage: "partial", totalCommands: 33, truncatedCommands: 1,
    northBinaryProbe: "failed",
  });
  expect(activity.snapshot().completions).toHaveLength(32);
});
