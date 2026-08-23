import { expect, test } from "bun:test";
import {
  "config-activation-of-json" as activationOfJson,
  "config-entry-active?" as configEntryActive,
  "config-state-text" as configStateText,
} from "../src/bridge/generated/north/bridge/app.js";
import { resolved } from "./bridge-set-members.test";

test("Bridge presents authority-resolved hook activity and every claimant", () => {
  const activation = activationOfJson(resolved());
  const hook = activation.units.find((unit: { name: string }) => unit.name === "worktree-guard");
  expect(configEntryActive(hook)).toBe(true);
  expect(configStateText(hook)).toBe("on · permission on");
  expect(hook.supports).toEqual(["repo-safety", "orchestration"]);
  expect(hook.activationPaths).toEqual([
    ["orchestration", "worktree-guard"],
    ["repo-safety", "worktree-guard"],
  ]);

  const inactive = { ...hook, active: false, permission: "on", state: "on" };
  expect(configEntryActive(inactive)).toBe(false);
  expect(configStateText(inactive)).toBe("off · permission on");
});
