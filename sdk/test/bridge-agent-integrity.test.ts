import { expect, test } from "bun:test";
import { BoxRenderable, TextRenderable } from "@opentui/core";
import { createTestRenderer } from "@opentui/core/testing";
import {
  agent_cell_text as agentCellText,
  agent_route_text as agentRouteText,
  agent_row_text as agentRowText,
  normalize_agents as normalizeAgents,
  reconcile_agent_selection_bang as reconcileAgentSelection,
  render_detail_panel_bang as renderDetailPanel,
  roster_text as rosterText,
  selected_agent_id as selectedAgentId,
} from "../src/bridge/generated/north/bridge/app.js";
import {
  Agent,
  make_model as makeModel,
  replace_projection as replaceProjection,
  select_agent as selectAgent,
  snapshot,
  upsert_agent as upsertAgent,
} from "../src/bridge/generated/north/bridge/model.js";

Object.defineProperty(process.stdout, "columns", { value: 180, configurable: true });
Object.defineProperty(process.stdout, "rows", { value: 40, configurable: true });

function agent(id: string, name: string, status = "working", task = "") {
  return Agent(id, name, status, task, "", "", "", "", "", "", "", "", "");
}

async function frameOf(content: unknown, width: number, height: number) {
  const { renderer, renderOnce, captureCharFrame } = await createTestRenderer({ width, height });
  const root = new BoxRenderable(renderer, { width: "100%", height: "100%" });
  const text = new TextRenderable(renderer, {
    width: "100%", height: "100%", wrapMode: "none", truncate: true,
  });
  root.add(text);
  renderer.root.add(root);
  text.content = content;
  await renderOnce();
  const frame = captureCharFrame();
  renderer.destroy();
  return frame;
}

test("agent reorder preserves the highlighted identity and composer target", async () => {
  const alpha = agent("alpha", "Alpha");
  const beta = agent("beta", "Beta");
  const gamma = agent("gamma", "Gamma");
  let model = makeModel("list");
  for (const row of [alpha, beta, gamma]) model = upsertAgent(model, row);

  const priorId = selectedAgentId(snapshot(model), 1);
  const runtime = {
    agentIndex: 1,
    model: selectAgent(replaceProjection(model, [gamma, alpha, beta], [], []), "alpha"),
  };
  expect(reconcileAgentSelection(runtime, priorId)).toBe("beta");

  const state = snapshot(runtime.model);
  const target = selectedAgentId(state, runtime.agentIndex);
  const frame = await frameOf(
    `${rosterText(state, runtime.agentIndex, "", false)}\ncomposer target: ${target}`,
    72,
    6,
  );
  expect(runtime.agentIndex).toBe(2);
  expect(state.selected_agent).toBe("beta");
  expect(target).toBe("beta");
  expect(frame).toContain("› Beta");
  expect(frame).toContain("composer target: beta");
  expect(frame).not.toContain("› Alpha");
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

  const frame = await frameOf(row, 24, 2);
  expect(frame).toContain("› Alice Root");
  expect(frame).not.toContain("owned");
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
  const frame = await frameOf(renderDetailPanel(runtime), 180, 7);
  expect(frame).toContain("Route agent (working) — Inspect the route");
  expect(frame).toContain("provider openai:codex personal");
  expect(frame).toContain("model sol");
  expect(frame).toContain("effort xhigh");
  expect(frame).toContain("orchestration:designer");
  expect(frame).toContain("state working");
  expect(frame).toContain("goal Preserve exact route");
});
