// §4 ATTRIBUTION reference model (TEST-ONLY, non-production). Encodes the B2
// four-envelope precedence + legacy :frame law and the rejected B-prime shape,
// pinned to CURRENT observed North source:
//   * fact :by order (E1/E2)  = NORTH_AGENT_ID > AGENT_ID > "user"
//       (src/north/main.bclj:834-842 — observed this session)
//   * legacy real-edit classifier ∈ {coord, agent, cli}
//       (src/north/staleness.bclj:36-38 — observed this session)
//   * mcp-coordinator order (DISTINCT surface, NOT fact :by, unchanged in both)
//       = explicit > caller AGENT_ID > NORTH_AGENT_ID
//       (bin/north-mcp:276-285 — observed this session)
// B-prime (REVOKED): AGENT_ID > NORTH_AGENT_ID precedence for fact :by, and the
// "daemon keeps :frame as the honest principal" premise.
import { check, section, note, finish } from "./harness.mjs";

// ---- fact :by resolution for E1 (fram cold CLI) / E2 (North Clojure client) --
function byE1(model, env) {
  const { NORTH_AGENT_ID, AGENT_ID } = env;
  if (model === "b2") return NORTH_AGENT_ID || AGENT_ID || "user";
  // bprime: the revoked AGENT_ID-first order.
  return AGENT_ID || NORTH_AGENT_ID || "user";
}
// E3 web-bjs JS client: authenticated session identity else "north-web".
const byE3 = (env) => env.session || "north-web";
// E4 authenticated gateway: OVERWRITES any caller-supplied :by.
const byE4 = (env) => `gateway:${env.tenant}`;
// mcp-coordinator (distinct surface): explicit > caller AGENT_ID > NORTH_AGENT_ID.
function mcpCoordinator(env, args) {
  return args.coordinator || env.AGENT_ID || env.NORTH_AGENT_ID || null;
}
// Legacy :frame classifier from staleness.bclj — a real edit iff frame∈{coord,agent,cli}.
const realEditFrame = (fr) => fr === "coord" || fr === "agent" || fr === "cli";

// A pinned session (NORTH_AGENT_ID) with a DIFFERENT inherited AGENT_ID — the
// case that distinguishes the two orders.
const pinnedEnv = { NORTH_AGENT_ID: "lane-self", AGENT_ID: "parent-inherited" };

section("E1/E2 fact :by precedence — NORTH_AGENT_ID beats AGENT_ID (B2)");
check("E1 B2 GREEN: pinned lane id (NORTH_AGENT_ID) wins", "lane-self", byE1("b2", pinnedEnv));
check("E1 B-prime RED: inherited AGENT_ID wrongly wins", "parent-inherited", byE1("bprime", pinnedEnv));
check("E1 B2 fallback to \"user\" when unset", "user", byE1("b2", {}));

section("E3/E4 envelopes");
check("E3 authenticated session identity", "sess-42", byE3({ session: "sess-42" }));
check("E3 anonymous web falls back to north-web", "north-web", byE3({}));
check("E4 gateway OVERWRITES caller :by with gateway:<tenant>", "gateway:acme",
  byE4({ tenant: "acme", session: "spoofed" }));

section("mcp-coordinator — DISTINCT surface, explicit>AGENT_ID>NORTH_AGENT_ID (unchanged both models)");
check("explicit coordinator arg wins", "interactive",
  mcpCoordinator(pinnedEnv, { coordinator: "interactive" }));
check("sub-spawn: caller AGENT_ID beats inherited pin", "parent-inherited",
  mcpCoordinator(pinnedEnv, {}));
note("mcp order is inverted vs fact :by BY DESIGN so an inherited pin never beats a lane's own sub-spawn id — both surfaces stay as-is, compatible by construction.");

section("Legacy :frame law + compat receipt matrix (R-compat)");
// B2 new daemon: daemon-mediated lines carry :frame "coord" (bug-compatible) + honest :by.
const newDaemonLine = { frame: "coord", by: "fram:snapshot" };
check("new-daemon line classified as real edit by OLD staleness (frame=coord)",
  true, realEditFrame(newDaemonLine.frame));
// old-client + new-daemon: daemon records :by "legacy:coord", :frame "coord".
const oldClientNewDaemon = { frame: "coord", by: "legacy:coord" };
check("old-client+new-daemon: :frame coord (staleness unchanged)",
  true, realEditFrame(oldClientNewDaemon.frame));
check("old-client+new-daemon: :by is legacy:coord, never bare coord",
  "legacy:coord", oldClientNewDaemon.by);
// new-client + old-daemon: old daemon ignores unknown :by key -> line = today's
// bytes, attribution DOWNGRADED, never rejected/misfolded.
function oldDaemonFold(line) {
  // old daemon keeps only known keys; :by is unknown -> dropped, never an error.
  const { by, ...known } = line;
  return { ...known, accepted: true };
}
const newClientOldDaemon = oldDaemonFold({ frame: "cli", by: "lane-self" });
check("new-client+old-daemon: line accepted (never rejected)", true, newClientOldDaemon.accepted);
check("new-client+old-daemon: unknown :by dropped, :frame preserved", "cli", newClientOldDaemon.frame);
// generation control record carries NO :frame -> old classifier excludes it (new
// line class, no regression).
check("generation control record has no :frame -> not a real edit for old classifier",
  false, realEditFrame(undefined));

finish();
