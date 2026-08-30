import { expect, test } from "bun:test";
import { chmodSync, mkdtempSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import {
  "delegation-argv!" as delegationArgv,
  "refresh!" as refresh,
  "semantic-view-text!" as semanticViewText,
} from "../src/bridge/generated/north/bridge/app.js";
import {
  "parse-bridge-view-id!" as parseBridgeViewId,
} from "../src/bridge/generated/north/bridge/cli.js";
import {
  "bridgesnapshot-all" as allThings,
  "bridgesnapshot-goals" as goals,
  "bridgesnapshot-semantic-agents" as semanticAgents,
  "bridgesnapshot-store-space" as storeSpace,
  "bridgesnapshot-store-version" as storeVersion,
  "make-model" as makeModel,
  "replace-catalog" as replaceCatalog,
  "set-filter" as setFilter,
  "snapshot" as snapshot,
  "trackedthing-assignee" as assignee,
  "trackedthing-assignee-title" as assigneeTitle,
  "trackedthing-desired-outcome" as desiredOutcome,
  "trackedthing-id" as trackedThingId,
  "trackedthing-project" as project,
  "trackedthing-task" as task,
  "trackedthing-title" as trackedThingTitle,
} from "../src/bridge/generated/north/bridge/model.js";
import {
  "referent-action-argv!" as actionArgv,
  "referent-action-request!" as actionRequest,
  "run-referent-action!" as runAction,
  "semantic-action-result-text!" as actionResultText,
  "validate-committed-readback!" as validateReadback,
  "validate-semantic-catalog!" as validateCatalog,
} from "../src/bridge/generated/north/bridge/referent-actions.js";

const NORTH = "/checkout/bin/north";

const CATALOG = {
  protocol: "north.semantic-catalog",
  version: 1,
  storeSpace: "north-coordination",
  storeVersion: 42,
  trackedThings: [
    {
      id: "@tracked:01-tracker", title: "Tracker", desiredOutcome: null,
      agent: true, plan: false, project: false, task: false,
      assignee: null, assigneeTitle: null, status: "ready",
    },
    {
      id: "@tracked:02-worker", title: "Worker", desiredOutcome: null,
      agent: true, plan: false, project: false, task: false,
      assignee: null, assigneeTitle: null, status: "ready",
    },
    {
      id: "@tracked:03-ship", title: "Ship bridge",
      desiredOutcome: "The bridge candidate is accepted",
      agent: false, plan: true, project: false, task: true,
      assignee: "@tracked:02-worker", assigneeTitle: "Worker", status: null,
    },
    {
      id: "@tracked:04-release", title: "Release succeeds",
      desiredOutcome: "The release is available to its intended users",
      agent: false, plan: false, project: false, task: false,
      assignee: null, assigneeTitle: null, status: null,
    },
    {
      id: "@tracked:05-project", title: "Release path", desiredOutcome: null,
      agent: false, plan: true, project: true, task: true,
      assignee: "@tracked:02-worker", assigneeTitle: "Worker", status: null,
    },
    {
      id: "@tracked:06-note", title: "Plain tracked note", desiredOutcome: null,
      agent: false, plan: false, project: false, task: false,
      assignee: null, assigneeTitle: null, status: null,
    },
  ],
};

function cloneCatalog() {
  return JSON.parse(JSON.stringify(CATALOG)) as typeof CATALOG;
}

test("catalog ingress is one exact command bound to one exact Store snapshot", async () => {
  const request = actionRequest("catalog", []);
  expect(actionArgv(request, NORTH)).toEqual([
    NORTH, "work", "catalog", "--json",
  ]);

  const calls: string[][] = [];
  const catalog = await runAction(request, {
    northExecutable: NORTH,
    runCommand: async (argv: string[]) => {
      calls.push(argv);
      return JSON.stringify(CATALOG);
    },
  });
  expect(calls).toEqual([[NORTH, "work", "catalog", "--json"]]);
  expect(catalog.storeSpace).toBe("north-coordination");
  expect(catalog.storeVersion).toBe(42);

  const model = replaceCatalog(
    makeModel("all"), catalog.trackedThings, catalog.storeSpace, catalog.storeVersion,
  );
  const state = snapshot(model);
  expect(storeSpace(state)).toBe("north-coordination");
  expect(storeVersion(state)).toBe(42);
  expect(allThings(state).map(trackedThingId)).toEqual(
    CATALOG.trackedThings.map((row) => row.id),
  );
});

test("Bridge refresh consumes only the public catalog command", async () => {
  const root = mkdtempSync(join(tmpdir(), "north-bridge-semantic-catalog-"));
  const north = join(root, "north");
  const catalogPath = join(root, "catalog.json");
  const argvPath = join(root, "argv.txt");
  const priorNorth = process.env.NORTH_BIN;
  try {
    await Bun.write(catalogPath, JSON.stringify(CATALOG));
    await Bun.write(
      north,
      `#!/bin/sh\nprintf '%s\\n' "$@" > ${JSON.stringify(argvPath)}\nexec cat ${JSON.stringify(catalogPath)}\n`,
    );
    chmodSync(north, 0o755);
    process.env.NORTH_BIN = north;

    const runtime = {
      model: makeModel("all"), view: "all", workIndex: 9, renders: 0,
      render() { this.renders += 1; },
    };
    await refresh(runtime);

    expect(readFileSync(argvPath, "utf8")).toBe("work\ncatalog\n--json\n");
    expect(allThings(snapshot(runtime.model)).map(trackedThingId))
      .toEqual(CATALOG.trackedThings.map((row) => row.id));
    expect(storeSpace(snapshot(runtime.model))).toBe("north-coordination");
    expect(storeVersion(snapshot(runtime.model))).toBe(42);
    expect(runtime.renders).toBe(1);
  } finally {
    if (priorNorth === undefined) delete process.env.NORTH_BIN;
    else process.env.NORTH_BIN = priorNorth;
    rmSync(root, { recursive: true, force: true });
  }
});

test("catalog validation requires the exact null-safe identity-ordered envelope", () => {
  const catalog = validateCatalog(cloneCatalog());
  const plain = catalog.trackedThings.at(-1)!;
  expect(desiredOutcome(plain)).toBeNull();
  expect(assignee(plain)).toBeNull();
  expect(assigneeTitle(plain)).toBeNull();

  const extraEnvelope = { ...cloneCatalog(), extra: true };
  expect(() => validateCatalog(extraEnvelope)).toThrow("invalid snapshot envelope");

  const omittedNull = cloneCatalog();
  delete (omittedNull.trackedThings[0] as Partial<typeof omittedNull.trackedThings[0]>).status;
  expect(() => validateCatalog(omittedNull)).toThrow("invalid tracked thing row");

  const emptyOptional = cloneCatalog();
  emptyOptional.trackedThings[0]!.status = "";
  expect(() => validateCatalog(emptyOptional)).toThrow("requires exact status");

  const duplicate = cloneCatalog();
  duplicate.trackedThings[1]!.id = duplicate.trackedThings[0]!.id;
  expect(() => validateCatalog(duplicate)).toThrow("uniquely identity-ordered");

  const unordered = cloneCatalog();
  [unordered.trackedThings[0], unordered.trackedThings[1]] =
    [unordered.trackedThings[1]!, unordered.trackedThings[0]!];
  expect(() => validateCatalog(unordered)).toThrow("uniquely identity-ordered");

  const incompleteTask = cloneCatalog();
  incompleteTask.trackedThings[2]!.assignee = null;
  incompleteTask.trackedThings[2]!.assigneeTitle = null;
  expect(() => validateCatalog(incompleteTask)).toThrow("complete Assignment");

  const splitAssignee = cloneCatalog();
  splitAssignee.trackedThings[2]!.assigneeTitle = null;
  expect(() => validateCatalog(splitAssignee)).toThrow("null together");
});

test("Agents, Goals, and All are identity-preserving catalog derivations", () => {
  const catalog = validateCatalog(cloneCatalog());
  const model = replaceCatalog(
    makeModel("all"), catalog.trackedThings, catalog.storeSpace, catalog.storeVersion,
  );
  const state = snapshot(model);

  expect(semanticAgents(state).map(trackedThingTitle)).toEqual(["Tracker", "Worker"]);
  expect(goals(state).map(trackedThingTitle)).toEqual(["Ship bridge", "Release succeeds"]);
  expect(allThings(state)).toHaveLength(CATALOG.trackedThings.length);
  expect(new Set(allThings(state).map(trackedThingId)).size).toBe(allThings(state).length);
  expect(allThings(state).map(trackedThingTitle)).toContain("Release path");
  expect(allThings(state).map(trackedThingTitle)).toContain("Plain tracked note");

  const releasePath = allThings(state).find(
    (item: unknown) => trackedThingTitle(item) === "Release path",
  )!;
  expect(project(releasePath)).toBe(true);
  expect(task(releasePath)).toBe(true);
  expect(semanticViewText(
    snapshot(replaceCatalog(makeModel("all"), [releasePath], "north-coordination", 42)),
    "all", 0, 120,
  )).toContain("[Plan · Project · Task] Release path · assigned to Worker");
});

test("All search joins an Agent and assigned Goal through the worker name", () => {
  const catalog = validateCatalog(cloneCatalog());
  const filtered = snapshot(setFilter(
    replaceCatalog(
      makeModel("all"), catalog.trackedThings, catalog.storeSpace, catalog.storeVersion,
    ),
    "worker",
  ));
  expect(allThings(filtered).map(trackedThingTitle)).toEqual([
    "Worker", "Ship bridge", "Release path",
  ]);
  expect(goals(filtered).map(trackedThingTitle)).toEqual(["Ship bridge"]);
  expect(semanticAgents(filtered).map(trackedThingTitle)).toEqual(["Worker"]);
});

test("work-start, ownership, Request, and delegation argv preserve explicit authority", () => {
  const start = actionRequest("start", [
    "@tracked:ship", "@revision:exact", "@actor:owner", "signed:exact",
  ]);
  expect(actionArgv(start, NORTH)).toEqual([
    NORTH, "work", "start", "@tracked:ship",
    "--revision", "@revision:exact",
    "--authorized-by", "@actor:owner",
    "--signature", "signed:exact",
    "--json",
  ]);

  const transition = JSON.stringify({ protocol: "work-ownership-v1", event: "accept" });
  expect(actionArgv(actionRequest("ownership", [transition]), NORTH)).toEqual([
    NORTH, "work", "ownership", "--transition", transition, "--json",
  ]);

  expect(actionArgv(actionRequest("request", ["@sender", "@recipient", "hello"]), NORTH))
    .toEqual([
      NORTH, "work", "request", "--from", "@sender", "--to", "@recipient",
      "--body", "hello", "--json",
    ]);
  expect(actionArgv(actionRequest(
    "request", ["@tracked:ship", "@sender", "@recipient", "hello"],
  ), NORTH)).toEqual([
    NORTH, "work", "request", "@tracked:ship", "--from", "@sender",
    "--to", "@recipient", "--body", "hello", "--json",
  ]);

  const delegated = delegationArgv(
    NORTH, "ship bridge|--role|implementer|--reasoning|high",
  );
  expect(delegated).toEqual([
    NORTH, "delegate", "ship bridge", "--role", "implementer", "--reasoning", "high",
  ]);
  expect(delegated).not.toContain("--thread");
  expect(delegated).not.toContain("@tracked:ship");
});

test("committed receipts are validated internally and summarized without raw JSON", () => {
  const request = actionRequest("track", ["Ship bridge", "@actor:tracker"]);
  const receipt = validateReadback(request, JSON.stringify({
    protocol: "north.semantic-receipt",
    version: 1,
    action: "track",
    storeVersion: 43,
    referent: "@tracked:ship",
  }));
  const summary = actionResultText(request, receipt);
  expect(summary).toBe("tracked thing committed: @tracked:ship");
  expect(summary).not.toContain("{");
  expect(summary).not.toContain("\"referent\"");
  expect(actionResultText(actionRequest("show", ["@tracked:ship"]), {}))
    .toBe("tracked thing view loaded");
});

test("Bridge CLI admits only the three public view ids", () => {
  expect(parseBridgeViewId(null)).toBeNull();
  expect(["agents", "goals", "all"].map(parseBridgeViewId))
    .toEqual(["agents", "goals", "all"]);
  for (const value of ["threads", "tasks", "list", "Agents", " goals "]) {
    expect(() => parseBridgeViewId(value)).toThrow(
      "requires exactly agents, goals, or all",
    );
  }
});
