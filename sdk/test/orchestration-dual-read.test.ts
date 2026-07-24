import { afterEach, expect, spyOn, test } from "bun:test";
import { resetCatalogBundleCache, staffingSource } from "../src/orchestration-graph-source";
import { loadGafferStaffing } from "../src/gaffer-staffing";

// Dual-read seam (thread 019f8f5c). These assertions are hermetic. Phase 2
// flipped the default source to GRAPH: unset resolves graph, and only an
// explicit `file` reads the packaged Gaffer contract. The graph path is
// exercised against an unreachable coordinator so its wiring is proven without
// depending on a live import. Byte-parity of the two live sources is proven
// separately by cli/tests/orchestration-parity-test.clj and
// src/orchestration-dual-read-probe.ts against a coordinator.

const priorSource = process.env.NORTH_STAFFING_SOURCE;
const priorPort = process.env.NORTH_PORT;

afterEach(() => {
  if (priorSource === undefined) delete process.env.NORTH_STAFFING_SOURCE;
  else process.env.NORTH_STAFFING_SOURCE = priorSource;
  if (priorPort === undefined) delete process.env.NORTH_PORT;
  else process.env.NORTH_PORT = priorPort;
  resetCatalogBundleCache();
});

test("staffing source defaults to graph; only explicit file falls back", () => {
  delete process.env.NORTH_STAFFING_SOURCE;
  expect(staffingSource()).toBe("graph");
  process.env.NORTH_STAFFING_SOURCE = "";
  expect(staffingSource()).toBe("graph");
  process.env.NORTH_STAFFING_SOURCE = "anything-else";
  expect(staffingSource()).toBe("graph");
  process.env.NORTH_STAFFING_SOURCE = "file";
  expect(staffingSource()).toBe("file");
});

test("explicit file source loads the packaged stock catalog", () => {
  process.env.NORTH_STAFFING_SOURCE = "file";
  const catalog = loadGafferStaffing();
  expect(catalog.sourceVersion).toBe(2);
  expect(catalog.presets.map((p) => p.name).sort()).toEqual([
    "analyst", "designer", "director", "executor", "implementer", "integrator",
    "judge", "research-scientist", "reviewer", "scout", "verifier",
  ]);
});

test("graph source falls back to the packaged catalog when the projector is unreachable", () => {
  // Spawn admission must never block on the graph: a failed projection (here an
  // unreachable coordinator) logs the named failure and falls back to the
  // packaged Gaffer JSON rather than throwing.
  process.env.NORTH_STAFFING_SOURCE = "graph";
  process.env.NORTH_PORT = "1"; // unreachable coordinator
  resetCatalogBundleCache();
  const warn = spyOn(console, "warn").mockImplementation(() => {});
  try {
    const catalog = loadGafferStaffing();
    expect(catalog.sourceVersion).toBe(2);
    expect(warn).toHaveBeenCalledTimes(1);
    expect(warn.mock.calls[0]?.[0]).toMatch(/graph catalog projection failed for staffing catalog/);
  } finally {
    warn.mockRestore();
  }
});
