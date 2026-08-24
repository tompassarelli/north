const [javascriptPath, outputPath] = Bun.argv.slice(2);

if (javascriptPath === undefined || outputPath === undefined) {
  throw new Error("usage: generate-bridge-declarations JS OUTPUT");
}

const typedDeclarations = new Map<string, string>([
  ["run-northbridge-app!", `declare function runNorthbridgeApp(options: {
  viewId?: string;
  sourceIdentity?: string;
}): Promise<unknown>;`],
  ["boot!", `declare function boot<T>(
  runtime: T,
  launch: (prompt: string, role: string) => Promise<unknown>,
): T;`],
  ["bridge-app-launch-argv!", `declare function bridgeAppLaunchArgv(
  runtime: unknown,
  prompt: string,
  role: string,
): string[];`],
  ["launch-thread-id", `declare function launchThreadId(
  runtime: unknown,
  role: string,
): string;`],
  ["handle-local-command!", `declare function handleLocalCommand(
  runtime: unknown,
  ui: unknown,
  input: string,
): boolean;`],
  ["palette-options", "declare function paletteOptions(frame: string, query: string): Array<{ name: string }>;"],
  ["parse-bridge-stream!", `declare function parseBridgeStream(
  runtime: unknown,
  streamState: unknown,
  chunk: string,
): unknown;`],
  ["launch-route-flags", `declare function launchRouteFlags(
  provider: unknown,
  tier: unknown,
  model: unknown,
  effort: unknown,
): string[];`],
  ["set-launch-route!", `declare function setLaunchRoute(
  runtime: Record<string, unknown>,
  name: string,
  value: string,
): unknown;`],
  ["take-launch-route-flags!", `declare function takeLaunchRouteFlags(
  runtime: Record<string, unknown>,
): string[];`],
  ["project-conversation", `declare function projectConversation(
  items: unknown[],
  executionId: string,
  aggregate: boolean,
): unknown[];`],
  ["suspend-runtime!", `declare function suspendRuntime(
  runtime: unknown,
  platform: string,
  processApi: unknown,
): boolean;`],
  ["cleanup-suspend!", `declare function cleanupSuspend(
  runtime: unknown,
  processApi: unknown,
): boolean;`],
]);

function bindingName(alias: string): string {
  if (!/^[a-z][a-z0-9]*(?:-[a-z0-9]+)*[!?]?$/u.test(alias)) {
    throw new Error(`Bridge export cannot map to a TypeScript binding: ${alias}`);
  }
  const words = alias.replace(/[!?]$/u, "").split("-");
  return words[0]! + words.slice(1)
    .map((word) => word[0]!.toUpperCase() + word.slice(1))
    .join("");
}

const javascript = await Bun.file(javascriptPath).text();
const aliases = [...javascript.matchAll(
  /^export \{ [A-Za-z0-9_$]+ as "([^"]+)" \};$/gmu,
)].map((match) => match[1]!);
const emitted = new Set(aliases);
if (aliases.length === 0 || emitted.size !== aliases.length) {
  throw new Error("generated Bridge JavaScript has no exact unique export mapping");
}

const staleTyped = [...typedDeclarations.keys()].filter((alias) => !emitted.has(alias));
if (staleTyped.length > 0) {
  throw new Error(`typed Bridge declarations are stale: ${staleTyped.join(",")}`);
}

const genericDeclarations = aliases
  .filter((alias) => !typedDeclarations.has(alias))
  .map((alias) => `declare const ${bindingName(alias)}: (...args: any[]) => any;`);
const exportMappings = aliases
  .map((alias) => `  ${bindingName(alias)} as ${JSON.stringify(alias)},`);
const output = `${[
  [...typedDeclarations.values()].join("\n\n"),
  genericDeclarations.join("\n"),
  `export {\n${exportMappings.join("\n")}\n};`,
].join("\n\n")}\n`;

await Bun.write(outputPath, output);
