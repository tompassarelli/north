import { afterEach, expect, test } from "bun:test";
import { chmodSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { spawnSync } from "node:child_process";

const temporary: string[] = [];
afterEach(() => {
  for (const path of temporary.splice(0)) rmSync(path, { recursive: true, force: true });
});

test("MCP dispatch advertises and forwards an exact account target to the SDK process", () => {
  const directory = mkdtempSync(join(tmpdir(), "north-mcp-target-"));
  temporary.push(directory);
  const capture = join(directory, "sdk-env");
  const events = join(directory, "events");
  const fakeBun = join(directory, "bun");
  const fakeBb = join(directory, "bb");
  const fakeNorth = join(directory, "north");
  writeFileSync(fakeBun, `#!/usr/bin/env bash
case "$*" in
  *mcp-route-preflight.ts*) exit 0 ;;
esac
printf 'spawn:%s\n' "$AGENT_ID" >> "$NORTH_MCP_EVENTS"
printf '%s|%s|%s|%s|%s\n' "$AGENT_TARGET" "$AGENT_PROVIDER" "$AGENT_ID" "$NORTH_DISPATCH_DRIVER_PRECLAIMED" "$*" > "$NORTH_MCP_CAPTURE"
"$NORTH_MCP_BB" ignored 7977 release "@\${@: -1}" "$AGENT_ID"
`);
  chmodSync(fakeBun, 0o755);
  writeFileSync(fakeBb, `#!/usr/bin/env bash
printf 'driver:%s:%s:%s\n' "$3" "$4" "$5" >> "$NORTH_MCP_EVENTS"
exit 0
`);
  chmodSync(fakeBb, 0o755);
  writeFileSync(fakeNorth, `#!/usr/bin/env bash
for _ in $(seq 1 200); do
  grep -q '^driver:release:' "$NORTH_MCP_EVENTS" 2>/dev/null && break
  sleep 0.01
done
printf '%s\n' '[{"predicate":"kind","value":"lane"},{"predicate":"role","value":"integrator"},{"predicate":"goal","value":"contract probe"},{"predicate":"provider","value":"anthropic"},{"predicate":"provider_target","value":"claude-personal-tompas0x-gmail"},{"predicate":"model","value":"claude-opus-4-8"},{"predicate":"effort","value":"xhigh"},{"predicate":"composition_kind","value":"preset"},{"predicate":"composition_id","value":"integrator"},{"predicate":"composition_overrides","value":"[]"},{"predicate":"repo","value":"north"},{"predicate":"spawned_at","value":"2026-07-17T00:00:00Z"},{"predicate":"display_handle","value":"anthropic-claude-gmail-opus-xhigh-integrator-probe"},{"predicate":"display_name","value":"anthropic:claude-personal-tompas0x-gmail · opus · xhigh · gaffer:integrator · contract probe"},{"predicate":"identity_manifest_sha256","value":"6accbefdabbad748a1bc37ba82db7171ed923930d498bcb8cfe47174f60cdb33"},{"predicate":"outcome","value":"ran"}]'
`);
  chmodSync(fakeNorth, 0o755);

  const north = resolve(import.meta.dir, "../..");
  const request = `${JSON.stringify({
    jsonrpc: "2.0", id: 1, method: "tools/call",
    params: { name: "dispatch", arguments: {
      id: "@019f6c5e-61d0-7880-98a0-f8999eac7b03",
      role: "integrator",
      composition: { kind: "preset", id: "integrator", overrides: [] },
      provider: "anthropic",
      target: "claude-personal-tompas0x-gmail",
    } },
  })}\n`;
  const result = spawnSync("bb", [resolve(north, "bin/north-mcp")], {
    input: request,
    encoding: "utf8",
    env: {
      ...process.env,
      NORTH_MCP_BUN: fakeBun,
      NORTH_MCP_BB: fakeBb,
      NORTH_MCP_CAPTURE: capture,
      NORTH_MCP_EVENTS: events,
      NORTH_BIN: fakeNorth,
    },
  });
  expect(result.status).toBe(0);
  const response = JSON.parse(result.stdout.trim());
  expect(response.result.isError).not.toBe(true);
  expect(response.result.content[0].text).toContain("completed anthropic-claude-gmail-opus-xhigh-integrator-probe");
  expect(response.result.content[0].text).not.toContain("Agent is running");
  expect(response.result.content[0].text).toContain("target=claude-personal-tompas0x-gmail");
  expect(response.result.content[0].text).toContain("thread @019f6c5e-61d0-7880-98a0-f8999eac7b03");
  expect(response.result.content[0].text).not.toContain("@@019f6c5e-61d0-7880-98a0-f8999eac7b03");
  const [target, provider, agentId, preclaimed, command] = readFileSync(capture, "utf8").trim().split("|");
  expect(target).toBe("claude-personal-tompas0x-gmail");
  expect(provider).toBe("anthropic");
  expect(preclaimed).toBe("1");
  expect(command).toContain("/dispatch.ts");
  expect(command).toContain("/dispatch.ts 019f6c5e-61d0-7880-98a0-f8999eac7b03");
  expect(command).not.toContain("@@019f6c5e-61d0-7880-98a0-f8999eac7b03");
  expect(agentId).toMatch(/^sdk-f8999eac7b03-[a-z0-9]+-[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/);

  const lifecycle = readFileSync(events, "utf8").trim().split("\n");
  const claim = lifecycle.findIndex((line) => line === `driver:claim:@019f6c5e-61d0-7880-98a0-f8999eac7b03:${agentId}`);
  const spawn = lifecycle.findIndex((line) => line === `spawn:${agentId}`);
  const release = lifecycle.findIndex((line) => line === `driver:release:@019f6c5e-61d0-7880-98a0-f8999eac7b03:${agentId}`);
  expect(claim).toBeGreaterThanOrEqual(0);
  expect(spawn).toBeGreaterThan(claim);
  expect(release).toBeGreaterThan(spawn);
});

test("MCP spawn reports pre-identity construction failure instead of fabricating a handle", () => {
  const directory = mkdtempSync(join(tmpdir(), "north-mcp-startup-failure-"));
  temporary.push(directory);
  const fakeNorth = join(directory, "north");
  writeFileSync(fakeNorth, "#!/usr/bin/env bash\nexit 1\n");
  chmodSync(fakeNorth, 0o755);

  const north = resolve(import.meta.dir, "../..");
  const request = `${JSON.stringify({
    jsonrpc: "2.0", id: 1, method: "tools/call",
    params: { name: "spawn", arguments: { prompt: "construction failure probe", role: "verifier" } },
  })}\n`;
  const result = spawnSync("bb", [resolve(north, "bin/north-mcp")], {
    input: request,
    encoding: "utf8",
    env: {
      ...process.env,
      HOME: directory,
      NORTH_BIN: fakeNorth,
      NORTH_MCP_BUN: "/bin/false",
      NORTH_SPAWN_STARTUP_TIMEOUT_MS: "500",
      NO_COLOR: "1",
    },
  });
  expect(result.status).toBe(0);
  const response = JSON.parse(result.stdout.trim());
  expect(response.result.isError).toBe(true);
  expect(response.result.content[0].text).toContain("child exited before startup acknowledgement");
  expect(response.result.content[0].text).toContain("durable log:");
  expect(response.result.content[0].text).not.toContain("spawned unknown");
  expect(response.result.content[0].text).not.toContain("Agent is running");
});

test("MCP dispatch rejects a contended thread before spawning and redacts coordinator output", () => {
  const directory = mkdtempSync(join(tmpdir(), "north-mcp-contended-"));
  temporary.push(directory);
  const marker = join(directory, "spawned");
  const fakeBun = join(directory, "bun");
  const fakeBb = join(directory, "bb");
  writeFileSync(fakeBun, `#!/usr/bin/env bash
touch "$NORTH_MCP_MARKER"
`);
  chmodSync(fakeBun, 0o755);
  writeFileSync(fakeBb, `#!/usr/bin/env bash
printf '%s\n' 'CANARY private coordinator diagnostic' >&2
exit 3
`);
  chmodSync(fakeBb, 0o755);

  const north = resolve(import.meta.dir, "../..");
  const thread = "019f6c5e-61d0-7880-98a0-f8999eac7b03";
  const result = spawnSync("bb", [resolve(north, "bin/north-mcp")], {
    input: `${JSON.stringify({ jsonrpc: "2.0", id: 1, method: "tools/call",
      params: { name: "dispatch", arguments: {
        id: thread,
        role: "verifier",
        composition: { kind: "preset", id: "verifier", overrides: [] },
      } } })}\n`,
    encoding: "utf8",
    env: { ...process.env, NORTH_MCP_BUN: fakeBun, NORTH_MCP_BB: fakeBb, NORTH_MCP_MARKER: marker },
  });
  expect(result.status).toBe(0);
  const response = JSON.parse(result.stdout.trim());
  expect(response.result.isError).toBe(true);
  expect(response.result.content[0].text).toBe(`thread @${thread} already has an active driver`);
  expect(response.result.content[0].text).not.toContain("CANARY");
  expect(() => readFileSync(marker)).toThrow();
});

test("raw MCP rejects non-contract Gaffer fields and verifier-as-topology before spawning", () => {
  const north = resolve(import.meta.dir, "../..");
  for (const [name, arguments_, expected] of [
    ["spawn", [], "arguments must be an object"],
    ["dispatch", {}, "dispatch id must be a non-empty string"],
    ["dispatch", { id: 42 }, "dispatch id must be a non-empty string"],
    ["dispatch", {
      id: "@@019f6c5e-61d0-7880-98a0-f8999eac7b03",
      role: "verifier",
      composition: { kind: "preset", id: "verifier", overrides: [] },
    }, "dispatch id must be a safe North thread id (bare or single @ prefix)"],
    ["dispatch", {
      id: "thread;touch-owned",
      role: "verifier",
      composition: { kind: "preset", id: "verifier", overrides: [] },
    }, "dispatch id must be a safe North thread id (bare or single @ prefix)"],
    ["spawn", {}, "spawn prompt must be a non-empty string"],
    ["spawn", { prompt: "" }, "spawn prompt must be a non-empty string"],
    ["spawn", { prompt: "probe" }, "managed spawn requires role selecting a canonical Gaffer preset or a complete bespoke composition"],
    ["dispatch", { id: "019f6c5e-61d0-7880-98a0-f8999eac7b03" }, "managed dispatch requires role selecting a canonical Gaffer preset or a complete bespoke composition"],
    ["spawn", { prompt: "probe", role: "verifier", model: 42 }, "model must be a non-empty string"],
    ["spawn", { prompt: "probe", role: "verifier", coordinator: { raw: "value" } }, "coordinator must be a non-empty string"],
    ["spawn", { prompt: "probe", role: "verifier", caveman: "extreme" }, "invalid caveman mode"],
  ] as const) {
    const result = spawnSync("bb", [resolve(north, "bin/north-mcp")], {
      input: `${JSON.stringify({ jsonrpc: "2.0", id: 0, method: "tools/call",
        params: { name, arguments: arguments_ } })}\n`,
      encoding: "utf8", env: { ...process.env, NORTH_MCP_BUN: "/bin/false" },
    });
    expect(result.status).toBe(0);
    const response = JSON.parse(result.stdout.trim());
    expect(response.result.isError).toBe(true);
    expect(response.result.content[0].text).toBe(expected);
  }

  for (const [field, value] of [
    ["invokedAs", "researcher"],
    ["shape", "integrate"],
    ["allocation", { mode: "preferential" }],
  ] as const) {
    const request = `${JSON.stringify({
      jsonrpc: "2.0", id: 1, method: "tools/call",
      params: { name: "spawn", arguments: { prompt: "contract probe", role: "integrator", [field]: value } },
    })}\n`;
    const result = spawnSync("bb", [resolve(north, "bin/north-mcp")], {
      input: request, encoding: "utf8", env: { ...process.env, NORTH_MCP_BUN: "/bin/false" },
    });
    expect(result.status).toBe(0);
    const response = JSON.parse(result.stdout.trim());
    expect(response.result.isError).toBe(true);
    expect(response.result.content[0].text).toContain(`unknown spawn routing field(s): ${field}`);
  }

  const topologyRequest = `${JSON.stringify({
    jsonrpc: "2.0", id: 2, method: "tools/call",
    params: { name: "spawn", arguments: { prompt: "contract probe", role: "verifier", topology: "verifier" } },
  })}\n`;
  const topologyResult = spawnSync("bb", [resolve(north, "bin/north-mcp")], {
    input: topologyRequest, encoding: "utf8", env: { ...process.env, NORTH_MCP_BUN: "/bin/false" },
  });
  const topologyResponse = JSON.parse(topologyResult.stdout.trim());
  expect(topologyResponse.result.isError).toBe(true);
  expect(topologyResponse.result.content[0].text).toBe("invalid topology");

  for (const [arguments_, expected] of [
    [{ prompt: "probe", role: "special" }, "unknown role special requires complete bespoke composition"],
    [{ prompt: "probe", role: "integrator", composition: { kind: "preset", id: "scout", overrides: [] } },
      "known role integrator requires preset composition id integrator"],
    [{ prompt: "probe", role: "researcher" }, "role researcher is retired because it was ambiguous"],
    [{ prompt: "probe", role: "orchestrator" }, "orchestrator is a topology, not a role"],
  ] as const) {
    const result = spawnSync("bb", [resolve(north, "bin/north-mcp")], {
      input: `${JSON.stringify({ jsonrpc: "2.0", id: 3, method: "tools/call",
        params: { name: "spawn", arguments: arguments_ } })}\n`,
      encoding: "utf8", env: { ...process.env, NORTH_MCP_BUN: "/bin/false" },
    });
    const response = JSON.parse(result.stdout.trim());
    expect(response.result.isError).toBe(true);
    expect(response.result.content[0].text).toContain(expected);
  }
});

test("managed MCP launch depth stops at orchestrator to worker for every composition shape", () => {
  const north = resolve(import.meta.dir, "../..");
  const orchestratorContract = {
    responsibility: "coordinate a bounded migration",
    deliverable: "integrated migration result",
    capabilities: ["coordination", "filesystem.read", "filesystem.search", "shell.readonly"],
    mayDecide: ["worker decomposition"],
    mustEscalate: ["scope expansion"],
    doneWhen: ["all worker results are reconciled"],
    report: "integrated verdict",
  };
  const shapes = [
    { role: "director" },
    {
      role: "director",
      tier: "senior",
      composition: {
        kind: "preset", id: "director", overrides: ["tier"],
        overrideReason: "bounded coordination does not require frontier tier",
      },
    },
    {
      role: "migration-director",
      taskGrade: "senior",
      domainRequirements: [],
      topology: "orchestrator",
      tier: "senior",
      reasoning: "high",
      posture: "preserve",
      composition: {
        kind: "bespoke",
        id: "migration-director",
        bespokeReason: "one-off coordination shape",
        promotionCandidate: false,
        contract: orchestratorContract,
      },
    },
  ];
  let id = 100;
  for (const name of ["spawn", "dispatch"]) {
    for (const shape of shapes) {
      const arguments_ = name === "spawn"
        ? { prompt: "must remain two tiers", ...shape }
        : { id: "depth-cap-thread", ...shape };
      const result = spawnSync("bb", [resolve(north, "bin/north-mcp")], {
        input: `${JSON.stringify({
          jsonrpc: "2.0", id: id++, method: "tools/call",
          params: { name, arguments: arguments_ },
        })}\n`,
        encoding: "utf8",
        env: {
          ...process.env,
          AGENT_TOPOLOGY: "orchestrator",
          AGENT_ID: "parent-director",
          NORTH_MCP_BUN: "/bin/false",
        },
      });
      expect(result.status).toBe(0);
      const response = JSON.parse(result.stdout.trim());
      expect(response.result.isError).toBe(true);
      expect(response.result.content[0].text).toContain(
        `coordination depth denied: ${name} from an orchestrator may create worker topology only`,
      );
    }
  }
});
