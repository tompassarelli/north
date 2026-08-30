import { isAbsolute } from 'path';
import { "TrackedThing" as TrackedThing, "trackedthing-id" as trackedthing_id } from './model.js';
import { keyword as $$bc$keyword, property_key as $$bc$property_key, record_value as $$bc$record_value, str as $$bc$str } from '../../beagle/core.js';
import { catch_dispatch as $$bd$catch_dispatch } from '../../beagle/exception-dispatch.js';

function CommandChild(stdout, stderr, exited) {
  return $$bc$record_value("north.bridge.referent-actions/CommandChild", {_tag: "CommandChild", stdout, stderr, exited});
}

function commandchild_stdout(r) { return r.stdout; }

function commandchild_stderr(r) { return r.stderr; }

function commandchild_exited(r) { return r.exited; }

function PromiseConstructor(all) {
  return $$bc$record_value("north.bridge.referent-actions/PromiseConstructor", {_tag: "PromiseConstructor", all});
}

function promiseconstructor_all(r) { return r.all; }

const ACTIONS = ["track", "plan", "start", "assign", "request", "ack", "ownership", "settle", "show", "history", "inbox", "catalog"];

const MUTATIONS = ["track", "plan", "start", "assign", "request", "ack", "ownership", "settle"];

const MUTATION_BASE_KEYS = ["protocol", "version", "action", "storeVersion"];

const CATALOG_KEYS = ["protocol", "version", "storeSpace", "storeVersion", "trackedThings"];

const CATALOG_ROW_KEYS = ["id", "title", "desiredOutcome", "agent", "plan", "project", "task", "assignee", "assigneeTitle", "status"];

function fail_bang(message) {
  return (() => { throw new Error(message); })();
}

function object_value_p(value) {
  return ((_logical) => (_logical !== false && _logical != null ? ((typeof value === "object") && (!Array.isArray(value))) : _logical))(value);
}

function exact_keys_p(value, expected) {
  return (object_value_p(value) && (() => { const actual = Object.keys(value); return ((actual.length === expected.length) && expected.every((key) => actual.includes(key))); })());
}

function exact_action_bang(value) {
  const action = ((typeof value === "string") ? value : "");
  if ((!((_truthy) => _truthy !== false && _truthy != null)(ACTIONS.includes(action)))) {
    fail_bang("Bridge action must name one public tracked thing command");
  }
  return action;
}

function exact_executable_bang(value) {
  const candidate = ((typeof value === "string") ? value : "");
  if (((candidate === "") || ((!(candidate === candidate.trim())) || (!((_truthy) => _truthy !== false && _truthy != null)(isAbsolute(candidate)))))) {
    fail_bang("Bridge actions require the absolute checkout North executable");
  }
  return candidate;
}

function exact_text_bang(action, label, value) {
  const candidate = ((typeof value === "string") ? value : "");
  if (((_truthy) => _truthy !== false && _truthy != null)(((candidate === "") || ((!(candidate === candidate.trim())) || candidate.includes("\u0000"))))) {
    fail_bang($$bc$str("Bridge ", action, " requires exact ", label));
  }
  return candidate;
}

function optional_text_bang(action, label, value) {
  return ((value == null) ? null : exact_text_bang(action, label, value));
}

function exact_bool_bang(action, label, value) {
  if ((!(typeof value === "boolean"))) {
    fail_bang($$bc$str("Bridge ", action, " requires Boolean ", label));
  }
  return value;
}

function exact_nonnegative_int_bang(action, label, value) {
  if (((!Number.isSafeInteger(value)) || (value < 0))) {
    fail_bang($$bc$str("Bridge ", action, " requires nonnegative ", label));
  }
  return value;
}

function exact_argument_count_bang(action, arguments$, minimum, maximum) {
  if (((!Array.isArray(arguments$)) || ((arguments$.length < minimum) || (arguments$.length > maximum)))) {
    fail_bang($$bc$str("Bridge ", action, " has invalid explicit argument count"));
  }
  return arguments$;
}

function referent_action_request_bang(action_value, arguments$) {
  const action = exact_action_bang(action_value);
  const minimum = (((action === "catalog")) ? 0 : ((action === "ownership")) ? 1 : ((action === "track")) ? 2 : ((action === "plan")) ? 3 : ((action === "start")) ? 4 : ((action === "assign")) ? 3 : ((action === "request")) ? 3 : ((action === "ack")) ? 2 : ((action === "settle")) ? 5 : 1);
  const maximum = ((action === "request") ? 4 : minimum);
  const values = exact_argument_count_bang(action, arguments$, minimum, maximum);
  return (((action === "track")) ? {[$$bc$property_key($$bc$keyword("action"))]: action, [$$bc$property_key($$bc$keyword("title"))]: exact_text_bang(action, "title", values[0]), [$$bc$property_key($$bc$keyword("trackedBy"))]: exact_text_bang(action, "tracking actor", values[1])} : ((action === "plan")) ? {[$$bc$property_key($$bc$keyword("action"))]: action, [$$bc$property_key($$bc$keyword("trackedThing"))]: exact_text_bang(action, "tracked thing", values[0]), [$$bc$property_key($$bc$keyword("path"))]: exact_text_bang(action, "intended path", values[1]), [$$bc$property_key($$bc$keyword("endorsedBy"))]: exact_text_bang(action, "endorser", values[2])} : ((action === "start")) ? {[$$bc$property_key($$bc$keyword("action"))]: action, [$$bc$property_key($$bc$keyword("trackedThing"))]: exact_text_bang(action, "Plan identity", values[0]), [$$bc$property_key($$bc$keyword("revision"))]: exact_text_bang(action, "exact Plan revision", values[1]), [$$bc$property_key($$bc$keyword("authorizedBy"))]: exact_text_bang(action, "authorizer", values[2]), [$$bc$property_key($$bc$keyword("signature"))]: exact_text_bang(action, "exact signature", values[3])} : ((action === "assign")) ? {[$$bc$property_key($$bc$keyword("action"))]: action, [$$bc$property_key($$bc$keyword("trackedThing"))]: exact_text_bang(action, "tracked thing", values[0]), [$$bc$property_key($$bc$keyword("to"))]: exact_text_bang(action, "assignee", values[1]), [$$bc$property_key($$bc$keyword("assignedBy"))]: exact_text_bang(action, "assigner", values[2])} : ((action === "request")) ? (() => { const with_about_p = (values.length === 4); const offset = (with_about_p ? 1 : 0); return {[$$bc$property_key($$bc$keyword("action"))]: action, [$$bc$property_key($$bc$keyword("trackedThing"))]: (with_about_p ? exact_text_bang(action, "tracked thing", values[0]) : null), [$$bc$property_key($$bc$keyword("from"))]: exact_text_bang(action, "sender", values[offset]), [$$bc$property_key($$bc$keyword("to"))]: exact_text_bang(action, "recipient", values[(offset + 1)]), [$$bc$property_key($$bc$keyword("body"))]: exact_text_bang(action, "body", values[(offset + 2)])}; })() : ((action === "ack")) ? {[$$bc$property_key($$bc$keyword("action"))]: action, [$$bc$property_key($$bc$keyword("requestId"))]: exact_text_bang(action, "Request identity", values[0]), [$$bc$property_key($$bc$keyword("actor"))]: exact_text_bang(action, "receipt actor", values[1])} : ((action === "ownership")) ? {[$$bc$property_key($$bc$keyword("action"))]: action, [$$bc$property_key($$bc$keyword("transition"))]: exact_text_bang(action, "work-ownership-v1 transition JSON", values[0])} : ((action === "settle")) ? {[$$bc$property_key($$bc$keyword("action"))]: action, [$$bc$property_key($$bc$keyword("assignment"))]: exact_text_bang(action, "Assignment identity", values[0]), [$$bc$property_key($$bc$keyword("acceptedTransition"))]: exact_text_bang(action, "accepted ownership transition", values[1]), [$$bc$property_key($$bc$keyword("owner"))]: exact_text_bang(action, "owner", values[2]), [$$bc$property_key($$bc$keyword("outcome"))]: exact_text_bang(action, "outcome", values[3]), [$$bc$property_key($$bc$keyword("summary"))]: exact_text_bang(action, "summary", values[4])} : (((action === "show") || (action === "history"))) ? {[$$bc$property_key($$bc$keyword("action"))]: action, [$$bc$property_key($$bc$keyword("trackedThing"))]: exact_text_bang(action, "tracked thing", values[0])} : ((action === "inbox")) ? {[$$bc$property_key($$bc$keyword("action"))]: action, [$$bc$property_key($$bc$keyword("actor"))]: exact_text_bang(action, "inbox actor", values[0])} : {[$$bc$property_key($$bc$keyword("action"))]: action});
}

function referent_action_argv_bang(request, north_executable) {
  const north = exact_executable_bang(north_executable);
  const action = exact_action_bang(request.action);
  const prefix = [north, "work", action];
  return (((action === "track")) ? prefix.concat([exact_text_bang(action, "title", request.title), "--tracked-by", exact_text_bang(action, "tracking actor", request.trackedBy), "--json"]) : ((action === "plan")) ? prefix.concat([exact_text_bang(action, "tracked thing", request.trackedThing), "--path", exact_text_bang(action, "intended path", request.path), "--endorsed-by", exact_text_bang(action, "endorser", request.endorsedBy), "--json"]) : ((action === "start")) ? prefix.concat([exact_text_bang(action, "Plan identity", request.trackedThing), "--revision", exact_text_bang(action, "exact Plan revision", request.revision), "--authorized-by", exact_text_bang(action, "authorizer", request.authorizedBy), "--signature", exact_text_bang(action, "exact signature", request.signature), "--json"]) : ((action === "assign")) ? prefix.concat([exact_text_bang(action, "tracked thing", request.trackedThing), "--to", exact_text_bang(action, "assignee", request.to), "--assigned-by", exact_text_bang(action, "assigner", request.assignedBy), "--json"]) : ((action === "request")) ? (() => { const tracked_thing = optional_text_bang(action, "tracked thing", request.trackedThing); const base = (((_truthy) => _truthy !== false && _truthy != null)(tracked_thing) ? prefix.concat([tracked_thing]) : prefix); return base.concat(["--from", exact_text_bang(action, "sender", request.from), "--to", exact_text_bang(action, "recipient", request.to), "--body", exact_text_bang(action, "body", request.body), "--json"]); })() : ((action === "ack")) ? prefix.concat([exact_text_bang(action, "Request identity", request.requestId), "--by", exact_text_bang(action, "receipt actor", request.actor), "--json"]) : ((action === "ownership")) ? prefix.concat(["--transition", exact_text_bang(action, "work-ownership-v1 transition JSON", request.transition), "--json"]) : ((action === "settle")) ? prefix.concat([exact_text_bang(action, "Assignment identity", request.assignment), "--transition", exact_text_bang(action, "accepted ownership transition", request.acceptedTransition), "--by", exact_text_bang(action, "owner", request.owner), "--outcome", exact_text_bang(action, "outcome", request.outcome), "--summary", exact_text_bang(action, "summary", request.summary), "--json"]) : (((action === "show") || (action === "history"))) ? prefix.concat([exact_text_bang(action, "tracked thing", request.trackedThing), "--json"]) : ((action === "inbox")) ? prefix.concat([exact_text_bang(action, "inbox actor", request.actor), "--json"]) : prefix.concat(["--json"]));
}

function mutation_extra_keys(request) {
  const action = request.action;
  return (((action === "track")) ? ["referent"] : ((action === "plan")) ? ["referent", "revision"] : ((action === "start")) ? ["referent", "occurrence"] : ((action === "assign")) ? ["referent", "assignment"] : ((action === "request")) ? ((request.trackedThing == null) ? ["request"] : ["request", "referent"]) : ((action === "ack")) ? ["request", "ack"] : ((action === "ownership")) ? ["transition", "owner"] : ((action === "settle")) ? ["assignment", "acceptedTransition", "settlement", "outcome"] : []);
}

function required_receipt_text_bang(action, receipt, key) {
  exact_text_bang(action, key, (((key === "referent")) ? receipt.referent : ((key === "revision")) ? receipt.revision : ((key === "occurrence")) ? receipt.occurrence : ((key === "assignment")) ? receipt.assignment : ((key === "request")) ? receipt.request : ((key === "ack")) ? receipt.ack : ((key === "transition")) ? receipt.transition : ((key === "owner")) ? receipt.owner : ((key === "acceptedTransition")) ? receipt.acceptedTransition : ((key === "settlement")) ? receipt.settlement : receipt.outcome));
  return null;
}

function validate_mutation_bang(request, receipt) {
  const action = request.action;
  const extra = mutation_extra_keys(request);
  const expected = MUTATION_BASE_KEYS.concat(extra);
  if (((!exact_keys_p(receipt, expected)) || ((!(receipt.protocol === "north.semantic-receipt")) || ((!(receipt.version === 1)) || (!(receipt.action === action)))))) {
    fail_bang($$bc$str("North ", action, " returned an invalid committed receipt"));
  }
  exact_nonnegative_int_bang(action, "Store version", receipt.storeVersion);
  extra.forEach((key) => required_receipt_text_bang(action, receipt, key));
  return receipt;
}

function validate_read_bang(request, receipt) {
  const action = request.action;
  const expected = (((action === "show")) ? ["protocol", "version", "referent", "facts", "derived"] : ((action === "history")) ? ["protocol", "version", "referent", "occurrences"] : ["protocol", "version", "actor", "requests"]);
  const protocol = (((action === "show")) ? "north.semantic-view" : ((action === "history")) ? "north.semantic-history" : "north.semantic-inbox");
  const collection = (((action === "show")) ? receipt.facts : ((action === "history")) ? receipt.occurrences : receipt.requests);
  if (((!exact_keys_p(receipt, expected)) || ((!(receipt.protocol === protocol)) || ((!(receipt.version === 1)) || ((!Array.isArray(collection)) || ((action === "show") && (!Array.isArray(receipt.derived)))))))) {
    fail_bang($$bc$str("North ", action, " returned an invalid committed view"));
  }
  required_receipt_text_bang(action, receipt, ((action === "inbox") ? "actor" : "referent"));
  return receipt;
}

function validate_catalog_row_bang(row, previous_id) {
  if ((!exact_keys_p(row, CATALOG_ROW_KEYS))) {
    fail_bang("North catalog returned an invalid tracked thing row");
  }
  const id = exact_text_bang("catalog", "tracked thing identity", row.id);
  const title = exact_text_bang("catalog", "tracked thing title", row.title);
  const desired_outcome = optional_text_bang("catalog", "desired outcome", row.desiredOutcome);
  const agent = exact_bool_bang("catalog", "agent derivation", row.agent);
  const plan = exact_bool_bang("catalog", "Plan derivation", row.plan);
  const project = exact_bool_bang("catalog", "Project derivation", row.project);
  const task = exact_bool_bang("catalog", "Task derivation", row.task);
  const assignee = optional_text_bang("catalog", "assignee identity", row.assignee);
  const assignee_title = optional_text_bang("catalog", "assignee title", row.assigneeTitle);
  const status = optional_text_bang("catalog", "status", row.status);
  if (((_truthy) => _truthy !== false && _truthy != null)(((_logical) => (_logical !== false && _logical != null ? (previous_id.localeCompare(id) >= 0) : _logical))(previous_id))) {
    fail_bang("North catalog tracked things must be uniquely identity-ordered");
  }
  if ((!((((_truthy) => _truthy !== false && _truthy != null)(assignee) ? true : false) === (((_truthy) => _truthy !== false && _truthy != null)(assignee_title) ? true : false)))) {
    fail_bang("North catalog assignee identity and title must be null together");
  }
  if ((task && (!plan))) {
    fail_bang("North catalog Task derivation requires Plan on the same tracked thing");
  }
  if ((task && (!((_truthy) => _truthy !== false && _truthy != null)(assignee)))) {
    fail_bang("North catalog Task derivation requires a complete Assignment");
  }
  if ((project && (!plan))) {
    fail_bang("North catalog Project derivation requires Plan on the same tracked thing");
  }
  return TrackedThing(id, title, desired_outcome, agent, plan, project, task, assignee, assignee_title, status);
}

function validate_semantic_catalog_bang(value) {
  if ((!exact_keys_p(value, CATALOG_KEYS))) {
    fail_bang("North catalog returned an invalid snapshot envelope");
  }
  if (((!(value.protocol === "north.semantic-catalog")) || (!(value.version === 1)))) {
    fail_bang("North catalog returned an unsupported protocol");
  }
  const store_space = exact_text_bang("catalog", "Store space", value.storeSpace);
  const store_version = exact_nonnegative_int_bang("catalog", "Store version", value.storeVersion);
  const rows = value.trackedThings;
  if ((!Array.isArray(rows))) {
    fail_bang("North catalog trackedThings must be an identity-ordered array");
  }
  const tracked_things = [];
  const state = {[$$bc$property_key($$bc$keyword("previous"))]: null};
  rows.forEach((row) => { const item = validate_catalog_row_bang(row, state.previous);
tracked_things.push(item);
return (state.previous = trackedthing_id(item)); });
  return {[$$bc$property_key($$bc$keyword("storeSpace"))]: store_space, [$$bc$property_key($$bc$keyword("storeVersion"))]: store_version, [$$bc$property_key($$bc$keyword("trackedThings"))]: tracked_things};
}

function parse_json_object_bang(action, output) {
  if ((!(typeof output === "string"))) {
    fail_bang($$bc$str("North ", action, " returned no textual committed readback"));
  }
  const raw = output;
  const parsed = (() => { try {
    return JSON.parse(raw.trim());
  } catch (_catch_0) {
    switch ($$bd$catch_dispatch(_catch_0, [Error])) {
      case 0: {
        const __ = _catch_0;
        return fail_bang($$bc$str("North ", action, " returned non-JSON readback"));
        break;
      }
    }
  } })();
  if ((!object_value_p(parsed))) {
    fail_bang($$bc$str("North ", action, " returned a non-object readback"));
  }
  return parsed;
}

function validate_committed_readback_bang(request, output) {
  const action = exact_action_bang(request.action);
  const parsed = parse_json_object_bang(action, output);
  return (((action === "catalog")) ? validate_semantic_catalog_bang(parsed) : (((_truthy) => _truthy !== false && _truthy != null)(MUTATIONS.includes(action))) ? validate_mutation_bang(request, parsed) : validate_read_bang(request, parsed));
}

async function default_run_command_bang(argv) {
  const child = Bun.spawn({[$$bc$property_key($$bc$keyword("cmd"))]: argv, [$$bc$property_key($$bc$keyword("stdin"))]: "ignore", [$$bc$property_key($$bc$keyword("stdout"))]: "pipe", [$$bc$property_key($$bc$keyword("stderr"))]: "pipe"});
  const observed = await Promise.all([new Response(child.stdout).text(), new Response(child.stderr).text(), child.exited]);
  const stdout = observed[0];
  const stderr = observed[1];
  const exit_code = observed[2];
  if ((!(exit_code === 0))) {
    fail_bang(((stderr.trim() === "") ? $$bc$str("North action refused with exit ", exit_code) : stderr.trim()));
  }
  return stdout;
}

async function run_referent_action_bang(request, dependencies) {
  const executable = exact_executable_bang(dependencies.northExecutable);
  const argv = referent_action_argv_bang(request, executable);
  const candidate = ((_logical) => (_logical !== false && _logical != null ? _logical : default_run_command_bang))(dependencies.runCommand);
  if ((!(typeof candidate === "function"))) {
    fail_bang("Bridge action runner must be a Promise-returning function");
  }
  const output = await (candidate)(argv);
  return validate_committed_readback_bang(request, output);
}

function semantic_action_result_text_bang(request, result) {
  const action = request.action;
  return (((action === "catalog")) ? $$bc$str("catalog snapshot ", result.storeSpace, "@", result.storeVersion) : ((action === "track")) ? $$bc$str("tracked thing committed: ", result.referent) : ((action === "plan")) ? $$bc$str("Plan committed: ", result.referent) : ((action === "start")) ? $$bc$str("Project authorized: ", result.referent) : ((action === "assign")) ? $$bc$str("Assignment committed for ", result.referent) : ((action === "request")) ? $$bc$str("Request committed: ", result.request) : ((action === "ack")) ? $$bc$str("ACK committed: ", result.ack) : ((action === "ownership")) ? $$bc$str("ownership transition committed: ", result.transition) : ((action === "settle")) ? $$bc$str("Settlement committed: ", result.settlement) : ((action === "show")) ? "tracked thing view loaded" : ((action === "history")) ? "tracked thing history loaded" : "inbox loaded");
}

export { referent_action_argv_bang as "referent-action-argv!" };
export { referent_action_request_bang as "referent-action-request!" };
export { run_referent_action_bang as "run-referent-action!" };
export { semantic_action_result_text_bang as "semantic-action-result-text!" };
export { validate_committed_readback_bang as "validate-committed-readback!" };
export { validate_semantic_catalog_bang as "validate-semantic-catalog!" };
