//! One reading of the fleet's lag: the objects `--json` prints, a sample for the scoreboard and one page to the
//! proxy per episode in which a process stays more than 5 minutes behind.
use super::behind;
use super::samples::{self, Sample};
use crate::runtime::layout::{self, Layout};
use crate::runtime::pin;
use crate::runtime::settle;
use crate::runtime::update;
use serde_json::Value;
use std::path::Path;

const KEY: &str = "runtime-lag";
const PAGE_AFTER_S: i64 = 300;

pub(super) fn objects(layout: &Layout, probed: &[Value]) -> Vec<Value> {
    let checkout = update::checkout();
    behind::objects(&super::ask::seen(layout, probed), &super::ask::landings(layout), pin::now(), |landed, runtime| landed == runtime || settle::ancestor(&checkout, landed, runtime))
}

/// Reads the lag, records its worst and pages the proxy once when an episode of lag over 5 minutes begins; it ends
/// when no process is over it.
pub(crate) fn watch(layout: &Layout, probed: &[Value]) {
    conclude(&layout.state, &objects(layout, probed), pin::now(), |text| update::page_proxy(KEY, text));
}

pub(super) fn conclude(state: &Path, objects: &[Value], at: i64, page: impl FnOnce(&str) -> bool) {
    let Some(worst) = behind::worst(objects) else { return };
    let over = objects.iter().filter(|object| object["seconds"].as_i64().is_some_and(|seconds| seconds > PAGE_AFTER_S)).count();
    let paged_before = samples::read(state).last().is_some_and(|last| last.paged);
    let paged = over > 0 && (paged_before || page(&text(worst, over)));
    if let Some(sample) = Sample::of(worst, at, paged) {
        samples::record(state, &sample);
    }
}

fn text(worst: &Value, over: usize) -> String {
    let runtime = worst["runtime"].as_str().map_or_else(|| "an older runtime".to_owned(), |sha| layout::short(sha).to_owned());
    format!(
        "north runtime lag: {} on {} runs {runtime}, {}s behind the newest landing that changed a runtime input; {over} process{} over {PAGE_AFTER_S}s; north runtime lag --json lists them",
        worst["process"].as_str().unwrap_or_default(),
        worst["host"].as_str().unwrap_or_default(),
        worst["seconds"],
        if over == 1 { "" } else { "es" },
    )
}
