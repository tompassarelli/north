import { createHash } from "node:crypto";
import type { ObservationCoverage } from "./run-ledger";

export const NATIVE_COMMAND_ACTIVITY_SOURCE = "codex-app-server:item-completed";
export const NORTH_BINARY_PROBE_SCRIPT =
  'command -v north; printf "%s\\n" "$NORTH_BIN"';

const MAX_COMPLETION_EVIDENCE = 32;
const SHA256 = /^[a-f0-9]{64}$/;
const SHELL = /^\/[A-Za-z0-9_+./-]+\/(?:bash|dash|sh|zsh)$/;

export type NativeCommandStatus = "completed" | "failed" | "declined";
export type NorthBinaryProbeStatus = "passed" | "failed" | "not_observed";

export interface NativeCommandCompletionEvidence {
  commandSha256: string;
  outputSha256: string;
  status: NativeCommandStatus;
  exitCode: number;
}

export interface NativeCommandActivityObservation {
  source: string;
  coverage: ObservationCoverage;
  totalCommands?: number;
  successfulCommands?: number;
  failedCommands?: number;
  declinedCommands?: number;
  truncatedCommands?: number;
  northBinaryProbe: NorthBinaryProbeStatus;
  completions: ReadonlyArray<Readonly<NativeCommandCompletionEvidence>>;
}

export interface NativeCommandCompletion {
  id: string;
  command: string;
  cwd: string;
  source: "agent" | "userShell" | "unifiedExecStartup" | "unifiedExecInteraction";
  status: NativeCommandStatus;
  aggregatedOutput: string;
  exitCode: number;
}

function sha256(value: string): string {
  return createHash("sha256").update(value).digest("hex");
}

function shlexSingleQuote(value: string): string {
  return `'${value.replaceAll("'", `'\"'\"'`)}'`;
}

function isExactProbeCommand(command: string): boolean {
  const suffix = ` -c ${shlexSingleQuote(NORTH_BINARY_PROBE_SCRIPT)}`;
  if (!command.endsWith(suffix)) return false;
  return SHELL.test(command.slice(0, -suffix.length));
}

function isProbeCandidate(command: string, output: string, expectedOutput: string): boolean {
  return command.includes(NORTH_BINARY_PROBE_SCRIPT) || output === expectedOutput;
}

export class NativeCommandActivityAccumulator {
  private readonly calls = new Set<string>();
  private readonly open = new Set<string>();
  private readonly completions: NativeCommandCompletionEvidence[] = [];
  private terminal = false;
  private identityLoss = false;
  private successful = 0;
  private failed = 0;
  private declined = 0;
  private truncated = 0;
  private probe: NorthBinaryProbeStatus = "not_observed";

  constructor(
    private readonly expectedCwd: string,
    private readonly expectedNorthBin: string,
  ) {}

  start(id: string): boolean {
    if (!id || id.length > 256 || this.calls.has(id) || this.open.has(id)) {
      this.identityLoss = true;
      return false;
    }
    this.open.add(id);
    return true;
  }

  observe(completion: NativeCommandCompletion): boolean {
    if (!completion.id || completion.id.length > 256 || this.calls.has(completion.id)
        || !this.open.delete(completion.id)) {
      this.identityLoss = true;
      return false;
    }
    this.calls.add(completion.id);
    if (completion.status === "completed" && completion.exitCode === 0) this.successful++;
    else if (completion.status === "declined") this.declined++;
    else this.failed++;

    const evidence = Object.freeze({
      commandSha256: sha256(completion.command),
      outputSha256: sha256(completion.aggregatedOutput),
      status: completion.status,
      exitCode: completion.exitCode,
    });
    if (this.completions.length < MAX_COMPLETION_EVIDENCE) this.completions.push(evidence);
    else this.truncated++;

    const expectedOutput = `${this.expectedNorthBin}\n${this.expectedNorthBin}\n`;
    const candidate = isProbeCandidate(
      completion.command, completion.aggregatedOutput, expectedOutput,
    );
    if (!candidate) return true;
    const passed = (completion.source === "agent" || completion.source === "unifiedExecStartup")
      && completion.cwd === this.expectedCwd
      && completion.status === "completed"
      && completion.exitCode === 0
      && completion.aggregatedOutput === expectedOutput
      && isExactProbeCommand(completion.command);
    if (passed) this.probe = "passed";
    else if (this.probe !== "passed") this.probe = "failed";
    return true;
  }

  complete(): boolean {
    if (this.open.size) this.identityLoss = true;
    this.terminal = true;
    return this.open.size === 0 && !this.identityLoss;
  }
  reopen(): void { this.terminal = false; }
  invalidate(): void { this.identityLoss = true; }

  snapshot(): NativeCommandActivityObservation {
    if (!this.terminal) {
      return Object.freeze({
        source: NATIVE_COMMAND_ACTIVITY_SOURCE,
        coverage: "unknown" as const,
        northBinaryProbe: "not_observed" as const,
        completions: Object.freeze([]),
      });
    }
    const completions = Object.freeze(this.completions.map((entry) => Object.freeze({ ...entry })));
    for (const evidence of completions) {
      if (!SHA256.test(evidence.commandSha256) || !SHA256.test(evidence.outputSha256))
        throw new Error("invalid native command completion digest");
    }
    const coverage = this.identityLoss || this.truncated ? "partial" as const : "exact" as const;
    return Object.freeze({
      source: NATIVE_COMMAND_ACTIVITY_SOURCE,
      coverage,
      totalCommands: this.calls.size,
      successfulCommands: this.successful,
      failedCommands: this.failed,
      declinedCommands: this.declined,
      ...(this.truncated ? { truncatedCommands: this.truncated } : {}),
      northBinaryProbe: coverage === "exact" ? this.probe : "failed",
      completions,
    });
  }
}

export function unknownNativeCommandActivity(source: string): NativeCommandActivityObservation {
  return Object.freeze({
    source,
    coverage: "unknown",
    northBinaryProbe: "not_observed",
    completions: Object.freeze([]),
  });
}
