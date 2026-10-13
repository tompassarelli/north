import { execFileSync } from "node:child_process";
import { readFileSync } from "node:fs";
import { hostname, homedir } from "node:os";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

export type Routine = { name: string; kind: string; schedule: string; placement: string };
export type JobRun = { name: string; host: string; last_started: string; last_ok: string | null; last_rc: number | null; open_since: string | null };
export type TimerReceipt = { name: string; live: boolean; result: string; rc: number; finished: string; error?: string };
export type MachineSnapshot = { host: "laptop" | "nexus"; observedAt: string; timers: TimerReceipt[] | null };
export type ScheduleRow = { key: string; title: string; description: string; status: "done" | "blocked" | "todo"; priority: "high" | "medium" };
export type ScheduleOptions = {
  root?: string;
  routinesDir?: string;
  laptopSnapshotPath?: string;
  snapshots?: MachineSnapshot[];
  jobs?: JobRun[];
  routines?: Routine[];
  sessionCrons?: { name: string; live: boolean; observedAt: string; error?: string }[];
  now?: Date;
  maxSnapshotAgeMs?: number;
};
const rootDefault = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
const ssh: string[] = JSON.parse(process.env.PAPERCLIP_SYNC_SSH_ARGS || '["-o","BatchMode=yes","-o","ConnectTimeout=5","nexus"]');
const redact = (text: string) => text.replace(/^.*(?:authorization|password|secret|api[_-]?key|token)\s*[=:].*$/gim, "[redacted credential line]").replace(/(?:AKIA[A-Z0-9]{16}|eyJ[A-Za-z0-9_-]+\.[A-Za-z0-9_-]+\.[A-Za-z0-9_-]+|Bearer\s+\S+|(?:sk-|gh[pousr]_|github_pat_)[A-Za-z0-9_-]+|(?:token|password|secret|api[_-]?key)\s*[=:]\s*\S+)/gi, "[redacted]").slice(-1200);
function command(argv: string[], remote = false): string {
  return execFileSync(remote ? "ssh" : argv[0], remote ? [...ssh, argv.map(a => "'" + a.replaceAll("'", "'\\''") + "'").join(" ")] : argv.slice(1), { encoding: "utf8", timeout: 20000, maxBuffer: 2 * 1024 * 1024, stdio: ["ignore", "pipe", "pipe"] });
}
function properties(text: string): Record<string, string>[] {
  return text.trim().split(/\n\n/).filter(Boolean).map(block => Object.fromEntries(block.split("\n").map(line => { const at = line.indexOf("="); return [line.slice(0, at), line.slice(at + 1)]; })));
}
export function collectMachineSnapshot(host: "laptop" | "nexus", remote = false): MachineSnapshot {
  const snapshot: MachineSnapshot = { host, observedAt: new Date().toISOString(), timers: null };
  try {
    const listed = JSON.parse(command(["systemctl", "--user", "list-timers", "--all", "--output=json", "--no-pager"], remote)) as { unit: string; activates: string }[];
    if (!Array.isArray(listed)) return snapshot;
    if (!listed.length) { snapshot.timers = []; return snapshot; }
    const units = listed.flatMap(timer => [timer.unit, timer.activates]);
    const shown = properties(command(["systemctl", "--user", "show", "-p", "Id", "-p", "ActiveState", "-p", "Result", "-p", "ExecMainStatus", "-p", "ExecMainExitTimestamp", ...units], remote));
    snapshot.timers = listed.map(timer => {
      const state = shown.find(row => row.Id === timer.unit);
      const service = shown.find(row => row.Id === timer.activates);
      const receipt: TimerReceipt = { name: timer.unit.replace(/\.timer$/, ""), live: state?.ActiveState === "active", result: service?.Result || "unknown", rc: Number(service?.ExecMainStatus || 0), finished: service?.ExecMainExitTimestamp || "" };
      if (receipt.rc !== 0 || !["success", "unknown"].includes(receipt.result)) {
        try { receipt.error = redact(command(["journalctl", "--user", "--unit", timer.activates, "-n", "8", "--no-pager", "--output=cat"], remote)); } catch { receipt.error = "unit error tail unavailable"; }
      }
      return receipt;
    });
  } catch { }
  return snapshot;
}
const hostName = (host: string) => host === process.env.PAPERCLIP_SYNC_LAPTOP_HOST ? "laptop" : host;
const timestamp = (value: string | null | undefined) => value ? Date.parse(value) || 0 : 0;
export function scheduleRows(routines: Routine[], snapshots: MachineSnapshot[], jobs: JobRun[], options: ScheduleOptions = {}): ScheduleRow[] {
  const now = (options.now || new Date()).getTime();
  const fresh = (at: string) => Number.isFinite(Date.parse(at)) && now - Date.parse(at) <= (options.maxSnapshotAgeMs ?? 15 * 60 * 1000) && Date.parse(at) <= now + 60000;
  return routines.map(routine => {
    const name = routine.name.split("@")[0];
    const matching = jobs.filter(job => job.name === name && (routine.placement === "any" || hostName(job.host) === routine.placement)).sort((a, b) => timestamp(b.last_started) - timestamp(a.last_started));
    const job = matching[0];
    const machines = snapshots.filter(s => routine.placement === "any" || s.host === routine.placement);
    const available = machines.filter(s => s.timers !== null && fresh(s.observedAt));
    const timers = available.flatMap(s => s.timers!.filter(t => t.name === name));
    const timer = timers.sort((a, b) => timestamp(b.finished) - timestamp(a.finished))[0];
    let status: ScheduleRow["status"] = "todo";
    let result = "unknown";
    let lastRun = job?.last_started || timer?.finished || "unknown";
    let error = "";
    const latestTimer = timestamp(timer?.finished) > timestamp(job?.last_started);
    if (timer && (latestTimer || (!timestamp(timer.finished) && (timer.rc !== 0 || !["success", "unknown"].includes(timer.result))))) {
      status = timer.rc !== 0 || !["success", "unknown"].includes(timer.result) ? "blocked" : timer.result === "success" ? "done" : "todo";
      result = `${timer.result} (rc ${timer.rc})`; error = timer.error || ""; lastRun = timer.finished || lastRun;
    } else if (job) {
      status = job.last_rc === null ? "todo" : job.last_rc === 0 ? "done" : "blocked";
      result = job.last_rc === null ? "running or unfinished" : `rc ${job.last_rc}`;
      if (status === "blocked") error = timer?.error || `latest ${hostName(job.host)} ledger receipt failed (rc ${job.last_rc})`;
    }
    if (routine.kind === "systemd-timer") {
      if (available.length && !timers.some(t => t.live)) { status = "blocked"; result = "not live"; error = error || "registered timer is absent or inactive"; }
      else if (!available.length) result += "; timer liveness unknown (snapshot unavailable or stale)";
    }
    if (routine.kind === "session-cron") {
      const cron = options.sessionCrons?.find(c => c.name === routine.name && fresh(c.observedAt));
      if (cron && !cron.live) { status = "blocked"; result = "not live"; error = cron.error || "active-session inventory confirms cron absent"; }
      else result += cron ? "; session cron live" : "; session cron liveness unknown";
    }
    if (routine.kind === "cloud-routine") result += "; provider schedule liveness unknown";
    return { key: `north:routine:${routine.name}`, title: routine.name, description: [`Schedule: ${routine.schedule}`, `Machine: ${routine.placement}${job ? ` (receipt: ${job.host})` : ""}`, `Kind: ${routine.kind}`, `Last run: ${lastRun}`, `Result: ${result}`, error ? `Error: ${redact(error)}` : ""].filter(Boolean).join("\n"), status, priority: status === "blocked" ? "high" : "medium" };
  });
}
export async function collectSchedules(options: ScheduleOptions = {}) {
  const root = options.root || rootDefault;
  const sources: Record<string, string> = { sessionCrons: options.sessionCrons ? "supplied" : "unavailable", cloud: "provider listing unavailable; routine inventory only" };
  let routines = options.routines;
  if (!routines) {
    const script = "import importlib.util,json,os,sys; s=importlib.util.spec_from_file_location('routines',sys.argv[1]); m=importlib.util.module_from_spec(s); s.loader.exec_module(m); os.environ['AGENT_ROUTINES_DIR']=sys.argv[2]; e,p=m.load(); print(json.dumps({'entries':list(e.values()),'problems':p}))";
    const loaded = JSON.parse(command(["python3", "-I", "-c", script, resolve(root, "harness/lib/routines.py"), options.routinesDir || resolve(homedir(), "code/south/main/agents/routines")]));
    if (!Array.isArray(loaded.entries) || loaded.problems.length) throw new Error("routine inventory invalid: " + redact(loaded.problems.join("; ")));
    routines = loaded.entries;
  }
  const local = hostname() === "nexus" ? "nexus" : "laptop";
  const snapshots = options.snapshots ? [...options.snapshots] : [collectMachineSnapshot(local)];
  if (!options.snapshots && local !== "nexus") snapshots.push(collectMachineSnapshot("nexus", true));
  if (options.laptopSnapshotPath) {
    try { const laptop = JSON.parse(readFileSync(options.laptopSnapshotPath, "utf8")); if (laptop.host !== "laptop" || !Array.isArray(laptop.timers) && laptop.timers !== null) throw new Error(); snapshots.push(laptop); } catch { sources.laptopSnapshot = "unavailable or invalid"; }
  }
  let jobs = options.jobs;
  if (!jobs) {
    try { jobs = JSON.parse(command([local === "nexus" ? resolve(root, "bin/threads") : "threads", "job-runs", "--json"], local !== "nexus")); sources.jobRuns = "native Nexus ledger"; } catch { jobs = []; sources.jobRuns = "unavailable"; }
  } else sources.jobRuns = "supplied";
  for (const snapshot of snapshots) sources[snapshot.host] = snapshot.timers === null ? "timer listing unavailable" : `snapshot ${snapshot.observedAt}`;
  return { project: { key: "north:project:schedules", name: "Schedules" }, rows: scheduleRows(routines!, snapshots, jobs!, options), sources };
}
