import { expect, test } from "bun:test";
import { BoxRenderable, TextRenderable } from "@opentui/core";
import { createTestRenderer } from "@opentui/core/testing";
import { chmodSync, mkdtempSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import {
  "agent-cell-text!" as agentCellText,
  "agent-route-text!" as agentRouteText,
  "agent-row-text!" as agentRowText,
  "normalize-agents" as normalizeAgents,
  "reconcile-agent-selection!" as reconcileAgentSelection,
  "refresh!" as refresh,
  "render-detail-panel!" as renderDetailPanel,
  "roster-text!" as rosterText,
  "selected-agent-id" as selectedAgentId,
  "submit-input!" as submitInput,
} from "../src/bridge/generated/north/bridge/app.js";
import {
  "->Agent" as Agent,
  "make-model" as makeModel,
  "replace-projection" as replaceProjection,
  "select-agent" as selectAgent,
  "snapshot" as modelSnapshot,
  "upsert-agent" as upsertAgent,
} from "../src/bridge/generated/north/bridge/model.js";

Object.defineProperty(process.stdout, "columns", { value: 180, configurable: true });
Object.defineProperty(process.stdout, "rows", { value: 40, configurable: true });

function agent(id: string, name: string, status = "working", task = "") {
  return Agent(id, name, status, task, "", "", "", "", "", "", "", "", "");
}

async function snapshotOf(content: unknown, width: number, height: number) {
  const { renderer, renderOnce, captureCharFrame } = await createTestRenderer({ width, height });
  const root = new BoxRenderable(renderer, { width: "100%", height: "100%" });
  const text = new TextRenderable(renderer, {
    width: "100%", height: "100%", wrapMode: "none", truncate: true,
  });
  root.add(text);
  renderer.root.add(root);
  text.content = content;
  await renderOnce();
  const snapshot = captureCharFrame();
  renderer.destroy();
  return snapshot;
}

test("agent reorder preserves the highlighted identity and composer target", async () => {
  const alpha = agent("alpha", "Alpha");
  const beta = agent("beta", "Beta");
  const gamma = agent("gamma", "Gamma");
  let model = makeModel("list");
  for (const row of [alpha, beta, gamma]) model = upsertAgent(model, row);

  const priorId = selectedAgentId(modelSnapshot(model), 1);
  const runtime = {
    agentIndex: 1,
    model: selectAgent(replaceProjection(model, [gamma, alpha, beta], [], []), "alpha"),
  };
  expect(reconcileAgentSelection(runtime, priorId)).toBe("beta");

  const state = modelSnapshot(runtime.model);
  const target = selectedAgentId(state, runtime.agentIndex);
  const snapshot = await snapshotOf(
    `${rosterText(state, runtime.agentIndex, "", false)}\ncomposer target: ${target}`,
    72,
    6,
  );
  expect(runtime.agentIndex).toBe(2);
  expect(state.selected_agent).toBe("beta");
  expect(target).toBe("beta");
  expect(snapshot).toContain("› Beta");
  expect(snapshot).toContain("composer target: beta");
  expect(snapshot).not.toContain("› Alpha");
});

test("refresh preserves the selected submit target and falls back when it disappears", async () => {
  const root = mkdtempSync(join(tmpdir(), "north-bridge-agent-selection-"));
  const north = join(root, "north");
  const roster = join(root, "roster.json");
  const messages = join(root, "messages.tsv");
  const priorNorthBin = process.env.NORTH_BIN;
  try {
    await Bun.write(
      north,
      `#!/bin/sh
case "$1" in
  agents) exec cat ${JSON.stringify(roster)} ;;
  json) printf '[]\\n' ;;
  msg) printf '%s\\t%s\\n' "$2" "$3" >> ${JSON.stringify(messages)} ;;
esac
`,
    );
    chmodSync(north, 0o755);
    process.env.NORTH_BIN = north;

    let model = makeModel("list");
    for (const row of [agent("alpha", "Alpha"), agent("beta", "Beta"), agent("gamma", "Gamma")]) {
      model = upsertAgent(model, row);
    }
    model = selectAgent(model, "beta");
    const runtime = {
      activeView: "list",
      agentIndex: 1,
      bridgeExecutions: new Set<string>(),
      collapsedListConditions: new Set<string>(),
      conversation: [] as unknown[],
      disposed: false,
      snapshot: "agents",
      itemSequence: 0,
      lastSubmitted: "",
      model,
      paletteIndex: 0,
      render() {},
      renderConversation() {},
      spinnerIndex: 0,
      spinnerTimer: null,
      supervisorId: "",
      workIndex: 0,
      working: false,
      workingLabel: "",
      workingSince: 0,
    };
    const ui = { composerInput: { value: "" } };

    await Bun.write(roster, JSON.stringify({ agents: [
      { control_id: "gamma", display_handle: "Gamma" },
      { control_id: "alpha", display_handle: "Alpha" },
      { control_id: "beta", display_handle: "Beta" },
    ] }));
    await refresh(runtime);
    expect(runtime.agentIndex).toBe(2);
    expect(modelSnapshot(runtime.model).selected_agent).toBe("beta");
    await submitInput(runtime, ui, "first message");

    await Bun.write(roster, JSON.stringify({ agents: [
      { control_id: "gamma", display_handle: "Gamma" },
      { control_id: "alpha", display_handle: "Alpha" },
    ] }));
    await refresh(runtime);
    expect(runtime.agentIndex).toBe(1);
    expect(modelSnapshot(runtime.model).selected_agent).toBe("alpha");
    await submitInput(runtime, ui, "second message");

    expect(readFileSync(messages, "utf8")).toBe(
      "beta\tfirst message\nalpha\tsecond message\n",
    );
  } finally {
    if (priorNorthBin === undefined) delete process.env.NORTH_BIN;
    else process.env.NORTH_BIN = priorNorthBin;
    rmSync(root, { recursive: true, force: true });
  }
});

test("agent rows remove terminal controls and clamp by terminal cells", async () => {
  const hostile = agent(
    "hostile",
    "Al\u001b]0;owned\u0007ice\nRoot\u0007\u009f",
    "work\ting\u001b[2J",
    "界界界界界界界界",
  );
  const row = agentRowText(hostile, true, 24);

  expect(row).not.toMatch(/[\u0000-\u001f\u007f-\u009f]/u);
  expect(row).not.toContain("owned");
  expect(Bun.stringWidth(row)).toBeLessThanOrEqual(24);
  expect(agentCellText("AB界界CD", 7)).toBe("AB界界…");

  const snapshot = await snapshotOf(row, 24, 2);
  expect(snapshot).toContain("› Alice Root");
  expect(snapshot).not.toContain("owned");
});

test("agent detail displays the route metadata already present in the roster", async () => {
  const rows = normalizeAgents({
    agents: [{
      control_id: "route-agent",
      display_handle: "Route agent",
      state_label: "working",
      task: "Inspect the route",
      provider: "openai",
      provider_target: "codex-personal",
      provider_label: "openai:\u001b[2Jcodex\npersonal",
      model: "gpt-5.6-sol",
      model_display: "sol",
      effort: "xhigh",
      orchestration_provenance: "orchestration:designer",
      goal: "Preserve\texact route",
      state: "working",
    }],
  }) as unknown[];
  const runtime = {
    detailIndex: 0,
    detailSegment: "all",
    detailView: "agents",
    model: replaceProjection(makeModel("list"), rows, [], []),
  };

  const route = agentRouteText(rows[0], 172);
  expect(route).not.toMatch(/[\u0000-\u001f\u007f-\u009f]/u);
  const snapshot = await snapshotOf(renderDetailPanel(runtime), 180, 7);
  expect(snapshot).toContain("Route agent (working) — Inspect the route");
  expect(snapshot).toContain("provider openai:codex personal");
  expect(snapshot).toContain("model sol");
  expect(snapshot).toContain("effort xhigh");
  expect(snapshot).toContain("orchestration:designer");
  expect(snapshot).toContain("state working");
  expect(snapshot).toContain("goal Preserve exact route");
});
