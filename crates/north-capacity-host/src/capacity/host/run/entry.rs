//! `north capacity host run`: refusing a decision, nesting under a parent lease, and supervising an admitted run.

use std::path::Path;

use serde_json::Value;

use super::super::json::{Obj, text};
use super::super::machine::read_signals;
use super::super::offering::{Admitted, HELD_HOST, NO_BUILD_HOST, SESSION_BUILDS_OFF};
use super::outcome::run_admitted;
use super::super::policy::batch_class;
use super::super::quiet_holds::quiet_owners;
use super::super::request::{Create, Request};
use super::scope::{north_command, timeout_text};
use super::super::state::{active_profile, exact_lease};
use super::super::systemd::apply_profile;
use super::super::terminations::Terminations;
use super::waiting::{admit, nested_parent};
use super::super::{fail, print_to};
use crate::capacity::dev_shell;
use crate::capacity::effects::Host;

fn refusal(admitted: &Obj) -> Option<String> {
    let view = admitted.value();
    if text(&view, "reason") == Some(NO_BUILD_HOST) {
        return Some(format!("refused: this host does not compile for fleet sessions and no build host took the job ({}); the detail names what to change or wait for", text(&view, "detail").unwrap_or_default()));
    }
    if text(&view, "reason") == Some(SESSION_BUILDS_OFF) {
        return Some(format!("refused: {} keeps fleet sessions' Rust compiles off this host and no build host took the job; run it on a build host with `north capacity offload run --host <build host> --base origin/main --head HEAD --timeout SECS -- \"COMMAND\"`", text(&view, "detail").unwrap_or_default()));
    }
    if text(&view, "reason") == Some(HELD_HOST) {
        return Some(format!("refused: this host is under a hold that keeps heavy work off it, and the offload declined this build ({}), so it never runs here; run it on a build host with `north capacity offload run --host <build host> --base origin/main --head HEAD --timeout SECS -- \"COMMAND\"`, which ships the tree and runs the command there", text(&view, "detail").unwrap_or_default()));
    }
    if text(&view, "reason") == Some("PAIR_HELD") {
        let holder = view.get("holder").map_or_else(String::new, |holder| holder.as_str().map_or_else(|| holder.to_string(), str::to_owned));
        return Some(format!("refused: pair {} is held by {holder}; free pair: {}", view["pair"], view["freePair"]));
    }
    if text(&view, "decision") == Some("TIMED_OUT") {
        return Some(format!("queued {}s, past its --timeout-seconds, never admitted: {}", view["queuedSeconds"], text(&view, "reason").unwrap_or("unknown")));
    }
    (text(&view, "decision") == Some("REFUSED")).then(|| {
        format!("refused after {}s: native evidence run {} is live; retry after it ends", view["queuedSeconds"], quiet_owners(&view).join(", "))
    })
}

pub fn run_scoped(root: &Path, request: &Request, owner: &str, timeout_seconds: Option<u64>, command: &[String], host: &Host) -> u8 {
    if request.name == "agent" {
        fail("run --class must be moderate, heavy, exclusive, gpu, critical, tom, runtime, or native", 2);
    }
    if let Some(parent) = batch_class(&request.name).then(|| nested_parent(root)).flatten() {
        print_to(&Obj::new().put("decision", "NESTED").put("parent", parent.as_str()).put("class", request.name.as_str()).put("owner", owner), true);
        let mut arguments = vec![exact_lease(root, &parent).to_string_lossy().into_owned(), timeout_text(timeout_seconds), "--".to_owned()];
        arguments.extend(dev_shell::enter(&north_command(), command));
        return crate::capacity::deadline::run(&arguments);
    }
    // The parent limit and weights must exist before any admitted command executes.
    let signals = read_signals();
    if !apply_profile(active_profile(root).1, signals.cores, signals.slice_cpus) {
        fail("cannot establish aggregate CPU limit and native weight", 75);
    }
    let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap_or_else(|error| fail(&error.to_string(), 1));
    let create = Create { kind: "run", owner: owner.to_owned(), timeout_seconds };
    let code = runtime.block_on(async {
        let mut terminations = Terminations::listen();
        let admitted = match admit(root, request, &create, command, host, &mut terminations).await {
            Admitted::Here(admitted) => admitted,
            Admitted::Offloaded(code) => return code,
        };
        super::super::admissions::record(owner, request, &admitted);
        print_to(&admitted, true);
        if let Some(message) = refusal(&admitted) {
            fail(&message, 75);
        }
        if admitted.get("decision").and_then(Value::as_str) != Some("RESERVED") {
            return 75;
        }
        super::super::priority::record_use(owner, request, &admitted, command);
        u8::try_from(run_admitted(root, request, owner, timeout_seconds, command, &admitted, &mut terminations).await & 0xff).unwrap_or(1)
    });
    runtime.shutdown_background();
    code
}
