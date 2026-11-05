#![cfg(test)]

use serde_json::Value;

use super::unleased::summarize;

fn parse(text: &str) -> Value {
    serde_json::from_str(text).unwrap()
}

// KEEP(bug): a leased build in agent-batch.slice is counted once, not again as unleased load; regression of the 2026-11-05 laptop double count (north#259, commit bb886f8eb)
#[test]
fn a_leased_build_in_the_batch_slice_is_not_also_counted_as_unleased_load() {
    let sample = r#"{"at":"2026-11-05T05:40:00.000Z","intervalSeconds":30,"cgroups":[{"cgroup":"agent.slice","cores":6.0},{"cgroup":"agent.slice/agent-batch.slice","cores":4.56},{"cgroup":"agent.slice/agent-batch.slice/agent-capacity-db2232aaf51142e3be81d9fb85e9922f.scope","cores":2.31},{"cgroup":"agent.slice/agent-batch.slice/agent-capacity-8e3dba8298e34fb0afb7c89c553bd39c.scope","cores":2.25},{"cgroup":"agent.slice/builder.scope","cores":1.2}],"top":[]}"#;
    let summary = summarize(&parse(sample), 1_793_857_200_000.0 + 5_000.0).expect("fresh sample");
    let heavy: Vec<&str> = summary["heavy"].as_array().unwrap().iter().filter_map(|entry| entry["cgroup"].as_str()).collect();
    assert_eq!(heavy, ["agent.slice/builder.scope"], "the two lease scopes the 2026-11-05 laptop counted twice are left to their leases");
    assert_eq!(summary["agentResidualCores"].as_f64(), Some(0.24));
    assert_eq!(summary["cores"].as_f64(), Some(1.44));
}
