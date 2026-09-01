import { "settle-bridge-launches!" as settle_bridge_launches_bang } from "../bridge/app.js";
import { "handle-managed-app-launch-signal!" as handle_managed_app_launch_signal_bang, "request-managed-app-launch-termination!" as request_managed_app_launch_termination_bang, "settle-managed-app-launch-before-start!" as settle_managed_app_launch_before_start_bang } from "../bridge/cli.js";
import { js_obj as $$bh$js_obj } from '../../beagle/host.js';

function assert_bang(condition, message) {
  if ((!condition)) {
    return (() => { throw new Error(message); })();
  }
}

async function main_bang() {
  const early_lease = $$bh$js_obj("held", true);
  const early_termination_calls = $$bh$js_obj("count", 0);
  const early_state = $$bh$js_obj("shutdownRequested", false, "termination", null, "managed", null, "terminate", (__) => { (early_termination_calls.count = (early_termination_calls.count + 1));
return Promise.resolve(null); });
  const early_managed = $$bh$js_obj("proveUnsent", (reason) => { assert_bang((reason === "process-signal-before-launch"), "pre-launch shutdown used the wrong settlement reason");
(early_lease.held = false);
return Promise.resolve(null); });
  const __early_signal = handle_managed_app_launch_signal_bang(early_state);
  const early_settled = await settle_managed_app_launch_before_start_bang(early_managed, early_state);
  const lease = $$bh$js_obj("held", true);
  const child_exit = Promise.withResolvers();
  const resolve_child_exit = child_exit.resolve;
  const termination_state = $$bh$js_obj("shutdownRequested", false, "termination", null);
  const managed = $$bh$js_obj("executionId", "execution:bridge-shutdown-fixture");
  const signals = [];
  const terminate_bang = (execution_id) => { assert_bang((execution_id === "execution:bridge-shutdown-fixture"), "termination targeted the wrong execution");
(lease.held = false);
resolve_child_exit(0);
return Promise.resolve(null); };
  const child = $$bh$js_obj("exited", child_exit.promise, "kill", (signal) => { signals.push(signal);
return request_managed_app_launch_termination_bang(managed, termination_state, terminate_bang); });
  const runtime = $$bh$js_obj("bridgeLaunchChildren", new Set([child]));
  assert_bang((((_truthy) => _truthy !== false && _truthy != null)(early_state.shutdownRequested) ? true : false), "pre-reservation signal was not remembered");
  assert_bang((early_termination_calls.count === 0), "pre-reservation signal tried to terminate an unknown execution");
  assert_bang(early_settled, "pre-launch shutdown did not settle the reserved attempt");
  assert_bang((!((_truthy) => _truthy !== false && _truthy != null)(early_lease.held)), "pre-launch shutdown left its delivery lease held");
  await settle_bridge_launches_bang(runtime);
  assert_bang(((signals.length === 1) && (signals[0] === "SIGTERM")), "Bridge shutdown did not signal its exact managed launch once");
  assert_bang((((_truthy) => _truthy !== false && _truthy != null)(termination_state.shutdownRequested) ? true : false), "managed launch did not enter signal-driven shutdown");
  assert_bang((!((_truthy) => _truthy !== false && _truthy != null)(lease.held)), "delivery lease remained held after Bridge shutdown");
  (lease.held = true);
  assert_bang((((_truthy) => _truthy !== false && _truthy != null)(lease.held) ? true : false), "delivery lease could not be reacquired immediately");
  return 0;
}

if (import.meta.main) {
  main_bang().then((code) => (process.exitCode = code));
}
