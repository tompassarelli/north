import { spawn } from "node:child_process";
import { writeFileSync } from "node:fs";
import { once } from "node:events";
import { ManagedQueryTermination } from "../../src/query-lifecycle";

const coordinatorPidFile = process.env.NORTH_TEST_COORDINATOR_PID_FILE;
const providerPidFile = process.env.NORTH_TEST_PROVIDER_PID_FILE;
const terminalFile = process.env.NORTH_TEST_TERMINAL_FILE;
if (!coordinatorPidFile || !providerPidFile || !terminalFile)
  throw new Error("daemonized lane lifecycle fixture paths are required");

function pidAlive(pid: number): boolean {
  try {
    process.kill(pid, 0);
    return true;
  } catch (error) {
    return (error as NodeJS.ErrnoException).code !== "ESRCH";
  }
}

async function waitForPidExit(pid: number): Promise<void> {
  while (pidAlive(pid))
    await new Promise((resolve) => setTimeout(resolve, 10));
}

const termination = new ManagedQueryTermination();
const coordinator = spawn(
  process.execPath,
  ["-e", "setInterval(() => {}, 1000)"],
  { detached: true, stdio: "ignore" },
);
if (!coordinator.pid) throw new Error("fixture coordinator did not start");
const coordinatorPid = coordinator.pid;
coordinator.unref();
writeFileSync(coordinatorPidFile, String(coordinatorPid));
termination.attachResource({
  close: async () => {
    if (pidAlive(coordinatorPid)) process.kill(coordinatorPid, "SIGTERM");
    await waitForPidExit(coordinatorPid);
  },
  forceClose: () => {
    if (pidAlive(coordinatorPid)) process.kill(coordinatorPid, "SIGKILL");
  },
});
console.log(`coordinator-boot pid=${coordinatorPid}`);

// Models the real pre-provider gap: readiness and graph writes can be waiting
// on promises backed only by unreferenced handles after the coordinator unrefs.
await new Promise<void>((resolve) => {
  const timer = setTimeout(resolve, 250);
  timer.unref();
});

const provider = spawn(
  process.execPath,
  ["-e", "console.log('provider-process-started'); setInterval(() => {}, 1000)"],
  { stdio: ["ignore", "inherit", "inherit"] },
);
if (!provider.pid) throw new Error("fixture provider did not start");
writeFileSync(providerPidFile, String(provider.pid));
console.log(`starting provider=fixture pid=${provider.pid}`);

await once(provider, "exit");
await termination.close();
termination.publicationSettled();
termination.cleanupSettled();
termination.release();
writeFileSync(terminalFile, "provider_died coordinator_reaped\n");
console.log("terminal provider_died coordinator_reaped");
