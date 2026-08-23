import { expect, test } from "bun:test";
import {
  "config-activation-of-json" as activationOfJson,
  "config-header-roles" as configHeaderRoles,
  "config-row-parts" as configRowParts,
  "config-section-title" as configSectionTitle,
  "config-state-text" as configStateText,
  "config-view-rows" as configViewRows,
} from "../src/bridge/generated/north/bridge/app.js";
import { resolved } from "./bridge-set-members.test";

test("globals renders exactly Sets, Skills, Hooks in authority order", () => {
  const activation = activationOfJson(resolved());
  const entries = configViewRows(activation.units, "globals");
  const roles = entries.map((entry: { kind: string }) => configHeaderRoles(entry.kind)[0]);
  expect([...new Set(roles)]).toEqual(["set", "skill", "hook"]);
  expect([...new Set(roles)].map(configSectionTitle)).toEqual(["SETS", "SKILLS", "HOOKS"]);

  const orchestration = entries.find((entry: { name: string }) => entry.name === "orchestration");
  const state = configStateText(orchestration, activation.units, [], true, false);
  expect(configRowParts(orchestration, [], false, "set", state, 100)).toMatchObject({
    name: "orchestration",
    state: "on · permission on",
    detail: "2 members",
  });
});
