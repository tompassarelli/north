import { expect, test } from "bun:test";
import { resolve } from "node:path";
import { pathToFileURL } from "node:url";

const spawnModule = pathToFileURL(resolve(import.meta.dir, "../src/spawn.ts")).href;

test("SIGTERMed spawn.ts appends a terminal line naming the signal", async () => {
  const script = `
    import { installSpawnTerminalHandlers } from ${JSON.stringify(spawnModule)};
    installSpawnTerminalHandlers();
    console.log("ready");
    setInterval(() => {}, 1000);
  `;
  const child = Bun.spawn(["bun", "-e", script], {
    stdout: "pipe",
    stderr: "pipe",
  });
  const reader = child.stdout.getReader();
  const first = await reader.read();
  expect(new TextDecoder().decode(first.value)).toContain("ready");
  reader.releaseLock();

  child.kill("SIGTERM");
  const [exit, stderr] = await Promise.all([
    child.exited,
    new Response(child.stderr).text(),
  ]);
  expect(exit).toBe(143);
  expect(stderr).toContain("[spawn] terminal signal=SIGTERM");
});
