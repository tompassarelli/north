import { ToolError } from "./files";

export interface Resolver { exists: { tree: string[] } | { root: string }; manifests: { path: string; text: string }[] }

const NORTH = process.env.CODE_HEALTH_NORTH_BIN ?? "north";

export function manifestsOf(paths: Iterable<string>, read: (p: string) => string | undefined): Resolver["manifests"] {
  return [...paths].filter((p) => p === "Cargo.toml" || p.endsWith("/Cargo.toml")).map((path) => ({ path, text: read(path) ?? "" }));
}

export function importsOf(files: { path: string; text: string }[], r: Resolver): Map<string, Set<string>> {
  const run = Bun.spawnSync([NORTH, "code-health", "imports"], { stdin: Buffer.from(JSON.stringify({ ...r, files })), stdout: "pipe", stderr: "pipe" });
  if (run.exitCode !== 0) throw new ToolError(`${NORTH} code-health imports: ${run.stderr.toString().trim()}`);
  const { imports } = JSON.parse(run.stdout.toString()) as { imports: string[][] };
  return new Map(files.map((f, i) => [f.path, new Set(imports[i])]));
}
