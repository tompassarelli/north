import { str as $$bc$str } from '../bridge/generated/beagle/core.js';
import { admit_host_object as $$bh$admit_host_object, aset as $$bh$aset, js_obj as $$bh$js_obj } from '../bridge/generated/beagle/host.js';

const crypto_module = require("node:crypto");

const create_hash = crypto_module.createHash;

const PROVIDER__JOIN__KEY__VERSION = "north-provider-join:v1";

const OPAQUE_PROVIDER_ID = new RegExp("^[A-Za-z0-9._:-]+$", "u");

function opaque_provider_id(value, label) {
  if (((!(typeof value === "string")) || ((value === "") || ((!(value === value.trim())) || ((Buffer.byteLength(value, "utf8") > 512) || (!((_truthy) => _truthy !== false && _truthy != null)(OPAQUE_PROVIDER_ID.test(value)))))))) {
    (() => { throw new Error($$bc$str(label, " is not a bounded opaque provider identifier")); })();
  }
  return value;
}

function digest(parts) {
  const hash = create_hash("sha256");
  parts.forEach((part) => {
  hash.update(part, "utf8");
});
  return hash.digest("hex");
}

function providerSessionKey(provider_session_id) {
  const id = opaque_provider_id(provider_session_id, "provider session id");
  return digest([$$bc$str("north-actor-key-v1\x00session\x00"), id]);
}

function providerTurnKey(provider, provider_turn_id) {
  const id = opaque_provider_id(provider_turn_id, "provider turn id");
  return digest(["north-provider-turn-key-v1\x00", provider, "\x00", id]);
}

function provider_join_evidence(provider, input) {
  const raw_session = input.sessionId;
  const session_key = ((raw_session === undefined) ? null : providerSessionKey(raw_session));
  const raw_turns = input.turnIds;
  const turn_ids = (Array.isArray(raw_turns) ? raw_turns : []);
  const turn_keys = Array.from(new Set(turn_ids.map((id) => providerTurnKey(provider, id)))).sort();
  const coverage = ((((_truthy) => _truthy !== false && _truthy != null)(((_logical) => (_logical !== false && _logical != null ? (turn_keys.length > 0) : _logical))(session_key))) ? "exact" : (((_truthy) => _truthy !== false && _truthy != null)(((_logical) => (_logical !== false && _logical != null ? _logical : (turn_keys.length > 0)))(session_key))) ? "partial" : "unknown");
  const result = $$bh$js_obj("version", PROVIDER__JOIN__KEY__VERSION, "turnKeys", Object.freeze(turn_keys), "sessionPersistence", input.sessionPersistence, "coverage", coverage);
  if (((_truthy) => _truthy !== false && _truthy != null)(session_key)) {
    (($beagle$host$arg$0, $beagle$host$arg$1, $beagle$host$arg$2) => $$bh$aset($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1, $beagle$host$arg$2))(result, "sessionKey", session_key);
  }
  return Object.freeze(result);
}

const providerJoinEvidence = provider_join_evidence;

function providerJoinEvidenceEqual(left, right) {
  return ((left.version === right.version) && ((left.sessionKey === right.sessionKey) && ((left.sessionPersistence === right.sessionPersistence) && ((left.coverage === right.coverage) && (left.turnKeys.join("\u0000") === right.turnKeys.join("\u0000"))))));
}

function fold_provider_join_evidence(evidence) {
  if ((evidence.length === 0)) {
    return undefined;
  } else {
    const sessions = new Set(evidence.map((entry) => entry.sessionKey).filter((session_key) => (!(session_key === undefined))));
    const persistences = new Set(evidence.map((entry) => entry.sessionPersistence));
    const session_key = ((sessions.size === 1) ? sessions.values().next().value : null);
    const session_persistence = ((persistences.size === 1) ? evidence[0].sessionPersistence : "unknown");
    const all_turn_keys = evidence.flatMap((entry) => entry.turnKeys);
    const turn_keys = Array.from(new Set(all_turn_keys)).sort();
    const exact_p = ((sessions.size <= 1) && ((persistences.size <= 1) && ((_logical) => (_logical !== false && _logical != null ? ((turn_keys.length > 0) && evidence.every((entry) => (entry.coverage === "exact"))) : _logical))(session_key)));
    const unknown_p = evidence.some((entry) => (entry.coverage === "unknown"));
    const coverage = ((unknown_p) ? "unknown" : (exact_p) ? "exact" : (((_truthy) => _truthy !== false && _truthy != null)(((_logical) => (_logical !== false && _logical != null ? _logical : (turn_keys.length > 0)))(session_key))) ? "partial" : "unknown");
    const result = $$bh$js_obj("version", PROVIDER__JOIN__KEY__VERSION, "turnKeys", Object.freeze(turn_keys), "sessionPersistence", session_persistence, "coverage", coverage);
    if (((_truthy) => _truthy !== false && _truthy != null)(session_key)) {
      (($beagle$host$arg$0, $beagle$host$arg$1, $beagle$host$arg$2) => $$bh$aset($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1, $beagle$host$arg$2))(result, "sessionKey", session_key);
    }
    return Object.freeze(result);
  }
}

const foldProviderJoinEvidence = fold_provider_join_evidence;

function provider_id(value) {
  if (((!(value === "anthropic")) && (!(value === "openai")))) {
    (() => { throw new Error("provider turn entry requires anthropic|openai"); })();
  }
  return value;
}

function record_value(value, label) {
  if (((!((_truthy) => _truthy !== false && _truthy != null)(value)) || ((!(typeof value === "object")) || Array.isArray(value)))) {
    (() => { throw new Error($$bc$str(label, " must be an object")); })();
  }
  return value;
}

function stdin_text_bang() {
  return Bun.stdin.text();
}

async function main_bang() {
  const input = record_value(JSON.parse(await stdin_text_bang()), "provider join input");
  const raw_sessions = input.sessions;
  const sessions = (Array.isArray(raw_sessions) ? raw_sessions.map((session) => providerSessionKey(session)) : []);
  const raw_turns = input.turns;
  const turns = (Array.isArray(raw_turns) ? raw_turns.map((entry) => { const turn = record_value(entry, "provider turn entry");
return providerTurnKey(provider_id(turn.provider), turn.id); }) : []);
  process.stdout.write($$bc$str(JSON.stringify($$bh$js_obj("version", PROVIDER__JOIN__KEY__VERSION, "sessions", sessions, "turns", turns)), "\n"));
  return null;
}

if (((_truthy) => _truthy !== false && _truthy != null)(import.meta.main)) {
  main_bang().catch((error) => { process.console.error(error);
(process.exitCode = 1);
return null; });
}

export { PROVIDER__JOIN__KEY__VERSION as "PROVIDER_JOIN_KEY_VERSION" };
export { foldProviderJoinEvidence as "foldProviderJoinEvidence" };
export { providerJoinEvidence as "providerJoinEvidence" };
export { providerJoinEvidenceEqual as "providerJoinEvidenceEqual" };
export { providerSessionKey as "providerSessionKey" };
export { providerTurnKey as "providerTurnKey" };
