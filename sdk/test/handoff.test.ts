import { describe, expect, test } from "bun:test";
import {
  checkHandoff,
  composeHandoffSpawn,
  fireHandoff,
  observeHandoffUsageSample,
  parseAvailabilityRows,
  recoveryPinEvidence,
  thresholdCrossings,
  type AvailabilityRow,
} from "../src/handoff";
import { runHandoffCli } from "../src/handoff-cli";

const reset = "2026-07-29T00:00:00.000Z";
const observedAt = "2026-07-28T05:00:00.000Z";

function row(
  account: string,
  provider: "anthropic" | "openai",
  values: {
    week?: number;
    window?: number;
    models?: Record<string, number>;
    stale?: boolean;
    usableModels?: string[];
    verdict?: string;
  } = {},
): AvailabilityRow {
  const defaultModel = provider === "anthropic" ? "claude-opus-5" : "gpt-5.6-sol";
  return {
    account,
    provider,
    observedAt,
    stale: values.stale ?? false,
    rungs: {
      window: { name: "five-hour", pct: values.window ?? 20, resetsAt: reset },
      week: { pct: values.week ?? 20, resetsAt: reset },
      models: Object.fromEntries(
        Object.entries(values.models ?? { [defaultModel]: 20 })
          .map(([model, pct]) => [model, { pct, resetsAt: reset }]),
      ),
    },
    verdict: values.verdict ?? "available",
    usableModels: values.usableModels ?? [defaultModel],
  };
}

const openaiHeir = row("codex-heir", "openai", {
  usableModels: ["gpt-5.6-sol"],
});

describe("active route classification and same-strength heir selection", () => {
  test("week threshold kills the account", () => {
    const check = checkHandoff([
      row("claude-active", "anthropic", { week: 80, verdict: "cooked-week" }),
      openaiHeir,
    ], {
      provider: "anthropic",
      account: "claude-active",
      model: "claude-opus-5",
      tier: "senior",
    }, 80);
    expect(check.classification).toBe("account-dead");
    expect(check.trigger?.rung).toBe("week");
    expect(check.heir).toEqual({
      provider: "openai",
      account: "codex-heir",
      model: "gpt-5.6-sol",
      tier: "senior",
      observedAt,
    });
  });

  test("window threshold kills the route until reset", () => {
    const check = checkHandoff([
      row("claude-active", "anthropic", { window: 91, verdict: "cooked-window" }),
      openaiHeir,
    ], {
      provider: "anthropic", account: "claude-active",
      model: "claude-opus-5", tier: "senior",
    }, 80);
    expect(check.classification).toBe("window-dead");
    expect(check.trigger).toMatchObject({ rung: "window", resetsAt: reset });
  });

  test("model threshold keeps the account alive and kills only the active model", () => {
    const check = checkHandoff([
      row("claude-active", "anthropic", {
        models: { "claude-opus-5": 95, "claude-sonnet-5": 10 },
        usableModels: ["claude-sonnet-5"],
        verdict: "model-cooked[claude-opus-5]",
      }),
      openaiHeir,
    ], {
      provider: "anthropic", account: "claude-active",
      model: "claude-opus-5", tier: "senior",
    }, 80);
    expect(check.classification).toBe("model-dead");
    expect(check.trigger).toMatchObject({ rung: "model", model: "claude-opus-5" });
    expect(check.receipts.active.rungs.week.pct).toBe(20);
  });

  test("below-threshold route stays active and does not name an heir", () => {
    const check = checkHandoff([
      row("claude-active", "anthropic"),
      openaiHeir,
    ], {
      provider: "anthropic", account: "claude-active",
      model: "claude-opus-5", tier: "senior",
    }, 80);
    expect(check.classification).toBe("available");
    expect(check.heir).toBeUndefined();
  });

  test("stale and model-cooked candidates do not count as capacity", () => {
    const check = checkHandoff([
      row("claude-active", "anthropic", { week: 100, verdict: "cooked-week" }),
      row("codex-stale", "openai", { stale: true }),
      row("codex-cooked", "openai", {
        models: { "gpt-5.6-sol": 90 },
        usableModels: ["gpt-5.6-terra"],
        verdict: "model-cooked[gpt-5.6-sol]",
      }),
    ], {
      provider: "anthropic", account: "claude-active",
      model: "claude-opus-5", tier: "senior",
    }, 80);
    expect(check.heir).toBeUndefined();
  });

  test("stale active evidence cannot authorize recovery", () => {
    expect(() => checkHandoff([
      row("claude-active", "anthropic", {
        week: 100,
        stale: true,
        verdict: "cooked-week",
      }),
      openaiHeir,
    ], {
      provider: "anthropic",
      account: "claude-active",
      model: "claude-opus-5",
      tier: "senior",
    }, 80)).toThrow("active availability evidence");
  });
});

test("pinned availability parser rejects shape drift", () => {
  const valid = row("codex-heir", "openai");
  expect(parseAvailabilityRows([valid])).toEqual([valid]);
  expect(() => parseAvailabilityRows([{ ...valid, surprise: true }]))
    .toThrow("fields mismatch");
});

test("warning detection reports every crossed rung", () => {
  const crossings = thresholdCrossings(row("claude-active", "anthropic", {
    week: 82,
    window: 81,
    models: { "claude-opus-5": 95, "claude-sonnet-5": 20 },
  }), 80);
  expect(crossings.map(({ rung, name }) => `${rung}:${name}`)).toEqual([
    "week:week",
    "window:five-hour",
    "model:claude-opus-5",
  ]);
});

test("provider-recovery evidence pins the complete heir route and embeds receipts", () => {
  const check = checkHandoff([
    row("claude-active", "anthropic", { week: 100, verdict: "cooked-week" }),
    openaiHeir,
  ], {
    provider: "anthropic", account: "claude-active",
    model: "claude-opus-5", tier: "senior",
  }, 80);
  const evidence = recoveryPinEvidence(check, new Date("2026-07-28T05:30:00.000Z"));
  expect(evidence.reasonCode).toBe("provider-recovery");
  expect(evidence.pins).toEqual([
    { kind: "provider", value: "openai" },
    { kind: "account", value: "codex-heir" },
    { kind: "model", value: "gpt-5.6-sol" },
  ]);
  expect(evidence.detail).toContain('"activeReceipt"');
  expect(evidence.detail).toContain('"heirReceipt"');
});

test("dry-run composition is complete and execution remains injectable", () => {
  const check = checkHandoff([
    row("claude-active", "anthropic", { window: 100, verdict: "cooked-window" }),
    openaiHeir,
  ], {
    provider: "anthropic", account: "claude-active",
    model: "claude-opus-5", tier: "senior",
  }, 80);
  const facts = new Map([
    ["root", [{ predicate: "title", value: "root program" }]],
    ["child", [{ predicate: "title", value: "child lane" }, { predicate: "part_of", value: "@root" }]],
  ]);
  const spawn = composeHandoffSpawn(check, "root", "/fixture/succession.md", "human", {
    now: new Date("2026-07-28T05:30:00.000Z"),
    northBin: "/fixture/north",
    peerBb: "/fixture/bb",
    msgCli: "/fixture/msg-cli.clj",
    env: { AGENT_ID: "coordinator", NORTH_PORT: "9000" },
    readBrief: () => "succession context",
    getChildren: () => ["child"],
    getFacts: (id) => facts.get(id) ?? [],
  });
  expect(spawn.command.args.slice(0, 2)).toEqual(["spawn", "team-lead"]);
  expect(spawn.command.args).toContain("--thread");
  expect(spawn.command.args).toContain("--provider");
  expect(spawn.command.args).toContain("--target");
  expect(spawn.command.args).toContain("--model");
  expect(spawn.command.args).toContain("--pin-evidence");
  expect(spawn.command.args).toContain("--notify");
  expect(spawn.context.brief.content).toBe("succession context");
  expect(spawn.context.threadMap.map(({ id }) => id)).toEqual(["root", "child"]);
  expect(spawn.prompt).toContain("THREAD MAP");
  expect(spawn.notification.args).toEqual([
    "/fixture/msg-cli.clj", "9000", "send", "coordinator", "human",
    "PROVIDER HANDOFF FIRED",
    "@root -> openai/codex-heir/gpt-5.6-sol (senior); reason=provider-recovery",
  ]);

  const commands: Array<[string, string[]]> = [];
  fireHandoff(spawn, {
    run: (executable, args) => {
      commands.push([executable, args]);
      return { status: 0 };
    },
  });
  expect(commands).toEqual([
    [spawn.command.executable, spawn.command.args],
    [spawn.notification.executable, spawn.notification.args],
  ]);
});

test("available routes and missing heirs refuse fire composition", () => {
  const active = {
    provider: "anthropic" as const,
    account: "claude-active",
    model: "claude-opus-5",
    tier: "senior" as const,
  };
  const runtime = {
    readBrief: () => "brief",
    getChildren: () => [],
    getFacts: () => [],
  };
  expect(() => composeHandoffSpawn(
    checkHandoff([row("claude-active", "anthropic")], active, 80),
    "root", "/brief", "human", runtime,
  )).toThrow("has not crossed");
  expect(() => composeHandoffSpawn(
    checkHandoff([
      row("claude-active", "anthropic", { week: 100, verdict: "cooked-week" }),
    ], active, 80),
    "root", "/brief", "human", runtime,
  )).toThrow("no same-tier");
});

describe("handoff CLI", () => {
  const rows = [
    row("claude-active", "anthropic", { week: 100, verdict: "cooked-week" }),
    openaiHeir,
  ];
  const env = {
    AGENT_PROVIDER: "anthropic",
    AGENT_TARGET: "claude-active",
    AGENT_MODEL: "claude-opus-5",
    AGENT_TIER: "senior",
    AGENT_ID: "coordinator",
    AGENT_COORDINATOR: "human",
    NORTH_PORT: "9000",
  };

  test("check renders the active trigger and heir", () => {
    const output: string[] = [];
    expect(runHandoffCli(["check", "--threshold", "80"], {
      env,
      loadRows: () => rows,
      stdout: (line) => output.push(line),
    })).toBe(0);
    expect(output.join("\n")).toContain("classification account-dead");
    expect(output.join("\n")).toContain("heir openai/codex-heir/gpt-5.6-sol");
  });

  test("fire dry-run prints the complete spawn and executes nothing", () => {
    const output: string[] = [];
    let executions = 0;
    expect(runHandoffCli([
      "fire", "--thread", "root", "--brief", "/fixture/succession.md", "--dry-run",
    ], {
      env,
      loadRows: () => rows,
      stdout: (line) => output.push(line),
      readBrief: () => "succession",
      getChildren: () => [],
      getFacts: () => [{ predicate: "title", value: "root" }],
      northBin: "/fixture/north",
      peerBb: "/fixture/bb",
      msgCli: "/fixture/msg-cli.clj",
      run: () => {
        executions++;
        return { status: 0 };
      },
    })).toBe(0);
    const document = JSON.parse(output[0]);
    expect(document.command.args.slice(0, 2)).toEqual(["spawn", "team-lead"]);
    expect(document.pinEvidence.reasonCode).toBe("provider-recovery");
    expect(document.context.brief.content).toBe("succession");
    expect(document.notification.target).toBe("human");
    expect(executions).toBe(0);
  });

  test("fire executes spawn then notification through injected commands", () => {
    const commands: string[] = [];
    expect(runHandoffCli([
      "fire", "--thread", "root", "--brief", "/fixture/succession.md",
    ], {
      env,
      loadRows: () => rows,
      stdout: () => {},
      readBrief: () => "succession",
      getChildren: () => [],
      getFacts: () => [],
      northBin: "/fixture/north",
      peerBb: "/fixture/bb",
      msgCli: "/fixture/msg-cli.clj",
      run: (executable, args) => {
        commands.push(`${executable} ${args.slice(0, 2).join(" ")}`);
        return { status: 0 };
      },
    })).toBe(0);
    expect(commands).toEqual([
      "/fixture/north spawn team-lead",
      "/fixture/bb /fixture/msg-cli.clj 9000",
    ]);
  });
});

describe("warn-first usage detection", () => {
  const rows = [
    row("claude-active", "anthropic", {
      week: 85,
      window: 82,
      models: { "claude-opus-5": 95 },
      verdict: "cooked-week",
    }),
    openaiHeir,
  ];
  const baseEnv = {
    AGENT_PROVIDER: "anthropic",
    AGENT_TARGET: "claude-active",
    AGENT_MODEL: "claude-opus-5",
    AGENT_TIER: "senior",
    AGENT_THREAD: "root",
    AGENT_ID: "coordinator",
    AGENT_COORDINATOR: "human",
    NORTH_PORT: "9000",
    NORTH_HANDOFF_WARN_THRESHOLD: "80",
  };

  test("default-off emits one fact and mail per crossed rung with no fire", () => {
    const commands: Array<{ executable: string; args: string[] }> = [];
    const warnings = observeHandoffUsageSample({
      env: baseEnv,
      loadRows: () => rows,
      northBin: "/fixture/north",
      peerBb: "/fixture/bb",
      msgCli: "/fixture/msg-cli.clj",
      run: (executable, args) => {
        commands.push({ executable, args });
        return { status: 0 };
      },
    });
    expect(warnings.map(({ crossing }) => crossing.rung)).toEqual([
      "week", "window", "model",
    ]);
    expect(warnings.every(({ automaticFire }) => automaticFire === false)).toBe(true);
    expect(commands).toHaveLength(6);
    expect(commands.filter(({ args }) => args[0] === "tell")).toHaveLength(3);
    expect(commands.filter(({ args }) => args.includes("PROVIDER CAPACITY WARNING"))).toHaveLength(3);
    expect(commands.some(({ args }) => args[0] === "handoff")).toBe(false);
  });

  test("enabled automatic fire remains after every warning command", () => {
    const commands: string[] = [];
    const warnings = observeHandoffUsageSample({
      env: {
        ...baseEnv,
        NORTH_HANDOFF_AUTO_FIRE: "true",
        NORTH_HANDOFF_ROOT_THREAD: "program-root",
        NORTH_HANDOFF_BRIEF: "/fixture/succession.md",
      },
      loadRows: () => rows,
      northBin: "/fixture/north",
      peerBb: "/fixture/bb",
      msgCli: "/fixture/msg-cli.clj",
      run: (executable, args) => {
        commands.push(`${executable} ${args.join(" ")}`);
        return { status: 0 };
      },
    });
    expect(warnings.every(({ automaticFire }) => automaticFire === true)).toBe(true);
    expect(commands).toHaveLength(7);
    expect(commands.at(-1)).toBe(
      "/fixture/north handoff fire --thread program-root --brief /fixture/succession.md",
    );
  });
});
