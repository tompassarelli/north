#!/usr/bin/env bun
import { existsSync, readFileSync, readdirSync, writeFileSync } from "node:fs";
import { homedir } from "node:os";
import { dirname, join, relative, resolve } from "node:path";
import { createRequire } from "node:module";

const UPSTREAM = "https://github.com/Effect-TS/effect.git";
const CONFIG = "effect-kit.json";
const HOST_RULE = /^(?:Bun\.(?:spawn|spawnSync|sleep|sleepSync)|setTimeout|spawnSync|fetch)$/;
const IMPORTS_EFFECT = /(?:from|import)\s*\(?\s*["'](?:effect|@effect\/[^"'/]+)(?:\/[^"']*)?["']/;
const EFFECT_DIAGNOSTIC = /\bTS377\d{3}\b/;
const EXIT_FLAGS = ["ignoreEffectSuggestionsInTscExitCode", "ignoreEffectWarningsInTscExitCode", "ignoreEffectErrorsInTscExitCode"];

const usage = `usage: effect-kit <command> [project-root]

  init   vendor upstream Effect at the installed version, write effect-kit.json,
         and make every Effect diagnostic fail the type-check
  check  fail on vendored-version drift, any Effect diagnostic, or a raw
         process, wait, promise or fetch outside Effect in host tools
  sync   move the vendored subtree to the installed effect version
  scan   [root] list projects under root/*/main that depend on effect but have
         no effect-kit.json (default root: the home code directory)

project-root defaults to the git top-level of the current directory.`;

export function lockedEffectVersion(lockText) {
  const match = lockText.match(/^\s*"effect":\s*\["effect@([^"]+)"/m);
  return match ? match[1] : undefined;
}

export function dependsOnEffect(packageJson) {
  return ["dependencies", "devDependencies", "peerDependencies"].some((field) => packageJson[field]?.effect !== undefined);
}

export function exitFlagProblems(plugin) {
  if (plugin === undefined) return ["tsconfig has no @effect/language-service plugin"];
  const problems = [];
  if (plugin.diagnostics === false) problems.push("plugin diagnostics are off");
  if (plugin.includeSuggestionsInTsc === false) problems.push("includeSuggestionsInTsc is false");
  if (plugin.ignoreEffectSuggestionsInTscExitCode !== false) problems.push("ignoreEffectSuggestionsInTscExitCode must be false");
  for (const flag of EXIT_FLAGS.slice(1)) if (plugin[flag] === true) problems.push(`${flag} must be false`);
  return problems;
}

export function effectDiagnostics(output) {
  return output.split("\n").filter((line) => EFFECT_DIAGNOSTIC.test(line));
}

export function hostToolViolations(ts, file, source) {
  const parsed = ts.createSourceFile(file, source, ts.ScriptTarget.Latest, true);
  const importsEffect = IMPORTS_EFFECT.test(source);
  const raw = (node) => {
    if (ts.isNewExpression(node)) return ts.isIdentifier(node.expression) && node.expression.text === "Promise";
    return ts.isCallExpression(node) && HOST_RULE.test(node.expression.getText().replace(/\s+/g, ""));
  };
  const inEffect = (node) => {
    for (let parent = node.parent; parent !== undefined; parent = parent.parent)
      if (ts.isCallExpression(parent) && /^Effect\./.test(parent.expression.getText())) return true;
    return false;
  };
  const lines = [];
  const visit = (node) => {
    if (raw(node) && (!importsEffect || !inEffect(node))) lines.push(parsed.getLineAndCharacterOfPosition(node.getStart()).line + 1);
    ts.forEachChild(node, visit);
  };
  visit(parsed);
  return lines;
}

const run = (command, cwd, options = {}) => {
  const result = Bun.spawnSync(command, { cwd, stdout: "pipe", stderr: "pipe", ...options });
  return { code: result.exitCode, out: `${result.stdout?.toString() ?? ""}${result.stderr?.toString() ?? ""}` };
};

const must = (command, cwd) => {
  const result = run(command, cwd);
  if (result.code !== 0) throw new Error(`${command.join(" ")} failed:\n${result.out}`);
  return result.out.trim();
};

const readJson = (path) => JSON.parse(readFileSync(path, "utf8"));
const writeJson = (path, value) => writeFileSync(path, `${JSON.stringify(value, null, 2)}\n`);

function projectRoot(argument) {
  if (argument) return resolve(argument);
  return must(["git", "rev-parse", "--show-toplevel"], process.cwd());
}

function findPackageDirectory(root) {
  for (const candidate of [".", "ts", "app", "web"]) {
    const path = join(root, candidate, "package.json");
    if (existsSync(path) && dependsOnEffect(readJson(path))) return candidate;
  }
  throw new Error(`no package.json depending on effect under ${root}`);
}

function loadTypeScript(packageRoot) {
  return createRequire(join(packageRoot, "package.json"))("typescript");
}

function readTsconfig(ts, path) {
  const parsed = ts.parseConfigFileTextToJson(path, readFileSync(path, "utf8"));
  if (parsed.error) throw new Error(`cannot parse ${path}`);
  return parsed.config;
}

const effectPlugin = (tsconfig) =>
  (tsconfig.compilerOptions?.plugins ?? []).find((plugin) => plugin.name === "@effect/language-service");

function installedVersion(packageRoot) {
  const lock = join(packageRoot, "bun.lock");
  if (!existsSync(lock)) throw new Error(`${lock} is missing`);
  const version = lockedEffectVersion(readFileSync(lock, "utf8"));
  if (!version) throw new Error(`${lock} does not lock effect`);
  return version;
}

function vendor(root, prefix, version) {
  const tag = `effect@${version}`;
  must(["git", "fetch", "--depth", "1", "--no-tags", UPSTREAM, `refs/tags/${tag}`], root);
  const commit = must(["git", "rev-parse", "FETCH_HEAD^{commit}"], root);
  if (existsSync(join(root, prefix))) must(["git", "rm", "-r", "-q", "--", prefix], root);
  must(["git", "read-tree", `--prefix=${prefix}/`, "-u", `${commit}^{tree}`], root);
  const metadataPath = join(root, `${prefix}.json`);
  const previous = existsSync(metadataPath) ? readJson(metadataPath) : {};
  writeJson(metadataPath, {
    ...previous,
    repository: UPSTREAM,
    prefix,
    tag,
    version,
    commit,
    license: "MIT",
    licenseFile: `${prefix}/LICENSE`,
    purpose: previous.purpose ?? "Read-only upstream source, tests and examples for agent reference",
    checkedAt: new Date().toISOString().slice(0, 10),
  });
  must(["git", "add", "--", `${prefix}.json`], root);
  return commit;
}

function loadConfig(root) {
  const path = join(root, CONFIG);
  if (!existsSync(path)) throw new Error(`${path} is missing; run effect-kit init`);
  return readJson(path);
}

function init(root) {
  const packageDirectory = findPackageDirectory(root);
  const packageRoot = join(root, packageDirectory);
  const version = installedVersion(packageRoot);
  const config = existsSync(join(root, CONFIG))
    ? loadConfig(root)
    : {
        package: packageDirectory,
        vendor: "repos/effect",
        tsconfig: "tsconfig.json",
        typecheck: ["bun", "run", "check"],
        hostTools: { include: ["scripts/**/*.ts"], exclude: ["**/*.test.ts", "**/*.tests.ts"] },
      };
  writeJson(join(root, CONFIG), config);
  const ts = loadTypeScript(packageRoot);
  const tsconfigPath = join(packageRoot, config.tsconfig);
  const tsconfig = readTsconfig(ts, tsconfigPath);
  const options = (tsconfig.compilerOptions ??= {});
  const plugins = (options.plugins ??= []);
  let plugin = effectPlugin(tsconfig);
  if (!plugin) plugins.unshift((plugin = { name: "@effect/language-service" }));
  if (exitFlagProblems(plugin).length > 0) {
    Object.assign(plugin, { diagnostics: true, includeSuggestionsInTsc: true });
    for (const flag of EXIT_FLAGS) plugin[flag] = false;
    writeJson(tsconfigPath, tsconfig);
  }
  const commit = vendor(root, config.vendor, version);
  console.log(`effect-kit: vendored effect@${version} (${commit.slice(0, 12)}) at ${config.vendor}; review and commit ${CONFIG}, ${relative(root, tsconfigPath)} and ${config.vendor}`);
}

function sync(root) {
  const config = loadConfig(root);
  const version = installedVersion(join(root, config.package));
  const metadata = readJson(join(root, `${config.vendor}.json`));
  if (metadata.version === version) return console.log(`effect-kit: ${config.vendor} already at effect@${version}`);
  const commit = vendor(root, config.vendor, version);
  console.log(`effect-kit: moved ${config.vendor} from effect@${metadata.version} to effect@${version} (${commit.slice(0, 12)}); commit the staged change`);
}

function check(root) {
  const config = loadConfig(root);
  const packageRoot = join(root, config.package);
  const problems = [];

  const version = installedVersion(packageRoot);
  const metadataPath = join(root, `${config.vendor}.json`);
  const vendored = existsSync(metadataPath) ? readJson(metadataPath).version : undefined;
  if (vendored !== version) problems.push(`version drift: ${config.vendor} is effect@${vendored ?? "missing"} but the lockfile has effect@${version}; run effect-kit sync`);
  if (!existsSync(join(root, config.vendor, "LLMS.md"))) problems.push(`${config.vendor}/LLMS.md is missing`);

  const ts = loadTypeScript(packageRoot);
  for (const problem of exitFlagProblems(effectPlugin(readTsconfig(ts, join(packageRoot, config.tsconfig)))))
    problems.push(`${config.tsconfig}: ${problem}`);

  let scanned = 0;
  const excludes = (config.hostTools.exclude ?? []).map((pattern) => new Bun.Glob(pattern));
  for (const pattern of config.hostTools.include) {
    for (const file of new Bun.Glob(pattern).scanSync(packageRoot)) {
      if (excludes.some((glob) => glob.match(file))) continue;
      scanned++;
      for (const line of hostToolViolations(ts, file, readFileSync(join(packageRoot, file), "utf8")))
        problems.push(`${config.package}/${file}:${line} raw process, wait, promise or fetch outside Effect`);
    }
  }
  if (scanned === 0) problems.push("host-tool rule matched no files");

  const typecheck = run(config.typecheck, packageRoot);
  const findings = effectDiagnostics(typecheck.out);
  for (const finding of findings) problems.push(`effect diagnostic: ${finding.trim()}`);
  if (typecheck.code !== 0 && findings.length === 0) problems.push(`${config.typecheck.join(" ")} exited ${typecheck.code}:\n${typecheck.out.trim()}`);

  for (const problem of problems) console.error(problem);
  console.log(`effect-kit check: effect@${version}, vendored ${vendored ?? "none"}, ${scanned} host tools, ${findings.length} Effect diagnostics, ${problems.length} problems`);
  return problems.length === 0 ? 0 : 1;
}

export function scan(root) {
  const missing = [];
  if (!existsSync(root)) return missing;
  for (const entry of readdirSync(root, { withFileTypes: true })) {
    const main = join(root, entry.name, "main");
    if (!entry.isDirectory() || !existsSync(main) || existsSync(join(main, CONFIG))) continue;
    for (const candidate of [".", "ts", "app", "web"]) {
      const path = join(main, candidate, "package.json");
      try {
        if (existsSync(path) && dependsOnEffect(readJson(path))) {
          missing.push(relative(root, dirname(path)) || entry.name);
          break;
        }
      } catch {}
    }
  }
  return missing;
}

if (import.meta.main) {
  const [command, argument] = process.argv.slice(2);
  try {
    if (command === "init") init(projectRoot(argument));
    else if (command === "sync") sync(projectRoot(argument));
    else if (command === "check") process.exitCode = check(projectRoot(argument));
    else if (command === "scan") {
      const root = argument ? resolve(argument) : join(homedir(), "code");
      for (const project of scan(root)) console.log(`effect-kit: ${project} depends on effect without the kit; run effect-kit init there`);
    } else {
      console.error(usage);
      process.exitCode = command === "--help" || command === "-h" ? 0 : 2;
    }
  } catch (error) {
    console.error(`effect-kit: ${error.message}`);
    process.exitCode = 1;
  }
}
