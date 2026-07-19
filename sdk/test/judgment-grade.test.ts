import { afterEach, expect, test } from "bun:test";
import { chmodSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { spawnSync } from "node:child_process";
import {
  adHocJudgmentGrade, judgmentGradeFromThreadFacts,
  parseJudgmentGrade, requireJudgmentGrade,
} from "../src/judgment-grade";

const temporary: string[] = [];
afterEach(() => {
  for (const path of temporary.splice(0)) rmSync(path, { recursive: true, force: true });
});

test("judgment grade parser and admission snapshots are exact", () => {
  for (const grade of ["s", "m", "l"] as const) {
    expect(parseJudgmentGrade(grade)).toBe(grade);
    expect(requireJudgmentGrade(grade)).toBe(grade);
    expect(judgmentGradeFromThreadFacts([
      { predicate: "judgment_grade", value: grade },
    ])).toEqual({ grade, status: "valid", source: "thread" });
  }
  for (const invalid of ["S", "medium", " s", "s ", "", null, 1]) {
    expect(parseJudgmentGrade(invalid)).toBeUndefined();
    expect(() => requireJudgmentGrade(invalid)).toThrow("exactly one of");
  }
  expect(judgmentGradeFromThreadFacts([])).toEqual({
    status: "unavailable", source: "thread",
  });
  expect(judgmentGradeFromThreadFacts([
    { predicate: "judgment_grade", value: "legacy-medium" },
  ])).toEqual({ status: "invalid", source: "thread" });
  expect(judgmentGradeFromThreadFacts([
    { predicate: "judgment_grade", value: "s" },
    { predicate: "judgment_grade", value: "l" },
  ])).toEqual({ status: "invalid", source: "thread" });
  expect(adHocJudgmentGrade()).toEqual({ status: "unavailable", source: "ad-hoc" });
});

test("admission snapshots cannot be mutated after the dispatcher freezes them", () => {
  const valid = judgmentGradeFromThreadFacts([
    { predicate: "judgment_grade", value: "s" },
  ]);
  expect(Object.isFrozen(valid)).toBe(true);
  expect(() => { (valid as any).grade = "l"; }).toThrow(TypeError);
  expect(valid).toEqual({ grade: "s", status: "valid", source: "thread" });
  const adHoc = adHocJudgmentGrade();
  expect(Object.isFrozen(adHoc)).toBe(true);
  expect(() => { (adHoc as any).status = "valid"; }).toThrow(TypeError);
});

test("generic write boundaries reserve judgment_grade for ambient humans and orchestrators", () => {
  const directory = mkdtempSync(join(tmpdir(), "north-judgment-boundary-"));
  temporary.push(directory);
  const calls = join(directory, "calls");
  const fakeBb = join(directory, "bb");
  const framDir = join(directory, "fram-bin");
  const fakeFram = join(framDir, "fram");
  mkdirSync(framDir, { recursive: true });
  writeFileSync(fakeBb, "#!/usr/bin/env bash\nprintf '@thread-probe\\n'\n");
  writeFileSync(fakeFram, `#!/usr/bin/env bash\nprintf '%s\\n' "$*" >> ${JSON.stringify(calls)}\n`);
  chmodSync(fakeBb, 0o755);
  chmodSync(fakeFram, 0o755);
  const north = resolve(import.meta.dir, "../../bin/north");
  const baseEnv = {
    ...process.env,
    NORTH_BB: fakeBb,
    NORTH_BUN: process.execPath,
    FRAM_BIN: framDir,
    FRAM_OUT: directory,
  };

  for (const verb of ["tell", "set", "retract"] as const) {
    const result = spawnSync(north, [verb, "thread-probe", "judgment_grade", "s"], {
      encoding: "utf8", env: { ...baseEnv, AGENT_TOPOLOGY: "worker" },
    });
    expect(result.status).not.toBe(0);
    expect(result.stderr).toContain("dispatcher/orchestrator-owned");
  }
  expect(() => readFileSync(calls, "utf8")).toThrow();

  for (const invalid of ["S", "medium", " s", "s ", ""]) {
    const result = spawnSync(north, ["tell", "thread-probe", "judgment_grade", invalid], {
      encoding: "utf8", env: { ...baseEnv, AGENT_TOPOLOGY: "" },
    });
    expect(result.status).not.toBe(0);
    expect(result.stderr).toContain("must be exactly one of");
  }

  for (const [topology, verb, value] of [
    ["", "tell", "s"],
    ["orchestrator", "tell", "l"],
    ["orchestrator", "set", "m"],
    ["", "retract", "legacy-medium"],
  ] as const) {
    const result = spawnSync(north, [verb, "thread-probe", "judgment_grade", value], {
      encoding: "utf8", env: { ...baseEnv, AGENT_TOPOLOGY: topology },
    });
    expect(result.status).toBe(0);
  }
  const written = readFileSync(calls, "utf8");
  expect(written).toContain("tell thread-probe judgment_grade s");
  expect(written).toContain("tell thread-probe judgment_grade l");
  expect(written).toContain("set thread-probe judgment_grade m");
  expect(written).toContain("untell thread-probe judgment_grade legacy-medium");
});
