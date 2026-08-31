import { conj_value as $$bc$conj_value, count as $$bc$count, empty_p as $$bc$empty_p, first as $$bc$first, keyword as $$bc$keyword, property_key as $$bc$property_key, rest as $$bc$rest, str as $$bc$str } from './bridge/generated/beagle/core.js';
import { admit_host_array as $$bh$admit_host_array, admit_host_object as $$bh$admit_host_object, aget as $$bh$aget, aset as $$bh$aset, host_object as $$bh$host_object, js_obj as $$bh$js_obj } from './bridge/generated/beagle/host.js';
import { catch_dispatch as $$bd$catch_dispatch } from './bridge/generated/beagle/exception-dispatch.js';

const anthropic_sdk = require("@anthropic-ai/claude-agent-sdk");

const createSdkMcpServer = anthropic_sdk.createSdkMcpServer;

const tool = anthropic_sdk.tool;

const crypto_module = require("node:crypto");

const createHash = crypto_module.createHash;

const z = require("zod").z;

const execFileSync = require("node:child_process").execFileSync;

const fs_module = require("node:fs");

const accessSync = fs_module.accessSync;

const constants = fs_module.constants;

const existsSync = fs_module.existsSync;

const readFileSync = fs_module.readFileSync;

const readdirSync = fs_module.readdirSync;

const realpathSync = fs_module.realpathSync;

const statSync = fs_module.statSync;

const path_module = require("node:path");

const basename = path_module.basename;

const delimiter = path_module.delimiter;

const dirname = path_module.dirname;

const relative = path_module.relative;

const resolve = path_module.resolve;

const sep = path_module.sep;

const authoring_guards_module = require("./authoring-guards");

const evaluateGuards = authoring_guards_module.evaluateGuards;

const resolveManagedGuardChain = authoring_guards_module.resolveManagedGuardChain;

const recordDenial = require("./guard-log").recordDenial;

const provider_catalog_module = require("./providers/catalog");

const observeProviderContextWindow = provider_catalog_module.observeProviderContextWindow;

const resolveModelAlias = provider_catalog_module.resolveModelAlias;

const resolveModelDelta = provider_catalog_module.resolveModelDelta;

const resolveRoute = provider_catalog_module.resolveRoute;

const admitRoutingRequest = require("./routing-admission").admitRoutingRequest;

const orchestrationCapabilities = require("./orchestration-staffing").orchestrationCapabilities;

const hasAuthoringCapability = require("./orchestration-capabilities").hasAuthoringCapability;

const bespoke_module = require("./bespoke-contract");

const BESPOKE__FINGERPRINT__DOMAIN = bespoke_module.BESPOKE_FINGERPRINT_DOMAIN;

const BESPOKE__FINGERPRINT__VERSION = bespoke_module.BESPOKE_FINGERPRINT_VERSION;

const bespokeContractFingerprint = bespoke_module.bespokeContractFingerprint;

const canonicalOrchestrationCapabilities = bespoke_module.canonicalOrchestrationCapabilities;

const assertCoordinationAuthority = require("./topology-authority").assertCoordinationAuthority;

const readonly_shell_module = require("./readonly-shell");

const MAX__READONLY__COMMAND__BYTES = readonly_shell_module.MAX_READONLY_COMMAND_BYTES;

const READONLY__SHELL__SERVER = readonly_shell_module.READONLY_SHELL_SERVER;

const READONLY__SHELL__TOOL = readonly_shell_module.READONLY_SHELL_TOOL;

const runReadonlyShell = readonly_shell_module.runReadonlyShell;

const managedNorthMcpEnvironment = require("./execution-admission").managedNorthMcpEnvironment;

const managedCodexNetworkPolicy = require("./providers/codex-network-policy").managedCodexNetworkPolicy;

const requireJudgmentGrade = require("./judgment-grade").requireJudgmentGrade;

const providerModelObservationPath = require("./provider-model-observation-store").providerModelObservationPath;

const composition_receipt_module = require("./composition-receipt");

const buildEnvironmentReceipt = composition_receipt_module.buildEnvironmentReceipt;

const buildPromptReceipt = composition_receipt_module.buildPromptReceipt;

const canonicalReceiptJson = composition_receipt_module.canonicalReceiptJson;

const sha256Bytes = composition_receipt_module.sha256Bytes;

const beagle_store_module = require("./beagle-store");

const beagleStoreBabashkaArguments = beagle_store_module.beagleStoreBabashkaArguments;

const beagleStoreCoordinatorChildTimeout = beagle_store_module.beagleStoreCoordinatorChildTimeout;

const beagleStoreEnvironment = beagle_store_module.beagleStoreEnvironment;

const presence_fence_module = require("./presence-fence");

const loadPresenceFence = presence_fence_module.loadPresenceFence;

const parsePresenceFence = presence_fence_module.parsePresenceFence;

const persistPresenceFence = presence_fence_module.persistPresenceFence;

const presenceFenceJson = presence_fence_module.presenceFenceJson;

const REPO = resolve(import.meta.dir, "../..");

const ENGINE = $$bc$str(REPO, "/bin/north");

const MCP = $$bc$str(REPO, "/bin/north-mcp");

const MSG_CLI = $$bc$str(REPO, "/cli/msg-cli.clj");

function is_store_repo_p(repo) {
  return repo.startsWith("/nix/store/");
}

const STABLE_SYSTEM_BIN = ["", "run", "current-system", "sw", "bin"].join("/");

const STABLE_SYSTEM_BINARIES = new Set(["north", "concern"]);

function stable_bin_path(...$beagle$args) {
  if (arguments.length === 1) {
    const name = $beagle$args[0];
    return stable_bin_path(name, REPO);
  }
  if (arguments.length === 2) {
    const name = $beagle$args[0];
    const repo = $beagle$args[1];
    return ((!is_store_repo_p(repo)) ? $$bc$str(repo, "/bin/", name) : (((_truthy) => _truthy !== false && _truthy != null)(STABLE_SYSTEM_BINARIES.has(name)) ? $$bc$str(STABLE_SYSTEM_BIN, "/", name) : name));
  }
  throw new Error('No matching arity: ' + $beagle$args.length);
}

function eso_spec_path(...$beagle$args) {
  if (arguments.length === 0) {
    return eso_spec_path(process.env, REPO);
  }
  if (arguments.length === 1) {
    const env = $beagle$args[0];
    return eso_spec_path(env, REPO);
  }
  if (arguments.length === 2) {
    const env = $beagle$args[0];
    const repo = $beagle$args[1];
    const relative_path = "sdk/src/vendor/eso/SPEC.md";
    if ((!is_store_repo_p(repo))) {
      return $$bc$str(repo, "/", relative_path);
    } else {
      const checkout = env.NORTH_HOME;
      if (((_truthy) => _truthy !== false && _truthy != null)(((_logical) => (_logical !== false && _logical != null ? (!is_store_repo_p(checkout)) : _logical))(checkout))) {
        const candidate = resolve(checkout, relative_path);
        return (existsSync(candidate) ? candidate : $$bc$str(repo, "/", relative_path));
      } else {
        return $$bc$str(repo, "/", relative_path);
      }
    }
  }
  throw new Error('No matching arity: ' + $beagle$args.length);
}

function north_port() {
  return ((_logical) => (_logical !== false && _logical != null ? _logical : "7977"))(process.env.NORTH_PORT);
}

function peer_bb() {
  return ((_logical) => (_logical !== false && _logical != null ? _logical : "bb"))(process.env.NORTH_PEER_BB);
}

function current_path_executable(...$beagle$args) {
  if (arguments.length === 1) {
    const name = $beagle$args[0];
    return current_path_executable(name, process.env);
  }
  if (arguments.length === 2) {
    const name = $beagle$args[0];
    const env = $beagle$args[1];
    return (((_truthy) => _truthy !== false && _truthy != null)(name.includes("/")) ? name : (() => { let directories = ((_logical) => (_logical !== false && _logical != null ? _logical : ""))(env.PATH).split(delimiter); while (true) {
    if ($$bc$empty_p(directories)) { return name; } else { const directory = $$bc$first(directories); if ((directory === "")) { const _recur_0 = $$bc$rest(directories); directories = _recur_0; continue; } else { const candidate = resolve(directory, name); { let _loop_try_result_0; try {
    _loop_try_result_0 = (() => { accessSync(candidate, constants.X_OK);
return candidate; })();
  } catch (_catch_0) {
    switch ($$bd$catch_dispatch(_catch_0, [Error])) {
      case 0: {
        const __error = _catch_0;
        const _recur_0 = $$bc$rest(directories); directories = _recur_0; continue;
        break;
      }
    }
  } return _loop_try_result_0; } } }
  } })());
  }
  throw new Error('No matching arity: ' + $beagle$args.length);
}

function presence_bb() {
  return current_path_executable(((_logical) => (_logical !== false && _logical != null ? _logical : "bb"))(process.env.NORTH_PEER_BB));
}

function edn_value(value) {
  if (((typeof value === "number") || (typeof value === "boolean"))) {
    return $$bc$str(value);
  } else {
    const text = (((typeof value === "object") && (!(value == null))) ? JSON.stringify(value) : $$bc$str(value));
    return (((_truthy) => _truthy !== false && _truthy != null)(new RegExp("^[@:]", "u").test(text)) ? text : JSON.stringify(text));
  }
}

function edn_args(args) {
  return $$bc$str("{", Object.entries(args).map((entry) => $$bc$str(":", (($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_array($beagle$host$arg$0), $beagle$host$arg$1))(entry, 0), " ", edn_value((($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_array($beagle$host$arg$0), $beagle$host$arg$1))(entry, 1)))).join(" "), "}");
}

const PEER_ROUTING_FIELDS = ["role", "taskGrade", "domainRequirements", "topology", "capabilityFloor", "serviceClass", "reasoning", "posture", "composition"];

const PEER_ROUTE_ADAPTER_FIELDS = ["provider", "target", "model"];

function exact_peer_fields_bang(args, allowed, operation) {
  const unknown = Object.keys(args).filter((field) => (!((_truthy) => _truthy !== false && _truthy != null)(allowed.includes(field))));
  if (($$bc$count(unknown) > 0)) {
    (() => { throw new Error($$bc$str(operation, " has unknown field(s): ", unknown.join(", "))); })();
  }
  return null;
}

function non_empty_object_string_p(args, field) {
  const value = (($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1))(args, field);
  return ((typeof value === "string") && (!(value.trim() === "")));
}

function validate_peer_command_args_bang(op, args) {
  if (((args == null) || ((!(typeof args === "object")) || Array.isArray(args)))) {
    (() => { throw new Error($$bc$str(op, " args must be an object")); })();
  }
  if ((op === "tell")) {
    exact_peer_fields_bang(args, ["id", "pred", "value"], op);
    if ((!((_truthy) => _truthy !== false && _truthy != null)(["id", "pred", "value"].every((field) => non_empty_object_string_p(args, field))))) {
      (() => { throw new Error("tell requires id, pred, and value"); })();
    }
    if ((args.pred === "judgment_grade")) {
      requireJudgmentGrade(args.value);
    }
  } else if ((op === "acquire")) {
    exact_peer_fields_bang(args, ["resource", "holder"], op);
    if ((!non_empty_object_string_p(args, "resource"))) {
      (() => { throw new Error("acquire requires resource"); })();
    }
  } else {
    const work_field = ((op === "spawn") ? "prompt" : "thread");
    const allowed = [work_field].concat(PEER_ROUTING_FIELDS, PEER_ROUTE_ADAPTER_FIELDS);
    exact_peer_fields_bang(args, allowed, op);
    if (((!non_empty_object_string_p(args, work_field)) || (!non_empty_object_string_p(args, "role")))) {
      (() => { throw new Error($$bc$str(op, " requires ", work_field, " and an explicit Orchestration role")); })();
    }
    const present_routing = PEER_ROUTING_FIELDS.filter((field) => Object.hasOwn(args, field));
    if ((!($$bc$count(present_routing) === $$bc$count(PEER_ROUTING_FIELDS)))) {
      const missing = PEER_ROUTING_FIELDS.filter((field) => (!((_truthy) => _truthy !== false && _truthy != null)(Object.hasOwn(args, field))));
      (() => { throw new Error($$bc$str(op, " requires the complete nine-field Orchestration request; missing: ", missing.join(", "), " (recover the valid payload shape: north show @contract:dispatch)")); })();
    }
    const metadata = $$bh$js_obj();
    present_routing.forEach((field) => {
  (($beagle$host$arg$0, $beagle$host$arg$1, $beagle$host$arg$2) => $$bh$aset($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1, $beagle$host$arg$2))(metadata, field, (($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1))(args, field));
});
    admitRoutingRequest(metadata, $$bc$str("managed peer ", op));
  }
  return null;
}

const validatePeerCommandArgs = validate_peer_command_args_bang;

function send_peer_command_bang(self, to, op, args) {
  assertCoordinationAuthority($$bc$str("command_peer:", op));
  if (((op === "spawn") || (op === "dispatch"))) {
    (() => { throw new Error($$bc$str("peer ", op, " is unsupported until atomic command claim + child reconciliation land; use North MCP/CLI ", op)); })();
  }
  validate_peer_command_args_bang(op, args);
  return execFileSync(peer_bb(), beagleStoreBabashkaArguments([MSG_CLI, north_port(), "send-cmd", self, to, op, edn_args(args)]), $$bh$host_object($$bc$keyword("encoding"), "utf8", $$bc$keyword("env"), beagleStoreEnvironment(), $$bc$keyword("timeout"), beagleStoreCoordinatorChildTimeout()));
}

const sendPeerCommand = send_peer_command_bang;

const COORDINATION__TOOLS = ["mcp__north__capture", "mcp__north__tell", "mcp__north__evidence_record", "mcp__north__show", "mcp__north__search", "mcp__north__artifact_read", "mcp__north__ready", "mcp__north__next"];

const ORCHESTRATION__TOOLS = ["mcp__north__dispatch", "mcp__north__spawn", "mcp__north-peer__command_peer"];

const NATIVE__AGENT__TOOLS = ["Agent", "Task", "Workflow"];

const NORTH__MCP__TOOL__NAMES = ["ready", "next", "blocked", "agenda", "leverage", "needs_review", "validate", "show", "search", "artifact_read", "capture", "tell", "evidence_record", "retract", "presentation", "linear_get", "linear_import", "linear_plan", "linear_sync", "dispatch", "spawn"];

const ALL_NORTH_MCP_TOOLS = NORTH__MCP__TOOL__NAMES.map((name) => $$bc$str("mcp__north__", name));

const CAPABILITY_TOOLS = $$bh$host_object("filesystem.read", ["Read"], "filesystem.search", ["Grep", "Glob"], "filesystem.write", ["Edit", "Write", "NotebookEdit"], "shell", ["Bash"], "shell.readonly", [READONLY__SHELL__TOOL], "web", ["WebSearch", "WebFetch"], "coordination", ORCHESTRATION__TOOLS);

const ALL_CAPABILITY_TOOLS = Array.from(new Set(Object.values(CAPABILITY_TOOLS).flat()));

function managedToolPolicy(capabilities) {
  const selected_capability_tools = Array.from(new Set(capabilities.flatMap((capability) => (($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1))(CAPABILITY_TOOLS, capability))));
  const orchestration_allowed = capabilities.includes("coordination");
  const allowed_tools = Array.from(new Set(selected_capability_tools.concat(COORDINATION__TOOLS, (orchestration_allowed ? ORCHESTRATION__TOOLS : []))));
  const disallowed_tools = Array.from(new Set(NATIVE__AGENT__TOOLS.concat(ALL_CAPABILITY_TOOLS.filter((tool_name) => (!((_truthy) => _truthy !== false && _truthy != null)(selected_capability_tools.includes(tool_name)))), ALL_NORTH_MCP_TOOLS.filter((tool_name) => (!((_truthy) => _truthy !== false && _truthy != null)(allowed_tools.includes(tool_name)))), (orchestration_allowed ? [] : ["mcp__north-peer__command_peer"]))));
  return $$bh$host_object($$bc$keyword("tools"), selected_capability_tools.filter((tool_name) => (!((_truthy) => _truthy !== false && _truthy != null)(tool_name.startsWith("mcp__")))), $$bc$keyword("allowedTools"), allowed_tools, $$bc$keyword("disallowedTools"), disallowed_tools);
}

function error_message(error) {
  return ((_logical) => (_logical !== false && _logical != null ? _logical : ((_logical) => (_logical !== false && _logical != null ? _logical : $$bc$str(error)))(error.message)))(error.stderr);
}

function peer_command_handler_bang(self, input) {
  return (() => { try {
    const to = input.to;
  const op = input.op;
  const args = input.args;
  const output = send_peer_command_bang(self, to, op, args);
  return $$bh$host_object($$bc$keyword("content"), [$$bh$host_object($$bc$keyword("type"), "text", $$bc$keyword("text"), $$bc$str("sent {:op :", op, "} -> ", to, "\n", output).trim())]);
  } catch (_catch_1) {
    switch ($$bd$catch_dispatch(_catch_1, [Error])) {
      case 0: {
        const error = _catch_1;
        return $$bh$host_object($$bc$keyword("content"), [$$bh$host_object($$bc$keyword("type"), "text", $$bc$keyword("text"), $$bc$str("command_peer failed: ", error_message(error)))], $$bc$keyword("isError"), true);
        break;
      }
    }
  } })();
}

function peer_command_server_bang(self) {
  return createSdkMcpServer($$bh$host_object($$bc$keyword("name"), "north-peer", $$bc$keyword("version"), "0.1.0", $$bc$keyword("tools"), [tool("command_peer", $$bc$str("Command a peer over the North fact feed with repeat-safe operations: ", "tell {id, pred, value} | acquire {resource}. Managed spawn/dispatch ", "use North's canonical MCP/CLI tools."), $$bh$host_object($$bc$keyword("to"), z.string().describe("exact recipient agent handle or held role; use literal '*' to broadcast"), $$bc$keyword("op"), z.enum(["tell", "acquire"]), $$bc$keyword("args"), z.record(z.string(), z.unknown()).describe("op-specific repeat-safe fact arguments")), (input) => peer_command_handler_bang(self, input))]));
}

const peerCommandServer = peer_command_server_bang;

async function readonly_shell_handler(cwd, environment, abort_signal, input) {
  return (async () => { try {
    const result = await runReadonlyShell(input.command, cwd, input.timeoutMs, environment, abort_signal);
  return (((_truthy) => _truthy !== false && _truthy != null)(result.ok) ? $$bh$host_object($$bc$keyword("content"), [$$bh$host_object($$bc$keyword("type"), "text", $$bc$keyword("text"), JSON.stringify(result))]) : $$bh$host_object($$bc$keyword("content"), [$$bh$host_object($$bc$keyword("type"), "text", $$bc$keyword("text"), JSON.stringify(result))], $$bc$keyword("isError"), true));
  } catch (_catch_2) {
    switch ($$bd$catch_dispatch(_catch_2, [Error])) {
      case 0: {
        const error = _catch_2;
        return $$bh$host_object($$bc$keyword("content"), [$$bh$host_object($$bc$keyword("type"), "text", $$bc$keyword("text"), JSON.stringify($$bh$host_object($$bc$keyword("ok"), false, $$bc$keyword("error"), ((_logical) => (_logical !== false && _logical != null ? _logical : "readonly_shell_unavailable"))(error.code), $$bc$keyword("message"), ((_logical) => (_logical !== false && _logical != null ? _logical : $$bc$str(error)))(error.message))))], $$bc$keyword("isError"), true);
        break;
      }
    }
  } })();
}

function readonly_shell_server(cwd, environment, abort_signal) {
  return createSdkMcpServer($$bh$host_object($$bc$keyword("name"), READONLY__SHELL__SERVER, $$bc$keyword("version"), "0.1.0", $$bc$keyword("tools"), [tool("run", $$bc$str("Run one command in North's network-isolated read-only shell. The checkout and host ", "filesystem are read-only; only an ephemeral /tmp is writable."), $$bh$host_object($$bc$keyword("command"), z.string().min(1).max(MAX__READONLY__COMMAND__BYTES).describe("Command interpreted intentionally by bash -lc inside the read-only sandbox"), $$bc$keyword("timeoutMs"), z.number().finite().int().min(100).max(120000).optional().describe("Bounded command timeout in milliseconds (default: 30000; maximum: 120000)")), (input) => readonly_shell_handler(cwd, environment, abort_signal, input))]));
}

function register_presence(self, cwd) {
  const output = execFileSync(presence_bb(), beagleStoreBabashkaArguments([$$bc$str(REPO, "/cli/presence-cli.clj"), north_port(), "register", self, cwd, self]), $$bh$host_object($$bc$keyword("env"), beagleStoreEnvironment(), $$bc$keyword("encoding"), "utf8", $$bc$keyword("stdio"), ["ignore", "pipe", "pipe"], $$bc$keyword("timeout"), beagleStoreCoordinatorChildTimeout(5000)));
  const fence = ((process.env.NORTH_IDENTITY_TEST_REDIRECT === "1") ? $$bh$host_object($$bc$keyword("resource"), $$bc$str("session:", self), $$bc$keyword("holder"), self, $$bc$keyword("epoch"), 1) : parsePresenceFence(output, self));
  persistPresenceFence(self, fence);
  return null;
}

const RENEW_THROTTLE_MS = 60000;

const last_renew = new Map();

function renewal_error_lines(error) {
  const stderr = error.stderr;
  const text = ((typeof stderr === "string") ? stderr : (((_truthy) => _truthy !== false && _truthy != null)(stderr) ? stderr.toString() : ""));
  return text.split("\n").map((line) => line.trim()).filter((line) => (!(line === "")));
}

function first_line_with(lines, prefix) {
  return lines.find((line) => line.startsWith(prefix));
}

function renew_presence(self) {
  const now = Date.now();
  const previous = ((_logical) => (_logical !== false && _logical != null ? _logical : 0))(last_renew.get(self));
  if (((now - previous) < RENEW_THROTTLE_MS)) {
    return null;
  } else {
    last_renew.set(self, now);
    return (() => { try {
    const fence = loadPresenceFence(self);
  const output = execFileSync(presence_bb(), beagleStoreBabashkaArguments([$$bc$str(REPO, "/cli/presence-cli.clj"), north_port(), "renew", self, presenceFenceJson(fence)]), $$bh$host_object($$bc$keyword("env"), beagleStoreEnvironment(), $$bc$keyword("encoding"), "utf8", $$bc$keyword("stdio"), ["ignore", "pipe", "pipe"], $$bc$keyword("timeout"), beagleStoreCoordinatorChildTimeout(5000)));
  const redirected = (process.env.NORTH_IDENTITY_TEST_REDIRECT === "1");
  const renewed = (redirected ? fence : parsePresenceFence(output, self));
  if (((!redirected) && (renewed.epoch <= fence.epoch))) {
    (() => { throw new Error("liveness renewal did not advance the exact lease fence"); })();
  }
  persistPresenceFence(self, renewed);
  if ((process.env.NORTH_PRESENCE_DEBUG === "1")) {
    console.error($$bc$str("[liveness] @agent:", self, " lease renewed (activity heartbeat)"));
  }
  return null;
  } catch (_catch_3) {
    switch ($$bd$catch_dispatch(_catch_3, [Error])) {
      case 0: {
        const error = _catch_3;
        if ((last_renew.get(self) === now)) {
          last_renew.set(self, previous);
        }
        const lines = renewal_error_lines(error);
        const detail = ((_logical) => (_logical !== false && _logical != null ? _logical : ((_logical) => (_logical !== false && _logical != null ? _logical : "lease helper failed without a diagnostic"))(first_line_with(lines, "Message:"))))(first_line_with(lines, "north lease-internal:"));
        console.error($$bc$str("[liveness] @agent:", self, " lease renewal FAILED — lane continues, roster may read lapsed: ", detail));
        return null;
        break;
      }
    }
  } })();
  }
}

function managed_codex_shell_boundary(...$beagle$args) {
  if (arguments.length === 0) {
    return managed_codex_shell_boundary([]);
  }
  if (arguments.length === 1) {
    const capabilities = $beagle$args[0];
    const network = managedCodexNetworkPolicy($$bh$host_object($$bc$keyword("sandbox"), (((_truthy) => _truthy !== false && _truthy != null)(capabilities.includes("shell.readonly")) ? "read-only" : "workspace-write"), $$bc$keyword("capabilities"), capabilities));
    const network_boundary = (((!((_truthy) => _truthy !== false && _truthy != null)(network.networkAccess))) ? "Your shell has NO network." : (((_truthy) => _truthy !== false && _truthy != null)(capabilities.includes("web"))) ? $$bc$str("Your shell has network access; the managed web proxy is limited to ", "chromium.googlesource.com.") : $$bc$str("Your shell has network access; the managed web proxy remains ", "disabled."));
    return ["", "", "## managed Codex sandbox — your actual write paths", $$bc$str(network_boundary, " Every North CLI that writes the graph (`north tell`,"), "`north evidence record`, `bin/concern …`) talks to the coordinator over a socket,", "so from your shell it fails — a graph write attempted that way is a lost write.", "Write the graph with the north MCP tools instead (tell, evidence_record, capture,", "show, ready, next). They run outside the sandbox and are the ONLY graph path", "you have — use the MCP `show` and catalog tools to READ too, not the shell CLI.", "Your workspace IS writable, including its git metadata: stage and COMMIT on your", "lane branch. You cannot push and must not try — your commits are", "harvested to the canonical checkout when the lane settles, and the coordinator lands", "them. Anything outside the workspace is read-only."].join("\n");
  }
  throw new Error('No matching arity: ' + $beagle$args.length);
}

function coordination_block(self, cwd, provider, capabilities) {
  const segments = cwd.split("/").filter((segment) => (!(segment === "")));
  const repo = ((_logical) => (_logical !== false && _logical != null ? _logical : "repo"))((() => { const _x = segments; return _x[_x.length - 1]; })());
  const protocol = ["", "", "## north coordination", $$bc$str("You are agent \"", self, "\" in \"", repo, "\". Other agents may work here concurrently."), "Coordinate through CONCERNS, not locks — work coexists; declaring never blocks. Before", "editing code for a feature, declare it so others can see + shape around your work:", $$bc$str("  ", stable_bin_path("concern"), " declare ", self, " ", repo, " \"<what you're building>\" <file1,file2,...>"), $$bc$str("  ", stable_bin_path("concern"), " overlap <id>   # who's in your footprint; likely-to-land marked — build against it"), $$bc$str("  ", stable_bin_path("concern"), " candidate <id> [git-rev] · done <id> · ls [repo]"), "", "Internal notes / status / scratch / handoffs -> docs/private/ (gitignored), NEVER public docs/.", $$bc$str("Run `", stable_bin_path("ensure-private-docs"), "` to set up the ignore in a repo before writing there.")].join("\n");
  return ((provider === "openai") ? $$bc$str(protocol, managed_codex_shell_boundary(capabilities)) : protocol);
}

function eso_appendix(...$beagle$args) {
  if (arguments.length === 0) {
    return eso_appendix(process.env);
  }
  if (arguments.length === 1) {
    const env = $beagle$args[0];
    return ((!(((_logical) => (_logical !== false && _logical != null ? _logical : "on"))(env.AGENT_ESO) === "on")) ? "" : $$bc$str("\n\nDENSE HANDOFF — when a final report contains a uniform array of ≥10 similar records ", "(grep hits, findings, file lists), emit it in ESO format instead of JSON or markdown table.\n", "Mini-syntax (full spec: ", eso_spec_path(), "):\n", "  !eso/1              ← required header\n", "  name=value          ← scalar field\n", "  items[N]{a,b,c}     ← N records, schema declared once; N is a checksum\n", "  val1\\tval2\\tval3   ← one tab-delimited row per record (strings with tabs/newlines use JSON quoting)"));
  }
  throw new Error('No matching arity: ' + $beagle$args.length);
}

const GLOBAL__AGENTS__MAX__BYTES = (32 * 1024);

function agent_laws_enabled_p(...$beagle$args) {
  if (arguments.length === 0) {
    return agent_laws_enabled_p(process.env);
  }
  if (arguments.length === 1) {
    const env = $beagle$args[0];
    const mode = ((_logical) => (_logical !== false && _logical != null ? _logical : "on"))(env.AGENT_LAWS);
    return (((mode === "on")) ? true : ((mode === "off")) ? false : (() => { throw new Error("AGENT_LAWS must be exactly 'on' or 'off'"); })());
  }
  throw new Error('No matching arity: ' + $beagle$args.length);
}

function error_with_cause(message, cause) {
  return new Error(message, {[$$bc$property_key($$bc$keyword("cause"))]: cause});
}

function read_global_agents(path, label) {
  const info = (() => { try {
    return statSync(path);
  } catch (_catch_4) {
    switch ($$bd$catch_dispatch(_catch_4, [Error])) {
      case 0: {
        const cause = _catch_4;
        return (() => { throw error_with_cause($$bc$str("global AGENTS bootstrap cannot inspect ", label, ": ", path), cause); })();
        break;
      }
    }
  } })();
  if ((!((_truthy) => _truthy !== false && _truthy != null)(info.isFile()))) {
    (() => { throw new Error($$bc$str("global AGENTS bootstrap ", label, " is not a regular file: ", path)); })();
  }
  if ((info.size > GLOBAL__AGENTS__MAX__BYTES)) {
    (() => { throw new Error($$bc$str("global AGENTS bootstrap exceeds ", GLOBAL__AGENTS__MAX__BYTES, " bytes at: ", path)); })();
  }
  const bytes = (() => { try {
    return readFileSync(path);
  } catch (_catch_5) {
    switch ($$bd$catch_dispatch(_catch_5, [Error])) {
      case 0: {
        const cause = _catch_5;
        return (() => { throw error_with_cause($$bc$str("global AGENTS bootstrap cannot read ", label, ": ", path), cause); })();
        break;
      }
    }
  } })();
  if ((bytes.byteLength > GLOBAL__AGENTS__MAX__BYTES)) {
    (() => { throw new Error($$bc$str("global AGENTS bootstrap exceeds ", GLOBAL__AGENTS__MAX__BYTES, " bytes at: ", path)); })();
  }
  const text = (() => { try {
    return new TextDecoder("utf-8", {[$$bc$property_key($$bc$keyword("fatal"))]: true}).decode(bytes);
  } catch (_catch_6) {
    switch ($$bd$catch_dispatch(_catch_6, [Error])) {
      case 0: {
        const cause = _catch_6;
        return (() => { throw error_with_cause($$bc$str("global AGENTS bootstrap is not valid UTF-8 at: ", path), cause); })();
        break;
      }
    }
  } })();
  if ((text.trim() === "")) {
    (() => { throw new Error($$bc$str("global AGENTS bootstrap is empty at: ", path)); })();
  }
  return $$bh$host_object($$bc$keyword("bytes"), bytes, $$bc$keyword("text"), text);
}

function globalLawsPath(...$beagle$args) {
  if (arguments.length === 0) {
    return globalLawsPath(process.env);
  }
  if (arguments.length === 1) {
    const env = $beagle$args[0];
    const override = ((_logical) => (_logical !== false && _logical != null ? _logical : ""))(env.AGENT_LAWS_PATH).trim();
    if ((!(override === ""))) {
      return resolve(override);
    } else {
      const home = ((_logical) => (_logical !== false && _logical != null ? _logical : ""))(env.HOME).trim();
      if ((home === "")) {
        (() => { throw new Error("global AGENTS bootstrap requires AGENT_LAWS_PATH or HOME"); })();
      }
      return resolve(home, ".agents", "AGENTS.md");
    }
  }
  throw new Error('No matching arity: ' + $beagle$args.length);
}

function canonicalGlobalAgents(...$beagle$args) {
  if (arguments.length === 0) {
    return canonicalGlobalAgents(process.env);
  }
  if (arguments.length === 1) {
    const env = $beagle$args[0];
    if ((!agent_laws_enabled_p(env))) {
      return null;
    } else {
      const path = globalLawsPath(env);
      const source = read_global_agents(path, "canonical source");
      const canonical_path = (() => { try {
    return realpathSync(path);
  } catch (_catch_7) {
    switch ($$bd$catch_dispatch(_catch_7, [Error])) {
      case 0: {
        const cause = _catch_7;
        return (() => { throw error_with_cause($$bc$str("global AGENTS bootstrap cannot resolve canonical source: ", path), cause); })();
        break;
      }
    }
  } })();
      return $$bh$host_object($$bc$keyword("path"), path, $$bc$keyword("realpath"), canonical_path, $$bc$keyword("bytes"), source.bytes, $$bc$keyword("text"), source.text);
    }
  }
  throw new Error('No matching arity: ' + $beagle$args.length);
}

function global_laws_appendix(...$beagle$args) {
  if (arguments.length === 0) {
    return global_laws_appendix(process.env);
  }
  if (arguments.length === 1) {
    const env = $beagle$args[0];
    const laws = canonicalGlobalAgents(env);
    return ((laws == null) ? "" : $$bc$str("\n\n## Global laws — ", laws.path, " (binds every provider and agent)\n\n", laws.text, (((_truthy) => _truthy !== false && _truthy != null)(laws.text.endsWith("\n")) ? "" : "\n")));
  }
  throw new Error('No matching arity: ' + $beagle$args.length);
}

const PROJECT__AGENTS__MAX__BYTES = (32 * 1024);

function git_root_for_project(cwd) {
  const canonical_cwd = (() => { try {
    const resolved = realpathSync(cwd);
  if ((!((_truthy) => _truthy !== false && _truthy != null)(statSync(resolved).isDirectory()))) {
    (() => { throw new Error($$bc$str("working directory is not a directory: ", resolved)); })();
  }
  return resolved;
  } catch (_catch_8) {
    switch ($$bd$catch_dispatch(_catch_8, [Error])) {
      case 0: {
        const cause = _catch_8;
        return (() => { throw error_with_cause($$bc$str("project AGENTS bootstrap cannot resolve cwd: ", cwd), cause); })();
        break;
      }
    }
  } })();
  return (() => { let cursor = canonical_cwd; while (true) {
    const marker = resolve(cursor, ".git"); { let _loop_try_result_1; try {
    _loop_try_result_1 = (() => { const marker_stat = statSync(marker); if (((!((_truthy) => _truthy !== false && _truthy != null)(marker_stat.isDirectory())) && (!((_truthy) => _truthy !== false && _truthy != null)(marker_stat.isFile())))) {
  (() => { throw new Error($$bc$str("Git marker is neither file nor directory: ", marker)); })();
}
return {[$$bc$property_key($$bc$keyword("cwd"))]: canonical_cwd, [$$bc$property_key($$bc$keyword("root"))]: cursor}; })();
  } catch (_catch_9) {
    switch ($$bd$catch_dispatch(_catch_9, [Error])) {
      case 0: {
        const error = _catch_9;
        if ((!(error.code === "ENOENT"))) { _loop_try_result_1 = (() => { throw error_with_cause($$bc$str("project AGENTS bootstrap cannot inspect Git marker: ", marker), error); })(); } else { const parent = dirname(cursor); if ((parent === cursor)) { _loop_try_result_1 = {[$$bc$property_key($$bc$keyword("cwd"))]: canonical_cwd, [$$bc$property_key($$bc$keyword("root"))]: canonical_cwd}; } else { const _recur_0 = parent; cursor = _recur_0; continue; } }
        break;
      }
    }
  } return _loop_try_result_1; }
  } })();
}

function project_instruction_file(directory) {
  return (() => { let names = ["AGENTS.override.md", "AGENTS.md"]; while (true) {
    if ($$bc$empty_p(names)) { return null; } else { const path = resolve(directory, $$bc$first(names)); { let _loop_try_result_2; try {
    _loop_try_result_2 = (() => { const info = statSync(path); if ((!((_truthy) => _truthy !== false && _truthy != null)(info.isFile()))) {
  (() => { throw new Error($$bc$str("project instruction source is not a regular file: ", path)); })();
}
if ((info.size > PROJECT__AGENTS__MAX__BYTES)) {
  (() => { throw new Error($$bc$str("project AGENTS bootstrap exceeds ", PROJECT__AGENTS__MAX__BYTES, " bytes at: ", path)); })();
}
return path; })();
  } catch (_catch_10) {
    switch ($$bd$catch_dispatch(_catch_10, [Error])) {
      case 0: {
        const error = _catch_10;
        if ((error.code === "ENOENT")) { const _recur_0 = $$bc$rest(names); names = _recur_0; continue; } else { _loop_try_result_2 = (() => { throw error_with_cause($$bc$str("project AGENTS bootstrap cannot inspect: ", path), error); })(); }
        break;
      }
    }
  } return _loop_try_result_2; } }
  } })();
}

function within_path_p(parent, child) {
  const from_parent = relative(parent, child);
  return ((!(from_parent === "..")) && ((!((_truthy) => _truthy !== false && _truthy != null)(from_parent.startsWith($$bc$str("..", sep)))) && (!((_truthy) => _truthy !== false && _truthy != null)(from_parent.startsWith(sep)))));
}

function instruction_directories(policy_root, cwd) {
  const rel = relative(policy_root, cwd);
  if (((_truthy) => _truthy !== false && _truthy != null)(((rel === "..") || rel.startsWith($$bc$str("..", sep))))) {
    (() => { throw new Error($$bc$str("project AGENTS bootstrap cwd escapes policy root: ", cwd)); })();
  }
  return (() => { let segments = rel.split(sep).filter((segment) => (!(segment === ""))); let cursor = policy_root; let directories = [policy_root]; while (true) {
    if ($$bc$empty_p(segments)) { return directories; } else { const next = resolve(cursor, $$bc$first(segments)); const _recur_0 = $$bc$rest(segments); const _recur_1 = next; const _recur_2 = $$bc$conj_value(directories, next); segments = _recur_0; cursor = _recur_1; directories = _recur_2; continue; }
  } })();
}

function decode_project_instructions(path, source) {
  return (() => { try {
    return new TextDecoder("utf-8", {[$$bc$property_key($$bc$keyword("fatal"))]: true}).decode(source).trim();
  } catch (_catch_11) {
    switch ($$bd$catch_dispatch(_catch_11, [Error])) {
      case 0: {
        const cause = _catch_11;
        return (() => { throw error_with_cause($$bc$str("project AGENTS bootstrap is not valid UTF-8: ", path), cause); })();
        break;
      }
    }
  } })();
}

function projectAgentsAppendix(...$beagle$args) {
  if (arguments.length === 1) {
    const cwd = $beagle$args[0];
    return projectAgentsAppendix(cwd, process.env);
  }
  if (arguments.length === 2) {
    const cwd = $beagle$args[0];
    const env = $beagle$args[1];
    if ((!agent_laws_enabled_p(env))) {
      return "";
    } else {
      const project = git_root_for_project(cwd);
      const home = ((_logical) => (_logical !== false && _logical != null ? _logical : ""))(env.HOME).trim();
      const policy_root = ((home === "") ? project.root : (() => { try {
    const canonical_home = realpathSync(home);
  return (within_path_p(canonical_home, project.cwd) ? canonical_home : project.root);
  } catch (_catch_12) {
    switch ($$bd$catch_dispatch(_catch_12, [Error])) {
      case 0: {
        const __error = _catch_12;
        return project.root;
        break;
      }
    }
  } })());
      const directories = instruction_directories(policy_root, project.cwd);
      const global = canonicalGlobalAgents(env);
      return (() => { let remaining = directories; let seen = new Set(); let sections = []; while (true) {
    if ($$bc$empty_p(remaining)) { return ($$bc$empty_p(sections) ? "" : $$bc$str("\n\n## Project instructions — policy root to cwd\n\n", sections.join("\n\n"))); } else { const directory = $$bc$first(remaining); const path = project_instruction_file(directory); if ((path == null)) { const _recur_0 = $$bc$rest(remaining); const _recur_1 = seen; const _recur_2 = sections; remaining = _recur_0; seen = _recur_1; sections = _recur_2; continue; } else { const source_realpath = (() => { try {
    return realpathSync(path);
  } catch (_catch_13) {
    switch ($$bd$catch_dispatch(_catch_13, [Error])) {
      case 0: {
        const cause = _catch_13;
        return (() => { throw error_with_cause($$bc$str("project AGENTS bootstrap cannot resolve: ", path), cause); })();
        break;
      }
    }
  } })(); if (((_truthy) => _truthy !== false && _truthy != null)(((source_realpath === global.realpath) || seen.has(source_realpath)))) { const _recur_0 = $$bc$rest(remaining); const _recur_1 = seen; const _recur_2 = sections; remaining = _recur_0; seen = _recur_1; sections = _recur_2; continue; } else { const source = (() => { try {
    return readFileSync(path);
  } catch (_catch_14) {
    switch ($$bd$catch_dispatch(_catch_14, [Error])) {
      case 0: {
        const cause = _catch_14;
        return (() => { throw error_with_cause($$bc$str("project AGENTS bootstrap cannot read: ", path), cause); })();
        break;
      }
    }
  } })(); if (((_truthy) => _truthy !== false && _truthy != null)(((_logical) => (_logical !== false && _logical != null ? global.bytes.equals(source) : _logical))(global))) { const _recur_0 = $$bc$rest(remaining); const _recur_1 = seen; const _recur_2 = sections; remaining = _recur_0; seen = _recur_1; sections = _recur_2; continue; } else { const text = decode_project_instructions(path, source); if ((text === "")) { const _recur_0 = $$bc$rest(remaining); const _recur_1 = seen; const _recur_2 = sections; remaining = _recur_0; seen = _recur_1; sections = _recur_2; continue; } else { const section = $$bc$str("### ", path, "\n\n", text); const next_sections = $$bc$conj_value(sections, section); const appendix = $$bc$str("\n\n## Project instructions — policy root to cwd\n\n", next_sections.join("\n\n")); ((Buffer.byteLength(appendix, "utf8") > PROJECT__AGENTS__MAX__BYTES) ? (() => { return (() => { throw new Error($$bc$str("project AGENTS bootstrap exceeds ", PROJECT__AGENTS__MAX__BYTES, " bytes at: ", path)); })(); })() : null); seen.add(source_realpath); const _recur_0 = $$bc$rest(remaining); const _recur_1 = seen; const _recur_2 = next_sections; remaining = _recur_0; seen = _recur_1; sections = _recur_2; continue; } } } } }
  } })();
    }
  }
  throw new Error('No matching arity: ' + $beagle$args.length);
}

function assert_canonical_global_agents_exactly_once_bang(...$beagle$args) {
  if (arguments.length === 1) {
    const prompt = $beagle$args[0];
    return assert_canonical_global_agents_exactly_once_bang(prompt, process.env);
  }
  if (arguments.length === 2) {
    const prompt = $beagle$args[0];
    const env = $beagle$args[1];
    const canonical = canonicalGlobalAgents(env);
    if ((canonical == null)) {
      return null;
    } else {
      const needle = canonical.text.trim();
      return (() => { let count = 0; let offset = 0; while (true) {
    const relative_index = prompt.slice(offset).indexOf(needle); if ((relative_index === -1)) { return (() => { if ((!(count === 1))) {
  (() => { throw new Error($$bc$str("Anthropic global AGENTS bootstrap expected exactly once, observed ", count)); })();
}
return null; })(); } else { const _recur_0 = (count + 1); const _recur_1 = (offset + relative_index + needle.length); count = _recur_0; offset = _recur_1; continue; }
  } })();
    }
  }
  throw new Error('No matching arity: ' + $beagle$args.length);
}

function orchestration_home(...$beagle$args) {
  if (arguments.length === 0) {
    return orchestration_home(process.env);
  }
  if (arguments.length === 1) {
    const env = $beagle$args[0];
    return resolve(((_logical) => (_logical !== false && _logical != null ? _logical : resolve(((_logical) => (_logical !== false && _logical != null ? _logical : ""))(env.HOME), "code/agent-machinery/main")))(env.AGENT_MACHINERY_HOME));
  }
  throw new Error('No matching arity: ' + $beagle$args.length);
}

function orchestration_docs(...$beagle$args) {
  if (arguments.length === 0) {
    return orchestration_docs(process.env);
  }
  if (arguments.length === 1) {
    const env = $beagle$args[0];
    return resolve(orchestration_home(env), "docs");
  }
  throw new Error('No matching arity: ' + $beagle$args.length);
}

function heading_line_p(line, heading_lower) {
  return (line.trim().toLowerCase() === heading_lower);
}

function extract_fence_from_lines(lines, start, stop_at_heading) {
  return (() => { let index = start; let fence_open = -1; while (true) {
    if ((index >= lines.length)) { return null; } else { const trimmed = lines[index].trim(); if (((_truthy) => _truthy !== false && _truthy != null)((stop_at_heading && ((fence_open === -1) && trimmed.startsWith("## "))))) { return null; } else if (((_truthy) => _truthy !== false && _truthy != null)(((fence_open === -1) && trimmed.startsWith("```")))) { const _recur_0 = (index + 1); const _recur_1 = (index + 1); index = _recur_0; fence_open = _recur_1; continue; } else if (((_truthy) => _truthy !== false && _truthy != null)(((!(fence_open === -1)) && trimmed.startsWith("```")))) { return lines.slice(fence_open, index).join("\n"); } else { const _recur_0 = (index + 1); const _recur_1 = fence_open; index = _recur_0; fence_open = _recur_1; continue; } }
  } })();
}

function extract_fence_from_section(text, heading) {
  const lines = text.split("\n");
  const heading_lower = $$bc$str("## ", heading.toLowerCase());
  return (() => { let index = 0; while (true) {
    if ((index >= $$bc$count(lines))) { return null; } else { if (heading_line_p(lines[index], heading_lower)) { return extract_fence_from_lines(lines, (index + 1), true); } else { const _recur_0 = (index + 1); index = _recur_0; continue; } }
  } })();
}

function extract_first_fence(text) {
  return extract_fence_from_lines(text.split("\n"), 0, false);
}

function exact_section_fence(path, heading, label) {
  const source = (() => { try {
    return readFileSync(path, "utf8");
  } catch (_catch_15) {
    switch ($$bd$catch_dispatch(_catch_15, [Error])) {
      case 0: {
        const __error = _catch_15;
        return (() => { throw new Error($$bc$str("Orchestration contract unavailable: ", label, " (", path, ")")); })();
        break;
      }
    }
  } })();
  const block = extract_fence_from_section(source, heading);
  return (((_truthy) => _truthy !== false && _truthy != null)(((_logical) => (_logical !== false && _logical != null ? (!(block.trim() === "")) : _logical))(block)) ? block : (() => { throw new Error($$bc$str("Orchestration contract malformed: ", label, " has no fenced block (", path, ")")); })());
}

function exact_first_fence(path, label) {
  const source = (() => { try {
    return readFileSync(path, "utf8");
  } catch (_catch_16) {
    switch ($$bd$catch_dispatch(_catch_16, [Error])) {
      case 0: {
        const __error = _catch_16;
        return (() => { throw new Error($$bc$str("Orchestration contract unavailable: ", label, " (", path, ")")); })();
        break;
      }
    }
  } })();
  const block = extract_first_fence(source);
  return (((_truthy) => _truthy !== false && _truthy != null)(((_logical) => (_logical !== false && _logical != null ? (!(block.trim() === "")) : _logical))(block)) ? block : (() => { throw new Error($$bc$str("Orchestration contract malformed: ", label, " has no fenced block (", path, ")")); })());
}

function list_lines(values) {
  return values.map((value) => $$bc$str("- ", value)).join("\n");
}

function bespoke_role_block(metadata) {
  const composition = metadata.composition;
  if ((!(composition.kind === "bespoke"))) {
    (() => { throw new Error("bespoke role block requires bespoke composition"); })();
  }
  const contract = composition.contract;
  return [$$bc$str("ROLE: BESPOKE ", composition.id.toUpperCase(), "."), $$bc$str("Responsibility: ", contract.responsibility), $$bc$str("Deliverable: ", contract.deliverable), "May decide:", list_lines(contract.mayDecide), "Must escalate:", list_lines(contract.mustEscalate), "Done when:", list_lines(contract.doneWhen), $$bc$str("REPORT: ", contract.report), $$bc$str("Why bespoke: ", composition.bespokeReason), $$bc$str("Promotion candidate: ", (((_truthy) => _truthy !== false && _truthy != null)(composition.promotionCandidate) ? "yes" : "no"), ".")].join("\n");
}

function requirement_slug(requirement) {
  return requirement.toLowerCase().replace(new RegExp("[^a-z0-9]+", "g"), "-").replace(new RegExp("^-|-$", "g"), "");
}

const SKILL_NAME = new RegExp("^[a-z0-9]+(?:-[a-z0-9]+)*$", "u");

function domainSkillsDir(...$beagle$args) {
  if (arguments.length === 0) {
    return domainSkillsDir(process.env);
  }
  if (arguments.length === 1) {
    const env = $beagle$args[0];
    const override = ((_logical) => (_logical !== false && _logical != null ? _logical : ""))(env.NORTH_AGENT_SKILLS).trim();
    if ((!(override === ""))) {
      return resolve(override);
    } else {
      const state_root = ((_logical) => (_logical !== false && _logical != null ? _logical : resolve(((_logical) => (_logical !== false && _logical != null ? _logical : ""))(env.HOME), ".local", "state", "north", "agents")))(((_logical) => (_logical !== false && _logical != null ? _logical : ""))(env.NORTH_AGENT_STATE_ROOT).trim());
      return resolve(state_root, "current", "skills", "shared");
    }
  }
  throw new Error('No matching arity: ' + $beagle$args.length);
}

function lifecycle_project_id(git_root, env) {
  const parent_name = basename(dirname(git_root));
  const container = (((basename(git_root) === "main")) ? dirname(git_root) : (((_truthy) => _truthy !== false && _truthy != null)(["worktrees", "pins"].includes(parent_name))) ? dirname(dirname(git_root)) : null);
  if ((container == null)) {
    return null;
  } else {
    const home = ((_logical) => (_logical !== false && _logical != null ? _logical : ""))(env.HOME).trim();
    if ((home === "")) {
      return null;
    } else {
      const code_root = (() => { try {
    return resolve(realpathSync(home), "code");
  } catch (_catch_17) {
    switch ($$bd$catch_dispatch(_catch_17, [Error])) {
      case 0: {
        const __error = _catch_17;
        return null;
        break;
      }
    }
  } })();
      if ((code_root == null)) {
        return null;
      } else {
        const path = relative(code_root, container);
        if (((_truthy) => _truthy !== false && _truthy != null)(((path === "") || ((path === "..") || ((_logical) => (_logical !== false && _logical != null ? _logical : path.startsWith(sep)))(path.startsWith($$bc$str("..", sep))))))) {
          return null;
        } else {
          const segments = path.split(sep);
          const id = ((($$bc$count(segments) === 1)) ? $$bc$first(segments) : ((($$bc$count(segments) === 3) && ($$bc$first(segments) === "clients"))) ? segments[2] : null);
          return (((_truthy) => _truthy !== false && _truthy != null)(((_logical) => (_logical !== false && _logical != null ? SKILL_NAME.test(id) : _logical))(id)) ? id : null);
        }
      }
    }
  }
}

function projectSkillTarget(...$beagle$args) {
  if (arguments.length === 1) {
    const cwd = $beagle$args[0];
    return projectSkillTarget(cwd, process.env);
  }
  if (arguments.length === 2) {
    const cwd = $beagle$args[0];
    const env = $beagle$args[1];
    const project = git_root_for_project(cwd);
    const marker_present = (() => { try {
    const marker = statSync(resolve(project.root, ".git"));
  if (((!((_truthy) => _truthy !== false && _truthy != null)(marker.isDirectory())) && (!((_truthy) => _truthy !== false && _truthy != null)(marker.isFile())))) {
    (() => { throw new Error("project skill target has an invalid Git marker"); })();
  }
  return true;
  } catch (_catch_18) {
    switch ($$bd$catch_dispatch(_catch_18, [Error])) {
      case 0: {
        const error = _catch_18;
        return ((error.code === "ENOENT") ? false : (() => { throw error_with_cause($$bc$str("project skill target cannot inspect Git marker: ", project.root), error); })());
        break;
      }
    }
  } })();
    if ((!marker_present)) {
      return null;
    } else {
      const lifecycle_id = lifecycle_project_id(project.root, env);
      const explicit = ((_logical) => (_logical !== false && _logical != null ? _logical : ""))(env.NORTH_PROJECT).trim();
      if (((!(explicit === "")) && (!((_truthy) => _truthy !== false && _truthy != null)(SKILL_NAME.test(explicit))))) {
        (() => { throw new Error($$bc$str("project skill target has invalid NORTH_PROJECT: ", explicit)); })();
      }
      if (((_truthy) => _truthy !== false && _truthy != null)(((!(explicit === "")) && ((_logical) => (_logical !== false && _logical != null ? (!(explicit === lifecycle_id)) : _logical))(lifecycle_id)))) {
        (() => { throw new Error($$bc$str("project skill target contradicts lifecycle identity: ", explicit, " != ", lifecycle_id)); })();
      }
      const id = ((explicit === "") ? lifecycle_id : explicit);
      return (((_truthy) => _truthy !== false && _truthy != null)(id) ? Object.freeze({[$$bc$property_key($$bc$keyword("id"))]: id, [$$bc$property_key($$bc$keyword("gitRoot"))]: project.root}) : null);
    }
  }
  throw new Error('No matching arity: ' + $beagle$args.length);
}

function active_skill_roots(env, cwd) {
  const configured_root = domainSkillsDir(env);
  const root = (() => { try {
    const canonical = realpathSync(configured_root);
  if ((!((_truthy) => _truthy !== false && _truthy != null)(statSync(canonical).isDirectory()))) {
    (() => { throw new Error($$bc$str("active skill catalog is not a directory: ", canonical)); })();
  }
  return canonical;
  } catch (_catch_19) {
    switch ($$bd$catch_dispatch(_catch_19, [Error])) {
      case 0: {
        const cause = _catch_19;
        return ((cause.code === "ENOENT") ? null : (() => { throw error_with_cause($$bc$str("active skill catalog is unreadable: ", configured_root), cause); })());
        break;
      }
    }
  } })();
  if ((root == null)) {
    return Object.freeze({[$$bc$property_key($$bc$keyword("root"))]: configured_root, [$$bc$property_key($$bc$keyword("roots"))]: Object.freeze([])});
  } else {
    const roots = [root];
    if (((_truthy) => _truthy !== false && _truthy != null)(((_logical) => (_logical !== false && _logical != null ? ((((_logical) => (_logical !== false && _logical != null ? _logical : ""))(env.NORTH_AGENT_SKILLS).trim() === "") && ((basename(root) === "shared") && (basename(dirname(root)) === "skills"))) : _logical))(cwd))) {
      const target = projectSkillTarget(cwd, env);
      if (((_truthy) => _truthy !== false && _truthy != null)(target)) {
        const generation = dirname(dirname(root));
        const candidate = resolve(generation, "projects", target.id, "skill");
        (() => { try {
    const canonical = realpathSync(candidate);
  const from_generation = relative(generation, canonical);
  if (((_truthy) => _truthy !== false && _truthy != null)(((from_generation === "..") || ((_logical) => (_logical !== false && _logical != null ? _logical : ((_logical) => (_logical !== false && _logical != null ? _logical : (!((_truthy) => _truthy !== false && _truthy != null)(statSync(canonical).isDirectory()))))(from_generation.startsWith(sep))))(from_generation.startsWith($$bc$str("..", sep)))))) {
    (() => { throw new Error($$bc$str("project skill package escapes its generation: ", candidate)); })();
  }
  return roots.push(canonical);
  } catch (_catch_20) {
    switch ($$bd$catch_dispatch(_catch_20, [Error])) {
      case 0: {
        const cause = _catch_20;
        if ((!(cause.code === "ENOENT"))) {
          return (() => { throw error_with_cause($$bc$str("project skill package is unreadable: ", candidate), cause); })();
        }
        break;
      }
    }
  } })();
      }
    }
    return Object.freeze({[$$bc$property_key($$bc$keyword("root"))]: root, [$$bc$property_key($$bc$keyword("roots"))]: Object.freeze(roots)});
  }
}

function skill_trigger_metadata(path, folder) {
  const bytes = (() => { try {
    const info = statSync(path);
  if ((!((_truthy) => _truthy !== false && _truthy != null)(info.isFile()))) {
    (() => { throw new Error($$bc$str("active skill source is not a regular file: ", path)); })();
  }
  return readFileSync(path);
  } catch (_catch_21) {
    switch ($$bd$catch_dispatch(_catch_21, [Error])) {
      case 0: {
        const cause = _catch_21;
        return (() => { throw error_with_cause($$bc$str("active skill source is stale or unreadable: ", path), cause); })();
        break;
      }
    }
  } })();
  const text = (() => { try {
    return new TextDecoder("utf-8", {[$$bc$property_key($$bc$keyword("fatal"))]: true}).decode(bytes);
  } catch (_catch_22) {
    switch ($$bd$catch_dispatch(_catch_22, [Error])) {
      case 0: {
        const cause = _catch_22;
        return (() => { throw error_with_cause($$bc$str("active skill source is not valid UTF-8: ", path), cause); })();
        break;
      }
    }
  } })();
  const match = text.match(new RegExp("^---\\r?\\n([\\s\\S]*?)\\r?\\n---(?:\\r?\\n|$)", "u"));
  const frontmatter = (((_truthy) => _truthy !== false && _truthy != null)(match) ? (($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1))(match, 1) : null);
  if ((frontmatter == null)) {
    (() => { throw new Error($$bc$str("active skill frontmatter is malformed: ", path)); })();
  }
  const parsed = (() => { try {
    return Bun.YAML.parse(frontmatter);
  } catch (_catch_23) {
    switch ($$bd$catch_dispatch(_catch_23, [Error])) {
      case 0: {
        const cause = _catch_23;
        return (() => { throw error_with_cause($$bc$str("active skill frontmatter is invalid YAML: ", path), cause); })();
        break;
      }
    }
  } })();
  const name = parsed.name;
  const raw_description = parsed.description;
  if (((!(typeof name === "string")) || ((!((_truthy) => _truthy !== false && _truthy != null)(SKILL_NAME.test(name))) || (!(name === folder))))) {
    (() => { throw new Error($$bc$str("active skill name must equal its folder ", folder, ": ", path)); })();
  }
  if (((!(typeof raw_description === "string")) || (raw_description.trim() === ""))) {
    (() => { throw new Error($$bc$str("active skill description is missing: ", path)); })();
  }
  return Object.freeze({[$$bc$property_key($$bc$keyword("name"))]: name, [$$bc$property_key($$bc$keyword("description"))]: raw_description.replace(new RegExp("\\s+", "g"), " ").trim(), [$$bc$property_key($$bc$keyword("path"))]: path});
}

function skill_catalog_appendix(roots, candidates) {
  const rows = candidates.map((candidate) => $$bc$str("- ", JSON.stringify(candidate)));
  return ($$bc$empty_p(rows) ? "" : ["", "", $$bc$str("## Active skill candidates — ", roots.join(", ")), "Trigger metadata only. Match the request against every description and load each", "matching SKILL.md under the global skill-loading law; no skill body is injected here."].concat(rows).join("\n"));
}

function activeSkillCatalog(...$beagle$args) {
  if (arguments.length === 0) {
    return activeSkillCatalog(process.env, null);
  }
  if (arguments.length === 1) {
    const env = $beagle$args[0];
    return activeSkillCatalog(env, null);
  }
  if (arguments.length === 2) {
    const env = $beagle$args[0];
    const cwd = $beagle$args[1];
    const root_state = active_skill_roots(env, cwd);
    const root = root_state.root;
    const roots = root_state.roots;
    if ($$bc$empty_p(roots)) {
      return Object.freeze({[$$bc$property_key($$bc$keyword("root"))]: root, [$$bc$property_key($$bc$keyword("roots"))]: roots, [$$bc$property_key($$bc$keyword("candidates"))]: Object.freeze([]), [$$bc$property_key($$bc$keyword("appendix"))]: ""});
    } else {
      const by_name = new Map();
      roots.forEach((skill_root) => {
  const entries = readdirSync(skill_root, $$bh$host_object($$bc$keyword("withFileTypes"), true)).sort((left, right) => (((left.name < right.name)) ? -1 : ((left.name > right.name)) ? 1 : 0));
  entries.forEach((entry) => {
  const directory = resolve(skill_root, entry.name);
  (() => { try {
    if ((!((_truthy) => _truthy !== false && _truthy != null)(statSync(directory).isDirectory()))) {
    return (() => { throw new Error($$bc$str("active skill entry is not a directory: ", directory)); })();
  }
  } catch (_catch_24) {
    switch ($$bd$catch_dispatch(_catch_24, [Error])) {
      case 0: {
        const cause = _catch_24;
        return (() => { throw error_with_cause($$bc$str("active skill entry is stale or unreadable: ", directory), cause); })();
        break;
      }
    }
  } })();
  const candidate = skill_trigger_metadata(resolve(directory, "SKILL.md"), entry.name);
  const previous = by_name.get(candidate.name);
  if (((_truthy) => _truthy !== false && _truthy != null)(previous)) {
    if ((!((_truthy) => _truthy !== false && _truthy != null)(readFileSync(previous.path).equals(readFileSync(candidate.path))))) {
      (() => { throw new Error($$bc$str("active skill UnitId collision ", candidate.name, ": ", previous.path, " != ", candidate.path)); })();
    }
  } else {
    by_name.set(candidate.name, candidate);
  }
});
  const candidates = Array.from(by_name.values()).sort((left, right) => left.name.localeCompare(right.name));
  Object.freeze({[$$bc$property_key($$bc$keyword("root"))]: root, [$$bc$property_key($$bc$keyword("roots"))]: Object.freeze(roots), [$$bc$property_key($$bc$keyword("candidates"))]: Object.freeze(candidates), [$$bc$property_key($$bc$keyword("appendix"))]: skill_catalog_appendix(roots, candidates)});
});
    }
  }
  throw new Error('No matching arity: ' + $beagle$args.length);
}

function domain_context_candidates(cwd, requirement, env) {
  const slug = requirement_slug(requirement);
  const skill_candidates = active_skill_roots(env, cwd).roots.map((root) => resolve(root, slug, "SKILL.md"));
  const candidates = [resolve(cwd, "AGENTS.md"), resolve(cwd, "docs", $$bc$str(slug, ".md")), resolve(cwd, "docs", "domains", $$bc$str(slug, ".md"))].concat(skill_candidates, [resolve(orchestration_home(env), "docs", "domains", $$bc$str(slug, ".md"))]);
  return Array.from(new Set(candidates.filter((candidate) => existsSync(candidate))));
}

function domain_context_gate(requirements, cwd, env) {
  if ($$bc$empty_p(requirements)) {
    return "";
  } else {
    const entries = requirements.map((requirement) => { const candidates = domain_context_candidates(cwd, requirement, env);
return [$$bc$str("### ", requirement), ($$bc$empty_p(candidates) ? "No context candidate was discovered by the harness." : $$bc$str("Candidate entry points (candidates are not proof of expertise):\n", list_lines(candidates)))].join("\n"); });
    return ["## Orchestration domain-context gate", "Before any side effect, satisfy every domain requirement by reading the relevant", "repo-local authoritative docs, triggered skills, or provider capability contract.", "For each requirement, name the exact artifact actually read and apply it. A candidate", "path is only an entry point, never evidence that you possess the expertise. If no", "authoritative context exists or access is missing, report `DOMAIN CONTEXT MISSING:", "<requirement>` to the orchestrator and stop before side effects; never fake expertise."].concat(entries).join("\n");
  }
}

const PROMPT__COMPOSITION__VERSION = "north-harness-prompt:v3";

const COMPACTION__POLICY__VERSION = "north-native-auto-compact:v1";

function capability_class(capabilities, topology) {
  return (((capabilities == null)) ? "unknown" : (((_truthy) => _truthy !== false && _truthy != null)(((topology === "orchestrator") && capabilities.includes("coordination")))) ? "orchestrator" : (hasAuthoringCapability(capabilities)) ? "authoring" : (((_truthy) => _truthy !== false && _truthy != null)(capabilities.includes("web"))) ? "readonly-web" : "readonly");
}

function model_delta_appendix(provider, model, omit_reason) {
  return ((((_truthy) => _truthy !== false && _truthy != null)(omit_reason)) ? $$bh$host_object($$bc$keyword("appendix"), "", $$bc$keyword("evidence"), {[$$bc$property_key($$bc$keyword("provider"))]: provider, [$$bc$property_key($$bc$keyword("model"))]: model, [$$bc$property_key($$bc$keyword("kind"))]: "omitted", [$$bc$property_key($$bc$keyword("reason"))]: omit_reason}) : (((provider == null) || (model == null))) ? $$bh$host_object($$bc$keyword("appendix"), "", $$bc$keyword("evidence"), {[$$bc$property_key($$bc$keyword("provider"))]: provider, [$$bc$property_key($$bc$keyword("model"))]: model, [$$bc$property_key($$bc$keyword("kind"))]: "omitted", [$$bc$property_key($$bc$keyword("reason"))]: (((_truthy) => _truthy !== false && _truthy != null)((provider == null)) ? "provider_unresolved" : "model_unresolved")}) : (() => { const delta = resolveModelDelta(provider, model); if ((delta.kind === "none")) {
  return $$bh$host_object($$bc$keyword("appendix"), "", $$bc$keyword("evidence"), {[$$bc$property_key($$bc$keyword("provider"))]: provider, [$$bc$property_key($$bc$keyword("model"))]: model, [$$bc$property_key($$bc$keyword("kind"))]: "none", [$$bc$property_key($$bc$keyword("reason"))]: delta.reason});
} else {
  const block = exact_first_fence(delta.absolutePath, $$bc$str("model-delta:", provider, ":", model));
  return $$bh$host_object($$bc$keyword("appendix"), $$bc$str("\n\n## Orchestration exact-model delta — ", provider, ":", model, "\n", block), $$bc$keyword("evidence"), {[$$bc$property_key($$bc$keyword("provider"))]: provider, [$$bc$property_key($$bc$keyword("model"))]: model, [$$bc$property_key($$bc$keyword("kind"))]: "calibrated", [$$bc$property_key($$bc$keyword("path"))]: delta.path});
} })());
}

function receipt_module(chunks, state, position) {
  const chunk = chunks[position];
  const id = chunk[0];
  const rendered = chunk[1];
  return $$bh$host_object($$bc$keyword("id"), id, $$bc$keyword("schemaVersion"), "v1", $$bc$keyword("position"), position, $$bc$keyword("dependencies"), (((_truthy) => _truthy !== false && _truthy != null)((position === 0)) ? [] : [chunks[(position - 1)][0]]), $$bc$keyword("sourceSha256"), sha256Bytes(rendered), $$bc$keyword("rendered"), rendered, $$bc$keyword("parameterDigests"), (((_truthy) => _truthy !== false && _truthy != null)((id === "core-base")) ? {[$$bc$property_key($$bc$keyword("esoMode"))]: sha256Bytes(((_logical) => (_logical !== false && _logical != null ? _logical : "on"))(state.environment.AGENT_ESO)), [$$bc$property_key($$bc$keyword("lawsMode"))]: sha256Bytes(((_logical) => (_logical !== false && _logical != null ? _logical : "on"))(state.environment.AGENT_LAWS))} : null));
}

function prompt_receipt(state, provider, model, prompt, chunks, delta, include_bootstrap) {
  return buildPromptReceipt($$bh$host_object($$bc$keyword("coverage"), "exact", $$bc$keyword("wirePrompt"), prompt, $$bc$keyword("modules"), chunks.map((__chunk, position) => receipt_module(chunks, state, position)), $$bc$keyword("branches"), [{[$$bc$property_key($$bc$keyword("ruleId"))]: "global-bootstrap-provider", [$$bc$property_key($$bc$keyword("conditionId"))]: "provider-kind", [$$bc$property_key($$bc$keyword("inputDigest"))]: sha256Bytes(((_logical) => (_logical !== false && _logical != null ? _logical : "unresolved"))(provider)), [$$bc$property_key($$bc$keyword("branch"))]: (((_truthy) => _truthy !== false && _truthy != null)(include_bootstrap) ? "included" : "native-provider")}, {[$$bc$property_key($$bc$keyword("ruleId"))]: "model-delta", [$$bc$property_key($$bc$keyword("conditionId"))]: "resolved-model", [$$bc$property_key($$bc$keyword("inputDigest"))]: sha256Bytes($$bc$str(((_logical) => (_logical !== false && _logical != null ? _logical : "unresolved"))(provider), ":", ((_logical) => (_logical !== false && _logical != null ? _logical : "unresolved"))(model))), [$$bc$property_key($$bc$keyword("branch"))]: delta.evidence.kind}, {[$$bc$property_key($$bc$keyword("ruleId"))]: "capability-class", [$$bc$property_key($$bc$keyword("conditionId"))]: "capability-set", [$$bc$property_key($$bc$keyword("inputDigest"))]: sha256Bytes(JSON.stringify(Array.from(((_logical) => (_logical !== false && _logical != null ? _logical : []))(state.capabilities)).sort())), [$$bc$property_key($$bc$keyword("branch"))]: capability_class(state.capabilities, state.evidence.topology)}]));
}

function context_window_observation(provider, model) {
  return (() => { try {
    return (((_truthy) => _truthy !== false && _truthy != null)(((_logical) => (_logical !== false && _logical != null ? model : _logical))(provider)) ? observeProviderContextWindow(provider, model) : null);
  } catch (_catch_25) {
    switch ($$bd$catch_dispatch(_catch_25, [Error])) {
      case 0: {
        const __error = _catch_25;
        return null;
        break;
      }
    }
  } })();
}

function prompt_economics(state, provider, model, stable_prefix, tail, prompt) {
  const context_window = context_window_observation(provider, model);
  return $$bh$host_object($$bc$keyword("compositionVersion"), PROMPT__COMPOSITION__VERSION, $$bc$keyword("compositionDigest"), createHash("sha256").update(prompt).digest("hex"), $$bc$keyword("capabilityClass"), capability_class(state.capabilities, state.evidence.topology), $$bc$keyword("capabilityCount"), ((_logical) => (_logical !== false && _logical != null ? _logical : []))(state.capabilities).length, $$bc$keyword("stablePrefixBytes"), Buffer.byteLength(stable_prefix, "utf8"), $$bc$keyword("uniqueTailBytes"), Buffer.byteLength(tail, "utf8"), $$bc$keyword("totalBytes"), Buffer.byteLength(prompt, "utf8"), $$bc$keyword("byteMeasurementSource"), "node-buffer-byte-length:utf8", $$bc$keyword("tokenMeasurementStatus"), "unknown", $$bc$keyword("tokenMeasurementSource"), "authoritative-tokenizer-unavailable", $$bc$keyword("providerContextWindowTokens"), (((_truthy) => _truthy !== false && _truthy != null)(context_window) ? context_window.tokens : null), $$bc$keyword("contextWindowEffectiveFrom"), (((_truthy) => _truthy !== false && _truthy != null)(context_window) ? context_window.effectiveFrom : null), $$bc$keyword("contextWindowStatus"), (((_truthy) => _truthy !== false && _truthy != null)(context_window) ? "observed" : "unknown"), $$bc$keyword("contextWindowSource"), ((((_truthy) => _truthy !== false && _truthy != null)(context_window)) ? "orchestration-provider-catalog" : (((_truthy) => _truthy !== false && _truthy != null)(((_logical) => (_logical !== false && _logical != null ? model : _logical))(provider))) ? "catalog-metadata-unavailable" : "provider-or-model-unresolved"), $$bc$keyword("contextBudgetStatus"), "unknown", $$bc$keyword("contextBudgetSource"), "north-harness-unconfigured", $$bc$keyword("compactionPolicy"), "native-auto-compact-enabled", $$bc$keyword("compactionPolicyVersion"), COMPACTION__POLICY__VERSION);
}

function compose_system_prompt(state, provider, model) {
  const include_bootstrap = ((provider == null) || (provider === "anthropic"));
  const bootstrap = (include_bootstrap ? global_laws_appendix(state.environment) : "");
  const skill_catalog = (((_truthy) => _truthy !== false && _truthy != null)(state.dataOnly) ? "" : state.skillCatalog.appendix);
  const delta = model_delta_appendix(provider, model, (((_truthy) => _truthy !== false && _truthy != null)(state.dataOnly) ? "data-only contract excludes model prompt deltas" : state.omitModelDeltaReason));
  const core = $$bc$str(state.basePrompt, bootstrap, skill_catalog);
  const capability = state.orchestrationAppendix;
  const project = projectAgentsAppendix(state.cwd, state.environment);
  const coordination = (((_truthy) => _truthy !== false && _truthy != null)(state.dataOnly) ? "" : coordination_block(state.self, state.cwd, provider, ((_logical) => (_logical !== false && _logical != null ? _logical : []))(state.capabilities)));
  const tail = $$bc$str(coordination, delta.appendix);
  const stable_prefix = $$bc$str(core, capability, project);
  const prompt = $$bc$str(stable_prefix, tail);
  const chunks = [["core-base", state.basePrompt], ["global-bootstrap", bootstrap], ["active-skill-catalog", skill_catalog], ["orchestration", state.orchestrationAppendix], ["project-instructions", project], ["coordination", coordination], ["model-delta", delta.appendix]];
  return $$bh$host_object($$bc$keyword("prompt"), prompt, $$bc$keyword("deltaEvidence"), delta.evidence, $$bc$keyword("economics"), prompt_economics(state, provider, model, stable_prefix, tail, prompt), $$bc$keyword("receipt"), prompt_receipt(state, provider, model, prompt, chunks, delta, include_bootstrap));
}

function set_evidence_bang(evidence, key, value) {
  (($beagle$host$arg$0, $beagle$host$arg$1, $beagle$host$arg$2) => $$bh$aset($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1, $beagle$host$arg$2))(evidence, key, value);
  return null;
}

function hash_text(text) {
  return createHash("sha256").update(text).digest("hex");
}

function orchestration_appendix_bang(...$beagle$args) {
  if (arguments.length === 1) {
    const metadata = $beagle$args[0];
    return orchestration_appendix_bang(metadata, process.cwd(), process.env);
  }
  if (arguments.length === 2) {
    const metadata = $beagle$args[0];
    const cwd = $beagle$args[1];
    return orchestration_appendix_bang(metadata, cwd, process.env);
  }
  if (arguments.length === 3) {
    const metadata = $beagle$args[0];
    const cwd = $beagle$args[1];
    const env = $beagle$args[2];
    if (((metadata == null) || (Object.keys(metadata).length === 0))) {
      return $$bh$host_object($$bc$keyword("appendix"), "", $$bc$keyword("evidence"), {});
    } else {
      const admitted = (((_truthy) => _truthy !== false && _truthy != null)(((_logical) => (_logical !== false && _logical != null ? _logical : metadata.composition))(metadata.role)) ? admitRoutingRequest(metadata, "Orchestration appendix") : null);
      const routing = ((_logical) => (_logical !== false && _logical != null ? _logical : metadata))(admitted);
      const blocks = [];
      const evidence = $$bh$js_obj();
      if (((_truthy) => _truthy !== false && _truthy != null)(admitted)) {
        const composition = admitted.composition;
        if ((composition.kind === "template")) {
          const role = exact_section_fence(resolve(orchestration_docs(env), "roles.md"), composition.id, $$bc$str("role:", composition.id));
          blocks.push($$bc$str("## Orchestration role contract — template:", composition.id, "\n", role));
          if ((composition.overrides.length > 0)) {
            blocks.push(["## Orchestration template override", $$bc$str("Axes changed: ", composition.overrides.join(", "), "."), $$bc$str("Reason: ", composition.overrideReason)].join("\n"));
            set_evidence_bang(evidence, "templateOverrides", Array.from(composition.overrides));
            set_evidence_bang(evidence, "templateOverrideReasonHash", hash_text(composition.overrideReason));
          }
        } else {
          blocks.push($$bc$str("## Orchestration role contract — bespoke:", composition.id, "\n", bespoke_role_block(admitted)));
          set_evidence_bang(evidence, "bespokeContractHash", bespokeContractFingerprint(composition.contract));
          set_evidence_bang(evidence, "bespokeContractFingerprintVersion", BESPOKE__FINGERPRINT__VERSION);
          set_evidence_bang(evidence, "bespokeContractFingerprintDomain", BESPOKE__FINGERPRINT__DOMAIN);
        }
        set_evidence_bang(evidence, "roleKind", composition.kind);
        set_evidence_bang(evidence, "roleId", composition.id);
        set_evidence_bang(evidence, "capabilities", ((composition.kind === "bespoke") ? canonicalOrchestrationCapabilities(composition.contract.capabilities) : orchestrationCapabilities(admitted)));
        const comms = exact_section_fence(resolve(orchestration_docs(env), "comms.md"), "universal", "comms:universal");
        blocks.push($$bc$str("## Orchestration communication contract — universal\n", comms));
        set_evidence_bang(evidence, "commsContractHash", hash_text(comms));
      }
      if (((_truthy) => _truthy !== false && _truthy != null)(routing.taskGrade)) {
        const block = exact_section_fence(resolve(orchestration_docs(env), "task-grades.md"), routing.taskGrade, $$bc$str("task-grade:", routing.taskGrade));
        blocks.push($$bc$str("## Orchestration task grade — ", routing.taskGrade, "\n", block));
        set_evidence_bang(evidence, "taskGrade", routing.taskGrade);
      }
      if ((((_logical) => (_logical !== false && _logical != null ? _logical : []))(routing.domainRequirements).length > 0)) {
        blocks.push(domain_context_gate(routing.domainRequirements, cwd, env));
        set_evidence_bang(evidence, "domainRequirements", Array.from(routing.domainRequirements));
      }
      if (((_truthy) => _truthy !== false && _truthy != null)(routing.topology)) {
        const block = exact_section_fence(resolve(orchestration_docs(env), "topologies.md"), routing.topology, $$bc$str("topology:", routing.topology));
        blocks.push($$bc$str("## Orchestration topology — ", routing.topology, "\n", block));
        set_evidence_bang(evidence, "topology", routing.topology);
      }
      if (((_truthy) => _truthy !== false && _truthy != null)(((_logical) => (_logical !== false && _logical != null ? _logical : ((_logical) => (_logical !== false && _logical != null ? _logical : routing.reasoning))(routing.serviceClass)))(routing.capabilityFloor))) {
        blocks.push(["## Orchestration capacity route", $$bc$str("Capability floor: ", ((_logical) => (_logical !== false && _logical != null ? _logical : "unselected"))(routing.capabilityFloor), "."), $$bc$str("Service class: ", ((_logical) => (_logical !== false && _logical != null ? _logical : "unselected"))(routing.serviceClass), "."), $$bc$str("Reasoning: ", ((_logical) => (_logical !== false && _logical != null ? _logical : "unselected"))(routing.reasoning), "."), $$bc$str("Capacity does not widen the role, grade, topology, or ", "domain authority above.")].join("\n"));
        if (((_truthy) => _truthy !== false && _truthy != null)(routing.capabilityFloor)) {
          set_evidence_bang(evidence, "capabilityFloor", routing.capabilityFloor);
        }
        if (((_truthy) => _truthy !== false && _truthy != null)(routing.serviceClass)) {
          set_evidence_bang(evidence, "serviceClass", routing.serviceClass);
        }
        if (((_truthy) => _truthy !== false && _truthy != null)(routing.reasoning)) {
          set_evidence_bang(evidence, "reasoning", routing.reasoning);
        }
      }
      if (((_truthy) => _truthy !== false && _truthy != null)(routing.posture)) {
        const block = exact_section_fence(resolve(orchestration_docs(env), "postures.md"), routing.posture, $$bc$str("posture:", routing.posture));
        blocks.push($$bc$str("## Orchestration posture — ", routing.posture, "\n", block));
        set_evidence_bang(evidence, "posture", routing.posture);
      }
      return $$bh$host_object($$bc$keyword("appendix"), (((_truthy) => _truthy !== false && _truthy != null)($$bc$empty_p(blocks)) ? "" : $$bc$str("\n\n", blocks.join("\n\n"))), $$bc$keyword("evidence"), evidence);
    }
  }
  throw new Error('No matching arity: ' + $beagle$args.length);
}

const orchestrationAppendix = orchestration_appendix_bang;

const harness_composition = new WeakMap();

const applied_evidence = new WeakMap();

const harness_activity_renewers = new WeakMap();

const harness_authority_seals = new WeakMap();

const authoring_hook_seals = new WeakMap();

function deep_freeze(value) {
  if (((_truthy) => _truthy !== false && _truthy != null)(((_logical) => (_logical !== false && _logical != null ? ((typeof value === "object") && (!Object.isFrozen(value))) : _logical))(value))) {
    Object.values(value).forEach((child) => {
  deep_freeze(child);
});
    Object.freeze(value);
  }
  return value;
}

function option_snapshot(raw, keys) {
  return Object.freeze(keys.map((key) => deep_freeze((($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1))(raw, key))));
}

function seal_harness_authority_bang(options, provider) {
  const raw = options;
  if (((raw.northRoutingRequest == null) || (raw.northCapabilities == null))) {
    return null;
  } else {
    const evidence = applied_evidence.get(options);
    const north_server = raw.mcpServers.north;
    if (((evidence == null) || ((!(typeof raw.systemPrompt === "string")) || ((raw.env == null) || (((north_server == null) && (!(raw.northDataOnly === true))) || (!(typeof raw.cwd === "string"))))))) {
      return null;
    } else {
      const option_keys = Object.keys(raw).sort();
      harness_authority_seals.set(options, {[$$bc$property_key($$bc$keyword("provider"))]: provider, [$$bc$property_key($$bc$keyword("optionKeys"))]: Object.freeze(option_keys), [$$bc$property_key($$bc$keyword("optionValues"))]: option_snapshot(raw, option_keys), [$$bc$property_key($$bc$keyword("systemPrompt"))]: raw.systemPrompt, [$$bc$property_key($$bc$keyword("routingRequest"))]: raw.northRoutingRequest, [$$bc$property_key($$bc$keyword("capabilities"))]: raw.northCapabilities, [$$bc$property_key($$bc$keyword("evidence"))]: evidence, [$$bc$property_key($$bc$keyword("env"))]: raw.env, [$$bc$property_key($$bc$keyword("mcpServers"))]: raw.mcpServers, [$$bc$property_key($$bc$keyword("mcpServerEntries"))]: Object.entries(raw.mcpServers), [$$bc$property_key($$bc$keyword("northServer"))]: ((_logical) => (_logical !== false && _logical != null ? _logical : raw.mcpServers))(north_server), [$$bc$property_key($$bc$keyword("tools"))]: raw.tools, [$$bc$property_key($$bc$keyword("allowedTools"))]: raw.allowedTools, [$$bc$property_key($$bc$keyword("disallowedTools"))]: raw.disallowedTools, [$$bc$property_key($$bc$keyword("settingSources"))]: raw.settingSources, [$$bc$property_key($$bc$keyword("strictMcpConfig"))]: raw.strictMcpConfig, [$$bc$property_key($$bc$keyword("permissionMode"))]: raw.permissionMode, [$$bc$property_key($$bc$keyword("agentId"))]: raw.env.AGENT_ID, [$$bc$property_key($$bc$keyword("managedLane"))]: raw.env.NORTH_MANAGED_LANE, [$$bc$property_key($$bc$keyword("topology"))]: raw.env.AGENT_TOPOLOGY, [$$bc$property_key($$bc$keyword("cwd"))]: raw.cwd, [$$bc$property_key($$bc$keyword("effort"))]: raw.effort, [$$bc$property_key($$bc$keyword("model"))]: raw.model, [$$bc$property_key($$bc$keyword("maxTurns"))]: raw.maxTurns, [$$bc$property_key($$bc$keyword("modelAvailability"))]: raw.northModelAvailability, [$$bc$property_key($$bc$keyword("dataOnly"))]: (raw.northDataOnly === true)});
      return null;
    }
  }
}

function exact_object_entries_p(actual, expected) {
  return ((actual.length === expected.length) && actual.every((entry, index) => { const expected_entry = (($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_array($beagle$host$arg$0), $beagle$host$arg$1))(expected, index);
return (((($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_array($beagle$host$arg$0), $beagle$host$arg$1))(entry, 0) === (($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_array($beagle$host$arg$0), $beagle$host$arg$1))(expected_entry, 0)) && ((($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_array($beagle$host$arg$0), $beagle$host$arg$1))(entry, 1) === (($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_array($beagle$host$arg$0), $beagle$host$arg$1))(expected_entry, 1))); }));
}

function hasCanonicalHarnessAuthority(options, provider) {
  const raw = options;
  const seal = harness_authority_seals.get(options);
  const server_entries = Object.entries(((_logical) => (_logical !== false && _logical != null ? _logical : {}))(raw.mcpServers));
  const option_keys = Object.keys(raw).sort();
  return Boolean(((_logical) => (_logical !== false && _logical != null ? ((seal.provider === provider) && ((option_keys.length === seal.optionKeys.length) && ((_logical) => (_logical !== false && _logical != null ? ((raw.systemPrompt === seal.systemPrompt) && ((raw.northRoutingRequest === seal.routingRequest) && ((raw.northCapabilities === seal.capabilities) && ((applied_evidence.get(options) === seal.evidence) && ((raw.env === seal.env) && ((raw.mcpServers === seal.mcpServers) && (exact_object_entries_p(server_entries, seal.mcpServerEntries) && (((raw.northDataOnly === true) ? (raw.mcpServers === seal.northServer) : (raw.mcpServers.north === seal.northServer)) && ((raw.tools === seal.tools) && ((raw.allowedTools === seal.allowedTools) && ((raw.disallowedTools === seal.disallowedTools) && ((raw.settingSources === seal.settingSources) && ((raw.strictMcpConfig === seal.strictMcpConfig) && ((raw.permissionMode === seal.permissionMode) && ((raw.env.AGENT_ID === seal.agentId) && ((raw.env.NORTH_MANAGED_LANE === seal.managedLane) && ((raw.env.AGENT_TOPOLOGY === seal.topology) && ((raw.cwd === seal.cwd) && ((raw.effort === seal.effort) && ((raw.model === seal.model) && ((raw.maxTurns === seal.maxTurns) && ((raw.northModelAvailability === seal.modelAvailability) && ((raw.northDataOnly === true) === seal.dataOnly))))))))))))))))))))))) : _logical))(option_keys.every((key, index) => ((key === seal.optionKeys[index]) && Object.is((($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1))(raw, key), seal.optionValues[index])))))) : _logical))(seal));
}

function canonicalHarnessModelAvailability(options, provider) {
  return ((!hasCanonicalHarnessAuthority(options, provider)) ? null : harness_authority_seals.get(options).modelAvailability);
}

function hook_snapshot(values) {
  return values.map((entry) => $$bh$host_object($$bc$keyword("matcher"), entry.matcher, $$bc$keyword("hooks"), (((_truthy) => _truthy !== false && _truthy != null)(Array.isArray(entry.hooks)) ? Array.from(entry.hooks) : [])));
}

function seal_authoring_hooks_bang(options) {
  const entries = options.hooks.PreToolUse;
  const post_entries = options.hooks.PostToolUse;
  if (((!Array.isArray(entries)) || (!Array.isArray(post_entries)))) {
    return null;
  } else {
    authoring_hook_seals.set(options, {[$$bc$property_key($$bc$keyword("topology"))]: options.env.AGENT_TOPOLOGY, [$$bc$property_key($$bc$keyword("entries"))]: hook_snapshot(entries), [$$bc$property_key($$bc$keyword("postEntries"))]: hook_snapshot(post_entries), [$$bc$property_key($$bc$keyword("mcpServers"))]: Object.entries(((_logical) => (_logical !== false && _logical != null ? _logical : {}))(options.mcpServers))});
    return null;
  }
}

function inherit_authoring_hook_seal_bang(source, target) {
  const seal = authoring_hook_seals.get(source);
  if (((_truthy) => _truthy !== false && _truthy != null)(seal)) {
    authoring_hook_seals.set(target, seal);
  }
  return null;
}

function exact_hook_entry_p(actual, expected) {
  const expected_keys = ((expected.matcher == null) ? ["hooks"] : ["hooks", "matcher"]);
  return ((_logical) => (_logical !== false && _logical != null ? ((typeof actual === "object") && ((!Array.isArray(actual)) && ((Object.keys(actual).sort().join(",") === expected_keys.join(",")) && ((actual.matcher === expected.matcher) && (Array.isArray(actual.hooks) && ((actual.hooks.length === expected.hooks.length) && actual.hooks.every((hook, index) => (hook === expected.hooks[index])))))))) : _logical))(actual);
}

function exact_hook_entries_p(actual, expected) {
  return expected.every((entry, index) => exact_hook_entry_p((($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_array($beagle$host$arg$0), $beagle$host$arg$1))(actual, index), entry));
}

function hasCanonicalAuthoringHooks(options) {
  const seal = authoring_hook_seals.get(options);
  const hook_surface = options.hooks;
  const entries = hook_surface.PreToolUse;
  const post_entries = hook_surface.PostToolUse;
  const servers = Object.entries(((_logical) => (_logical !== false && _logical != null ? _logical : {}))(options.mcpServers));
  return (((seal == null) || ((hook_surface == null) || ((!(Object.keys(hook_surface).sort().join(",") === "PostToolUse,PreToolUse")) || ((!(options.env.AGENT_TOPOLOGY === seal.topology)) || ((!Array.isArray(entries)) || ((!Array.isArray(post_entries)) || ((!(entries.length === seal.entries.length)) || ((!(post_entries.length === seal.postEntries.length)) || (!exact_object_entries_p(servers, seal.mcpServers)))))))))) ? false : (exact_hook_entries_p(entries, seal.entries) && exact_hook_entries_p(post_entries, seal.postEntries)));
}

function canonical_route_model(state, provider, concrete_model, effort) {
  return (((_truthy) => _truthy !== false && _truthy != null)(((_logical) => (_logical !== false && _logical != null ? concrete_model : _logical))(state.routingRequest)) ? resolveRoute(provider, state.routingRequest.capabilityFloor, state.routingRequest.serviceClass, null, effort).model : null);
}

function availability_required_p(state, provider, concrete_model, effort) {
  return (((provider === "anthropic") || (provider === "openai")) ? ((((_truthy) => _truthy !== false && _truthy != null)(state.exactModelPinned)) ? true : (((_truthy) => _truthy !== false && _truthy != null)(((_logical) => (_logical !== false && _logical != null ? concrete_model : _logical))(state.routingRequest))) ? (() => { try {
    return (!(concrete_model === canonical_route_model(state, provider, concrete_model, effort)));
  } catch (_catch_26) {
    switch ($$bd$catch_dispatch(_catch_26, [Error])) {
      case 0: {
        const __error = _catch_26;
        return true;
        break;
      }
    }
  } })() : false) : false);
}

function model_availability_binding(state, options, provider, concrete_model, effort, availability) {
  const required = availability_required_p(state, provider, concrete_model, effort);
  const existing = options.northModelAvailability;
  const target_id = ((_logical) => (_logical !== false && _logical != null ? _logical : ((_logical) => (_logical !== false && _logical != null ? _logical : provider))(existing.targetId)))(availability.targetId);
  const env = ((_logical) => (_logical !== false && _logical != null ? _logical : process.env))(options.env);
  return deep_freeze($$bh$host_object($$bc$keyword("required"), required, $$bc$keyword("targetId"), target_id, $$bc$keyword("model"), concrete_model, $$bc$keyword("receipt"), (((_truthy) => _truthy !== false && _truthy != null)(((_logical) => (_logical !== false && _logical != null ? availability : _logical))(required)) ? availability.receipt : null), $$bc$keyword("observationPath"), ((_logical) => (_logical !== false && _logical != null ? _logical : providerModelObservationPath(env)))(existing.observationPath)));
}

function route_options(state, options, provider, concrete_model, effort, composed, availability) {
  return Object.assign({}, ((_logical) => (_logical !== false && _logical != null ? _logical : options))(state.routeBase), {[$$bc$property_key($$bc$keyword("model"))]: ((_logical) => (_logical !== false && _logical != null ? _logical : state.initialModel))(concrete_model), [$$bc$property_key($$bc$keyword("effort"))]: ((_logical) => (_logical !== false && _logical != null ? _logical : state.initialEffort))(effort), [$$bc$property_key($$bc$keyword("systemPrompt"))]: composed.prompt, [$$bc$property_key($$bc$keyword("northModelAvailability"))]: model_availability_binding(state, options, provider, concrete_model, effort, availability)});
}

function apply_harness_route_bang(...$beagle$args) {
  if (arguments.length === 2) {
    const options = $beagle$args[0];
    const provider = $beagle$args[1];
    return apply_harness_route_bang(options, provider, null, null, null);
  }
  if (arguments.length === 3) {
    const options = $beagle$args[0];
    const provider = $beagle$args[1];
    const model = $beagle$args[2];
    return apply_harness_route_bang(options, provider, model, null, null);
  }
  if (arguments.length === 4) {
    const options = $beagle$args[0];
    const provider = $beagle$args[1];
    const model = $beagle$args[2];
    const effort = $beagle$args[3];
    return apply_harness_route_bang(options, provider, model, effort, null);
  }
  if (arguments.length === 5) {
    const options = $beagle$args[0];
    const provider = $beagle$args[1];
    const model = $beagle$args[2];
    const effort = $beagle$args[3];
    const availability = $beagle$args[4];
    const state = harness_composition.get(options);
    if ((state == null)) {
      return $$bh$host_object($$bc$keyword("options"), options);
    } else {
      const source_seal = harness_authority_seals.get(options);
      if (((_truthy) => _truthy !== false && _truthy != null)(((_logical) => (_logical !== false && _logical != null ? (!hasCanonicalHarnessAuthority(options, source_seal.provider)) : _logical))(source_seal))) {
        (() => { throw new Error("harness authority source mutated before route application"); })();
      }
      if (((_truthy) => _truthy !== false && _truthy != null)(((_logical) => (_logical !== false && _logical != null ? ((!(options.northRoutingRequest === state.routingRequest)) || ((!(options.northCapabilities === state.capabilities)) || (!hasCanonicalAuthoringHooks(options)))) : _logical))(state.routingRequest))) {
        (() => { throw new Error("harness composition root mutated before route application"); })();
      }
      const concrete_model = resolveModelAlias(provider, model);
      const composed = compose_system_prompt(state, provider, concrete_model);
      if ((provider === "anthropic")) {
        assert_canonical_global_agents_exactly_once_bang(composed.prompt, state.environment);
      }
      const next = route_options(state, options, provider, concrete_model, effort, composed, availability);
      const renew_activity = harness_activity_renewers.get(options);
      const evidence = Object.assign({}, state.evidence, {[$$bc$property_key($$bc$keyword("modelDelta"))]: composed.deltaEvidence, [$$bc$property_key($$bc$keyword("promptEconomics"))]: composed.economics, [$$bc$property_key($$bc$keyword("promptReceipt"))]: composed.receipt});
      harness_composition.set(next, state);
      if (((_truthy) => _truthy !== false && _truthy != null)(renew_activity)) {
        harness_activity_renewers.set(next, renew_activity);
      }
      inherit_authoring_hook_seal_bang(options, next);
      applied_evidence.set(next, deep_freeze(evidence));
      seal_harness_authority_bang(next, provider);
      return $$bh$host_object($$bc$keyword("options"), next, $$bc$keyword("evidence"), evidence);
    }
  }
  throw new Error('No matching arity: ' + $beagle$args.length);
}

const applyHarnessRoute = apply_harness_route_bang;

function harnessRouteSeed(options) {
  const state = harness_composition.get(options);
  return (((_truthy) => _truthy !== false && _truthy != null)(state) ? $$bh$host_object($$bc$keyword("provider"), state.initialProvider, $$bc$keyword("model"), state.initialModel) : null);
}

function harnessCompositionEvidence(options) {
  return ((_logical) => (_logical !== false && _logical != null ? _logical : harness_composition.get(options).evidence))(applied_evidence.get(options));
}

function renewHarnessPresence(options) {
  const renewer = harness_activity_renewers.get(options);
  if (((_truthy) => _truthy !== false && _truthy != null)(renewer)) {
    renewer();
  }
  return null;
}

function praxisAppendix(...$beagle$args) {
  if (arguments.length === 0) {
    return praxisAppendix(null, null, null);
  }
  if (arguments.length === 1) {
    const __model = $beagle$args[0];
    return praxisAppendix(__model, null, null);
  }
  if (arguments.length === 2) {
    const __model = $beagle$args[0];
    const role = $beagle$args[1];
    return praxisAppendix(__model, role, null);
  }
  if (arguments.length === 3) {
    const __model = $beagle$args[0];
    const role = $beagle$args[1];
    const posture = $beagle$args[2];
    const blocks = [];
    if (((_truthy) => _truthy !== false && _truthy != null)(role)) {
      blocks.push($$bc$str("## Praxis — role: ", role, "\n", exact_section_fence(resolve(orchestration_docs(), "roles.md"), role, $$bc$str("role:", role))));
    }
    if (((_truthy) => _truthy !== false && _truthy != null)(posture)) {
      blocks.push($$bc$str("## Praxis — posture: ", posture, "\n", exact_section_fence(resolve(orchestration_docs(), "postures.md"), posture, $$bc$str("posture:", posture))));
    }
    return ($$bc$empty_p(blocks) ? "" : $$bc$str("\n\n", blocks.join("\n\n")));
  }
  throw new Error('No matching arity: ' + $beagle$args.length);
}

const FIRN_GUARDS = resolveManagedGuardChain(["firn-system-policy"]);

const EDIT_GUARDS = resolveManagedGuardChain(["launch-critical-worktree-guard.sh", "concrete-model-identity-guard.sh"]);

const BASH_GUARDS = resolveManagedGuardChain(["launch-critical-worktree-guard.sh", "git-blind-stage-guard.sh", "tripwire-guard.sh", "corpus-scan-guard.sh", "resource-safe-search-guard.sh", "session-kill-guard.sh", "concrete-model-identity-guard.sh"]);

const WORKER_BASH_GUARDS = resolveManagedGuardChain(["agent-spawn-guard.sh", "launch-critical-worktree-guard.sh", "git-blind-stage-guard.sh", "tripwire-guard.sh", "corpus-scan-guard.sh", "resource-safe-search-guard.sh", "session-kill-guard.sh", "concrete-model-identity-guard.sh"]);

function receipt_file_artifact(id, path) {
  return (() => { try {
    const info = statSync(path);
  return ((!((_truthy) => _truthy !== false && _truthy != null)(info.isFile())) ? $$bh$host_object($$bc$keyword("id"), id, $$bc$keyword("coverage"), "unknown") : $$bh$host_object($$bc$keyword("id"), id, $$bc$keyword("sha256"), sha256Bytes(readFileSync(path)), $$bc$keyword("coverage"), "exact"));
  } catch (_catch_27) {
    switch ($$bd$catch_dispatch(_catch_27, [Error])) {
      case 0: {
        const __error = _catch_27;
        return $$bh$host_object($$bc$keyword("id"), id, $$bc$keyword("coverage"), "unknown");
        break;
      }
    }
  } })();
}

function unique_guard_paths() {
  const values = FIRN_GUARDS.concat(EDIT_GUARDS, WORKER_BASH_GUARDS);
  return values.filter((path, index) => (values.indexOf(path) === index));
}

function receipt_tools(allowed, disallowed) {
  const names = allowed.map((name) => $$bc$str("allow:", name)).concat(disallowed.map((name) => $$bc$str("deny:", name))).sort();
  return names.map((name, index) => $$bh$host_object($$bc$keyword("id"), $$bc$str("tool-", index), $$bc$keyword("sha256"), sha256Bytes(name), $$bc$keyword("coverage"), "exact"));
}

function skill_receipt_artifacts(catalog) {
  return catalog.candidates.map((candidate) => $$bh$host_object($$bc$keyword("id"), $$bc$str("skill:", candidate.name), $$bc$keyword("sha256"), sha256Bytes(JSON.stringify(candidate)), $$bc$keyword("coverage"), "exact"));
}

function instruction_receipts(global, project) {
  return ((((_truthy) => _truthy !== false && _truthy != null)(global) ? [{[$$bc$property_key($$bc$keyword("id"))]: "global-instructions", [$$bc$property_key($$bc$keyword("sha256"))]: sha256Bytes(global.bytes), [$$bc$property_key($$bc$keyword("coverage"))]: "exact"}] : [])).concat([{[$$bc$property_key($$bc$keyword("id"))]: "project-instructions", [$$bc$property_key($$bc$keyword("sha256"))]: sha256Bytes(project), [$$bc$property_key($$bc$keyword("coverage"))]: "exact"}]);
}

function harness_environment_receipt(args) {
  const env = args.env;
  const global = canonicalGlobalAgents(env);
  const project = projectAgentsAppendix(args.cwd, env);
  return buildEnvironmentReceipt($$bh$host_object($$bc$keyword("availableSkills"), ((_logical) => (_logical !== false && _logical != null ? _logical : skill_receipt_artifacts(args.skillCatalog)))(args.availableSkills), $$bc$keyword("activatedResources"), ((_logical) => (_logical !== false && _logical != null ? _logical : [{[$$bc$property_key($$bc$keyword("id"))]: "activated-resource-observation", [$$bc$property_key($$bc$keyword("coverage"))]: "unknown"}]))(args.activatedResources), $$bc$keyword("tools"), receipt_tools(args.allowedTools, args.disallowedTools), $$bc$keyword("hooks"), unique_guard_paths().map((path, index) => receipt_file_artifact($$bc$str("hook-", index), path)), $$bc$keyword("configs"), [{[$$bc$property_key($$bc$keyword("id"))]: "routing-request", [$$bc$property_key($$bc$keyword("coverage"))]: "exact", [$$bc$property_key($$bc$keyword("sha256"))]: sha256Bytes(JSON.stringify(((_logical) => (_logical !== false && _logical != null ? _logical : {}))(args.routingMetadata)))}, receipt_file_artifact("learning-policy", ((_logical) => (_logical !== false && _logical != null ? _logical : resolve(((_logical) => (_logical !== false && _logical != null ? _logical : ""))(env.HOME), ".config/north/learning-policy.json")))(env.NORTH_LEARNING_POLICY))], $$bc$keyword("executables"), [receipt_file_artifact("north-executable", ENGINE), receipt_file_artifact("north-mcp-executable", MCP), receipt_file_artifact("babashka-executable", current_path_executable(((_logical) => (_logical !== false && _logical != null ? _logical : "bb"))(env.NORTH_PEER_BB), env))], $$bc$keyword("instructions"), instruction_receipts(global, project), $$bc$keyword("coverageReason"), (((_truthy) => _truthy !== false && _truthy != null)(args.activatedResources) ? null : "activated-resource-observation-unavailable")));
}

function guard_denial(self, reason, input, observation) {
  recordDenial(self, reason, input);
  return $$bh$host_object($$bc$keyword("hookSpecificOutput"), $$bh$host_object($$bc$keyword("hookEventName"), "PreToolUse", $$bc$keyword("permissionDecision"), "deny", $$bc$keyword("permissionDecisionReason"), reason, $$bc$keyword("additionalContext"), (((_truthy) => _truthy !== false && _truthy != null)(observation) ? observation.raw : null)));
}

function guard_environment(topology) {
  const managed_runtime = "/etc/codex/hooks/runtime";
  const env = Object.assign({}, process.env, {[$$bc$property_key($$bc$keyword("PATH"))]: [managed_runtime, process.env.PATH].filter((value) => Boolean(value)).join(delimiter), [$$bc$property_key($$bc$keyword("NORTH_AGENT_PYTHON"))]: resolve(managed_runtime, "python3")});
  if (((_truthy) => _truthy !== false && _truthy != null)(topology)) {
    (($beagle$host$arg$0, $beagle$host$arg$1, $beagle$host$arg$2) => $$bh$aset($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1, $beagle$host$arg$2))(env, "AGENT_TOPOLOGY", topology);
  }
  return env;
}

async function guard_hook(self, scripts, input, topology) {
  return (async () => { try {
    const decision = await evaluateGuards(scripts, input, 10000, guard_environment(topology));
  return (((decision.decision === "deny")) ? guard_denial(self, decision.reason, input, decision.observation) : (((_truthy) => _truthy !== false && _truthy != null)(((decision.decision === "allow") && decision.observation))) ? $$bh$host_object($$bc$keyword("continue"), true, $$bc$keyword("hookSpecificOutput"), $$bh$host_object($$bc$keyword("hookEventName"), "PreToolUse", $$bc$keyword("additionalContext"), decision.observation.raw)) : $$bh$host_object($$bc$keyword("continue"), true));
  } catch (_catch_28) {
    switch ($$bd$catch_dispatch(_catch_28, [Error])) {
      case 0: {
        const __error = _catch_28;
        return guard_denial(self, "authoring_guard_unavailable", input, null);
        break;
      }
    }
  } })();
}

function remove_ambient_keys_bang(env, keys) {
  keys.forEach((key) => {
  Reflect.deleteProperty(env, key);
});
  return env;
}

function ambient_child_environment_bang() {
  return remove_ambient_keys_bang(Object.assign($$bh$js_obj(), process.env), ["NORTH_DISPATCH_DRIVER_PRECLAIMED", "NORTH_RUN_ID", "NORTH_THREAD_ID", "NORTH_RUN_CAPABILITY", "NORTH_RUN_ARTIFACT_DIR", "NORTH_MANAGED_LANE", "NORTH_ORCHESTRATION_ROLE", "NORTH_CODEX_BIN", "NORTH_BIN", "AGENT_MODEL", "AGENT_PROJECT_PROFILE", "AGENT_REASONING", "AGENT_CAPABILITY_FLOOR", "AGENT_SERVICE_CLASS"]);
}

function frozen_copy(values) {
  return Object.freeze(Array.from(values));
}

function add_optional_field_bang(object, key, value) {
  if ((!(value == null))) {
    (($beagle$host$arg$0, $beagle$host$arg$1, $beagle$host$arg$2) => $$bh$aset($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1, $beagle$host$arg$2))(object, key, value);
  }
  return object;
}

function resolved_harness_model(options, metadata) {
  return (((_truthy) => _truthy !== false && _truthy != null)(((_logical) => (_logical !== false && _logical != null ? metadata : _logical))(options.provider)) ? resolveRoute(options.provider, metadata.capabilityFloor, metadata.serviceClass, options.model, metadata.reasoning).model : options.model);
}

function data_only_skill_catalog(environment) {
  return Object.freeze({[$$bc$property_key($$bc$keyword("root"))]: domainSkillsDir(environment), [$$bc$property_key($$bc$keyword("roots"))]: Object.freeze([]), [$$bc$property_key($$bc$keyword("candidates"))]: Object.freeze([]), [$$bc$property_key($$bc$keyword("appendix"))]: ""});
}

function project_exposure_context(profile) {
  return ((profile == null) ? "" : ["\n\n## Resolved project exposure context\n", "This lifecycle context is resolved at routing admission and remains outside the portable request.\n", $$bc$str(canonicalReceiptJson(profile), "\n")].join(""));
}

function data_only_policy(capability_policy) {
  return $$bh$host_object($$bc$keyword("tools"), [], $$bc$keyword("allowedTools"), [], $$bc$keyword("disallowedTools"), Array.from(new Set(capability_policy.allowedTools.concat(capability_policy.disallowedTools))));
}

function default_disallowed_tools(orchestration_allowed) {
  return Array.from(new Set(NATIVE__AGENT__TOOLS.concat((orchestration_allowed ? [] : ORCHESTRATION__TOOLS))));
}

function default_allowed_tools(options, disallowed, orchestration_allowed) {
  return Array.from(new Set(((_logical) => (_logical !== false && _logical != null ? _logical : []))(options.extraTools).filter((name) => (!((_truthy) => _truthy !== false && _truthy != null)(disallowed.includes(name)))).concat(COORDINATION__TOOLS, (orchestration_allowed ? ORCHESTRATION__TOOLS : []))));
}

function managed_child_environment_bang(options, metadata, cwd) {
  const ambient = ambient_child_environment_bang();
  const bin_directory = dirname(ENGINE);
  const child = Object.assign(ambient, {[$$bc$property_key($$bc$keyword("NORTH_BIN"))]: ENGINE, [$$bc$property_key($$bc$keyword("PATH"))]: (((_truthy) => _truthy !== false && _truthy != null)(ambient.PATH) ? $$bc$str(bin_directory, delimiter, ambient.PATH) : bin_directory), [$$bc$property_key($$bc$keyword("AGENT_ID"))]: options.self, [$$bc$property_key($$bc$keyword("AGENT_TOPOLOGY"))]: options.enforcementTopology, [$$bc$property_key($$bc$keyword("NORTH_MANAGED_LANE"))]: "1", [$$bc$property_key($$bc$keyword("NORTH_PORT"))]: north_port()});
  if (((_truthy) => _truthy !== false && _truthy != null)(options.projectProfile)) {
    (($beagle$host$arg$0, $beagle$host$arg$1, $beagle$host$arg$2) => $$bh$aset($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1, $beagle$host$arg$2))(child, "AGENT_PROJECT_PROFILE", JSON.stringify(options.projectProfile));
  }
  if (((_truthy) => _truthy !== false && _truthy != null)(metadata.capabilityFloor)) {
    (($beagle$host$arg$0, $beagle$host$arg$1, $beagle$host$arg$2) => $$bh$aset($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1, $beagle$host$arg$2))(child, "AGENT_CAPABILITY_FLOOR", metadata.capabilityFloor);
  }
  if (((_truthy) => _truthy !== false && _truthy != null)(metadata.serviceClass)) {
    (($beagle$host$arg$0, $beagle$host$arg$1, $beagle$host$arg$2) => $$bh$aset($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1, $beagle$host$arg$2))(child, "AGENT_SERVICE_CLASS", metadata.serviceClass);
  }
  if (((_truthy) => _truthy !== false && _truthy != null)(metadata.reasoning)) {
    (($beagle$host$arg$0, $beagle$host$arg$1, $beagle$host$arg$2) => $$bh$aset($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1, $beagle$host$arg$2))(child, "AGENT_REASONING", metadata.reasoning);
  }
  if (((_truthy) => _truthy !== false && _truthy != null)(metadata.role)) {
    (($beagle$host$arg$0, $beagle$host$arg$1, $beagle$host$arg$2) => $$bh$aset($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1, $beagle$host$arg$2))(child, "NORTH_ORCHESTRATION_ROLE", metadata.role);
  }
  if (((_truthy) => _truthy !== false && _truthy != null)(options.deliveryRun)) {
    (($beagle$host$arg$0, $beagle$host$arg$1, $beagle$host$arg$2) => $$bh$aset($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1, $beagle$host$arg$2))(child, "NORTH_RUN_ID", options.deliveryRun.runId);
    (($beagle$host$arg$0, $beagle$host$arg$1, $beagle$host$arg$2) => $$bh$aset($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1, $beagle$host$arg$2))(child, "NORTH_THREAD_ID", options.deliveryRun.threadId);
    (($beagle$host$arg$0, $beagle$host$arg$1, $beagle$host$arg$2) => $$bh$aset($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1, $beagle$host$arg$2))(child, "NORTH_RUN_CAPABILITY", options.deliveryRun.capability);
  }
  return child;
}

function harness_presence_renewer(options) {
  return (((options.presenceRenewer === false)) ? null : (((_truthy) => _truthy !== false && _truthy != null)(options.presenceRenewer)) ? options.presenceRenewer : ((options.presenceRegistrar == null)) ? renew_presence : null);
}

function north_mcp_environment(child, options) {
  const source = Object.assign($$bh$js_obj(), child, {[$$bc$property_key($$bc$keyword("NORTH_BIN"))]: ENGINE});
  if (((_truthy) => _truthy !== false && _truthy != null)(options.artifactDirectory)) {
    (($beagle$host$arg$0, $beagle$host$arg$1, $beagle$host$arg$2) => $$bh$aset($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1, $beagle$host$arg$2))(source, "NORTH_RUN_ARTIFACT_DIR", options.artifactDirectory);
  }
  return Object.freeze(managedNorthMcpEnvironment(source));
}

function build_mcp_servers_bang(options, cwd, child, capabilities, orchestration_allowed, readonly_shell) {
  const servers = $$bh$js_obj();
  const data_only = (options.dataOnly === true);
  if ((!data_only)) {
    (($beagle$host$arg$0, $beagle$host$arg$1, $beagle$host$arg$2) => $$bh$aset($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1, $beagle$host$arg$2))(servers, "north", Object.freeze({[$$bc$property_key($$bc$keyword("type"))]: "stdio", [$$bc$property_key($$bc$keyword("command"))]: MCP, [$$bc$property_key($$bc$keyword("args"))]: Object.freeze([]), [$$bc$property_key($$bc$keyword("env"))]: north_mcp_environment(child, options)}));
  }
  if ((orchestration_allowed && (!data_only))) {
    (($beagle$host$arg$0, $beagle$host$arg$1, $beagle$host$arg$2) => $$bh$aset($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1, $beagle$host$arg$2))(servers, "north-peer", Object.freeze(peer_command_server_bang(options.self)));
  }
  if ((readonly_shell && (!data_only))) {
    (($beagle$host$arg$0, $beagle$host$arg$1, $beagle$host$arg$2) => $$bh$aset($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1, $beagle$host$arg$2))(servers, READONLY__SHELL__SERVER, Object.freeze(readonly_shell_server(cwd, child, options.abortController.signal)));
  }
  return Object.freeze(servers);
}

function frozen_policy_tools(policy, selector) {
  return (((_truthy) => _truthy !== false && _truthy != null)(policy) ? Object.freeze(Array.from((((selector === "tools")) ? policy.tools : ((selector === "allowed")) ? policy.allowedTools : policy.disallowedTools))) : null);
}

function composition_seed(options, metadata, cwd, catalog, orchestration, capabilities, environment, effective_model, effective_effort, environment_receipt) {
  return $$bh$host_object($$bc$keyword("self"), options.self, $$bc$keyword("basePrompt"), $$bc$str(((_logical) => (_logical !== false && _logical != null ? _logical : DEFAULT__SYSTEM__PROMPT))(options.systemPrompt), project_exposure_context(options.projectProfile), (((_truthy) => _truthy !== false && _truthy != null)(options.dataOnly) ? "" : eso_appendix(environment))), $$bc$keyword("skillCatalog"), catalog, $$bc$keyword("orchestrationAppendix"), orchestration.appendix, $$bc$keyword("capabilities"), (((_truthy) => _truthy !== false && _truthy != null)(capabilities) ? Array.from(capabilities) : null), $$bc$keyword("cwd"), cwd, $$bc$keyword("evidence"), Object.assign({}, orchestration.evidence, {[$$bc$property_key($$bc$keyword("capabilities"))]: capabilities, [$$bc$property_key($$bc$keyword("environmentReceipt"))]: environment_receipt}), $$bc$keyword("routingRequest"), metadata, $$bc$keyword("initialProvider"), options.provider, $$bc$keyword("initialModel"), effective_model, $$bc$keyword("initialEffort"), effective_effort, $$bc$keyword("omitModelDeltaReason"), options.omitModelDeltaReason, $$bc$keyword("exactModelPinned"), (((_truthy) => _truthy !== false && _truthy != null)(options.modelAvailability) ? options.modelAvailability.exactModelPinned : (!((_truthy) => _truthy !== false && _truthy != null)((options.model == null)))), $$bc$keyword("dataOnly"), (options.dataOnly === true), $$bc$keyword("environment"), environment);
}

function authority_hooks(options, orchestration_allowed, enforcement_topology, presence_renewer) {
  return $$bh$host_object($$bc$keyword("PreToolUse"), [{[$$bc$property_key($$bc$keyword("hooks"))]: [(input) => guard_hook(options.self, FIRN_GUARDS, input, null)]}, {[$$bc$property_key($$bc$keyword("matcher"))]: "Edit|Write|MultiEdit", [$$bc$property_key($$bc$keyword("hooks"))]: [(input) => guard_hook(options.self, EDIT_GUARDS, input, null)]}, {[$$bc$property_key($$bc$keyword("matcher"))]: "Bash", [$$bc$property_key($$bc$keyword("hooks"))]: [(input) => guard_hook(options.self, (((_truthy) => _truthy !== false && _truthy != null)(orchestration_allowed) ? BASH_GUARDS : WORKER_BASH_GUARDS), input, enforcement_topology)]}], $$bc$keyword("PostToolUse"), [{[$$bc$property_key($$bc$keyword("hooks"))]: [() => { if (((_truthy) => _truthy !== false && _truthy != null)(presence_renewer)) {
  presence_renewer(options.self);
}
return $$bh$host_object($$bc$keyword("continue"), true); }]}]);
}

function base_harness_options(source, metadata, capabilities, policy, mcp_servers, child, cwd, system_prompt, effective_model, effective_effort, allowed, disallowed, enforcement_topology, presence_renewer) {
  const result = $$bh$host_object($$bc$keyword("mcpServers"), mcp_servers, $$bc$keyword("allowedTools"), Object.freeze(Array.from(allowed)), $$bc$keyword("model"), (((_truthy) => _truthy !== false && _truthy != null)(source.provider) ? resolveModelAlias(source.provider, effective_model) : effective_model), $$bc$keyword("effort"), effective_effort, $$bc$keyword("env"), child, $$bc$keyword("permissionMode"), (((_truthy) => _truthy !== false && _truthy != null)(((_logical) => (_logical !== false && _logical != null ? (!((_truthy) => _truthy !== false && _truthy != null)(capabilities.includes("filesystem.write"))) : _logical))(capabilities)) ? "default" : "acceptEdits"), $$bc$keyword("cwd"), cwd, $$bc$keyword("systemPrompt"), system_prompt, $$bc$keyword("maxTurns"), ((_logical) => (_logical !== false && _logical != null ? _logical : (() => { const configured = Number(process.env.AGENT_MAX_TURNS); return (((_truthy) => _truthy !== false && _truthy != null)(((_logical) => (_logical !== false && _logical != null ? (configured > 0) : _logical))(Number.isSafeInteger(configured))) ? configured : 200); })()))(source.maxTurns), $$bc$keyword("northModelAvailability"), deep_freeze($$bh$host_object($$bc$keyword("required"), false, $$bc$keyword("targetId"), ((_logical) => (_logical !== false && _logical != null ? _logical : ((_logical) => (_logical !== false && _logical != null ? _logical : "unresolved"))(source.provider)))(source.modelAvailability.targetId), $$bc$keyword("model"), (((_truthy) => _truthy !== false && _truthy != null)(source.provider) ? resolveModelAlias(source.provider, effective_model) : effective_model), $$bc$keyword("observationPath"), providerModelObservationPath(child))), $$bc$keyword("settings"), {[$$bc$property_key($$bc$keyword("autoCompactEnabled"))]: true}, $$bc$keyword("hooks"), authority_hooks(source, (enforcement_topology === "orchestrator"), enforcement_topology, presence_renewer));
  if (((_truthy) => _truthy !== false && _truthy != null)(policy)) {
    (($beagle$host$arg$0, $beagle$host$arg$1, $beagle$host$arg$2) => $$bh$aset($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1, $beagle$host$arg$2))(result, "tools", frozen_policy_tools(policy, "tools"));
    (($beagle$host$arg$0, $beagle$host$arg$1, $beagle$host$arg$2) => $$bh$aset($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1, $beagle$host$arg$2))(result, "settingSources", Object.freeze([]));
    (($beagle$host$arg$0, $beagle$host$arg$1, $beagle$host$arg$2) => $$bh$aset($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1, $beagle$host$arg$2))(result, "strictMcpConfig", true);
  }
  if ((disallowed.length > 0)) {
    (($beagle$host$arg$0, $beagle$host$arg$1, $beagle$host$arg$2) => $$bh$aset($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1, $beagle$host$arg$2))(result, "disallowedTools", Object.freeze(Array.from(disallowed)));
  }
  if (((_truthy) => _truthy !== false && _truthy != null)(capabilities)) {
    (($beagle$host$arg$0, $beagle$host$arg$1, $beagle$host$arg$2) => $$bh$aset($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1, $beagle$host$arg$2))(result, "northCapabilities", Object.freeze(Array.from(capabilities)));
  }
  if (((_truthy) => _truthy !== false && _truthy != null)(metadata)) {
    (($beagle$host$arg$0, $beagle$host$arg$1, $beagle$host$arg$2) => $$bh$aset($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1, $beagle$host$arg$2))(result, "northRoutingRequest", metadata);
  }
  if (((_truthy) => _truthy !== false && _truthy != null)(source.dataOnly)) {
    (($beagle$host$arg$0, $beagle$host$arg$1, $beagle$host$arg$2) => $$bh$aset($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1, $beagle$host$arg$2))(result, "northDataOnly", true);
  }
  if (((_truthy) => _truthy !== false && _truthy != null)(source.outputFormat)) {
    (($beagle$host$arg$0, $beagle$host$arg$1, $beagle$host$arg$2) => $$bh$aset($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1, $beagle$host$arg$2))(result, "outputFormat", source.outputFormat);
  }
  if ((!(source.persistSession == null))) {
    (($beagle$host$arg$0, $beagle$host$arg$1, $beagle$host$arg$2) => $$bh$aset($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1, $beagle$host$arg$2))(result, "persistSession", source.persistSession);
  }
  if (((_truthy) => _truthy !== false && _truthy != null)(source.abortController)) {
    (($beagle$host$arg$0, $beagle$host$arg$1, $beagle$host$arg$2) => $$bh$aset($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1, $beagle$host$arg$2))(result, "abortController", source.abortController);
  }
  return result;
}

function harness_options_bang(options) {
  const cwd = ((_logical) => (_logical !== false && _logical != null ? _logical : process.cwd()))(options.cwd);
  const composer_environment = Object.freeze(Object.assign({}, process.env));
  const metadata = (((_truthy) => _truthy !== false && _truthy != null)(options.routingMetadata) ? admitRoutingRequest(options.routingMetadata, "managed North harness") : null);
  const effective_effort = metadata.reasoning;
  const effective_model = resolved_harness_model(options, metadata);
  const topology = metadata.topology;
  const orchestration = orchestration_appendix_bang(metadata, cwd, composer_environment);
  const capabilities = orchestration.evidence.capabilities;
  const catalog = (((_truthy) => _truthy !== false && _truthy != null)(options.dataOnly) ? data_only_skill_catalog(composer_environment) : activeSkillCatalog(composer_environment, cwd));
  const orchestration_allowed = ((topology === "orchestrator") && ((_logical) => (_logical !== false && _logical != null ? capabilities.includes("coordination") : _logical))(capabilities));
  const capability_policy = (((_truthy) => _truthy !== false && _truthy != null)(capabilities) ? managedToolPolicy(capabilities) : null);
  const policy = (((_truthy) => _truthy !== false && _truthy != null)(((_logical) => (_logical !== false && _logical != null ? capability_policy : _logical))(options.dataOnly)) ? data_only_policy(capability_policy) : capability_policy);
  const disallowed = (((_truthy) => _truthy !== false && _truthy != null)(policy) ? policy.disallowedTools : default_disallowed_tools(orchestration_allowed));
  const allowed = (((_truthy) => _truthy !== false && _truthy != null)(policy) ? policy.allowedTools : default_allowed_tools(options, disallowed, orchestration_allowed));
  const enforcement_topology = (orchestration_allowed ? "orchestrator" : "worker");
  const options_with_topology = Object.assign({}, options, {[$$bc$property_key($$bc$keyword("enforcementTopology"))]: enforcement_topology});
  const child = Object.freeze(managed_child_environment_bang(options_with_topology, metadata, cwd));
  const presence_renewer = harness_presence_renewer(options);
  const readonly_shell = ((_logical) => (_logical !== false && _logical != null ? capabilities.includes("shell.readonly") : _logical))(capabilities);
  const mcp_servers = build_mcp_servers_bang(options, cwd, child, ((_logical) => (_logical !== false && _logical != null ? _logical : []))(capabilities), orchestration_allowed, readonly_shell);
  const environment_receipt = harness_environment_receipt($$bh$host_object($$bc$keyword("env"), composer_environment, $$bc$keyword("cwd"), cwd, $$bc$keyword("allowedTools"), allowed, $$bc$keyword("disallowedTools"), disallowed, $$bc$keyword("routingMetadata"), metadata, $$bc$keyword("activatedResources"), options.activatedResources, $$bc$keyword("availableSkills"), options.availableSkills, $$bc$keyword("skillCatalog"), catalog));
  const seed = composition_seed(options, metadata, cwd, catalog, orchestration, capabilities, composer_environment, effective_model, effective_effort, environment_receipt);
  const initial_route_model = (((_truthy) => _truthy !== false && _truthy != null)(options.provider) ? resolveModelAlias(options.provider, effective_model) : effective_model);
  const initial_composition = compose_system_prompt(seed, options.provider, initial_route_model);
  const initial_system_prompt = initial_composition.prompt;
  if ((options.provider == null)) {
    assert_canonical_global_agents_exactly_once_bang(initial_system_prompt, composer_environment);
  }
  const result = base_harness_options(options, metadata, capabilities, policy, mcp_servers, child, cwd, initial_system_prompt, effective_model, effective_effort, allowed, disallowed, enforcement_topology, presence_renewer);
  deep_freeze(result.hooks);
  const state = Object.assign({}, seed, {[$$bc$property_key($$bc$keyword("capabilities"))]: ((_logical) => (_logical !== false && _logical != null ? _logical : seed.capabilities))(result.northCapabilities), [$$bc$property_key($$bc$keyword("routeBase"))]: Object.freeze(Object.assign({}, result))});
  harness_composition.set(result, state);
  if (((_truthy) => _truthy !== false && _truthy != null)(presence_renewer)) {
    harness_activity_renewers.set(result, () => { presence_renewer(options.self);
return null; });
  }
  applied_evidence.set(result, deep_freeze(Object.assign({}, seed.evidence, {[$$bc$property_key($$bc$keyword("modelDelta"))]: initial_composition.deltaEvidence, [$$bc$property_key($$bc$keyword("promptEconomics"))]: initial_composition.economics, [$$bc$property_key($$bc$keyword("promptReceipt"))]: initial_composition.receipt})));
  seal_authoring_hooks_bang(result);
  const routed = (((_truthy) => _truthy !== false && _truthy != null)(options.provider) ? apply_harness_route_bang(result, options.provider, effective_model, effective_effort, $$bh$host_object($$bc$keyword("targetId"), ((_logical) => (_logical !== false && _logical != null ? _logical : options.provider))(options.modelAvailability.targetId), $$bc$keyword("receipt"), options.modelAvailability.receipt)).options : result);
  if ((!(options.presenceRegistrar === false))) {
    const registrar = ((_logical) => (_logical !== false && _logical != null ? _logical : register_presence))(options.presenceRegistrar);
    registrar(options.self, cwd);
  }
  return routed;
}

const harnessOptions = harness_options_bang;

const DEFAULT__SYSTEM__PROMPT = $$bc$str("You are a north agent on a shared coordination graph — recursive Triples with ", "assertion history. Prefer native north coordination ", "tools over editing coordination state: capture/tell to record work and ready/next ", "to find it. Your Orchestration topology contract, when present, is the sole source of ", "delegation authority. Acquire before editing shared code. Report concisely.");

export { COMPACTION__POLICY__VERSION as "COMPACTION_POLICY_VERSION" };
export { COORDINATION__TOOLS as "COORDINATION_TOOLS" };
export { DEFAULT__SYSTEM__PROMPT as "DEFAULT_SYSTEM_PROMPT" };
export { GLOBAL__AGENTS__MAX__BYTES as "GLOBAL_AGENTS_MAX_BYTES" };
export { NATIVE__AGENT__TOOLS as "NATIVE_AGENT_TOOLS" };
export { NORTH__MCP__TOOL__NAMES as "NORTH_MCP_TOOL_NAMES" };
export { ORCHESTRATION__TOOLS as "ORCHESTRATION_TOOLS" };
export { PROJECT__AGENTS__MAX__BYTES as "PROJECT_AGENTS_MAX_BYTES" };
export { PROMPT__COMPOSITION__VERSION as "PROMPT_COMPOSITION_VERSION" };
export { activeSkillCatalog as "activeSkillCatalog" };
export { applyHarnessRoute as "applyHarnessRoute" };
export { canonicalGlobalAgents as "canonicalGlobalAgents" };
export { canonicalHarnessModelAvailability as "canonicalHarnessModelAvailability" };
export { domainSkillsDir as "domainSkillsDir" };
export { globalLawsPath as "globalLawsPath" };
export { harnessCompositionEvidence as "harnessCompositionEvidence" };
export { harnessOptions as "harnessOptions" };
export { harnessRouteSeed as "harnessRouteSeed" };
export { hasCanonicalAuthoringHooks as "hasCanonicalAuthoringHooks" };
export { hasCanonicalHarnessAuthority as "hasCanonicalHarnessAuthority" };
export { managedToolPolicy as "managedToolPolicy" };
export { orchestrationAppendix as "orchestrationAppendix" };
export { peerCommandServer as "peerCommandServer" };
export { praxisAppendix as "praxisAppendix" };
export { projectAgentsAppendix as "projectAgentsAppendix" };
export { projectSkillTarget as "projectSkillTarget" };
export { renewHarnessPresence as "renewHarnessPresence" };
export { sendPeerCommand as "sendPeerCommand" };
export { validatePeerCommandArgs as "validatePeerCommandArgs" };
