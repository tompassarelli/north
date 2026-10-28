use serde_json::Value;

use super::unleased::summarize;

const SAMPLE: &str = r#"{"at":"2026-10-28T20:00:00.000Z","intervalSeconds":9.6,"cgroups":[{"cgroup":"agent.slice","cores":9.46},{"cgroup":"agent.slice/agent-capacity.slice","cores":1.5},{"cgroup":"agent.slice/builder.scope","cores":5.123},{"cgroup":"agent.slice/builder.scope/inner.scope","cores":4.994},{"cgroup":"session.slice","cores":3},{"cgroup":"user.slice/other.scope","cores":2.4},{"cgroup":"user.slice/small.scope","cores":0.5}],"top":[{"cgroup":"agent.slice/builder.scope/inner.scope","pid":11,"cores":4.2,"command":"cargo build"},{"cgroup":"user.slice/other.scope","pid":12,"cores":1.1,"command":"rustc a"},{"cgroup":"user.slice/other.scope","pid":13,"cores":0.9,"command":"rustc b"},{"cgroup":"user.slice/other.scope","pid":14,"cores":0.5,"command":"rustc c"},{"cgroup":"user.slice/other.scope","pid":15,"cores":0.4,"command":"rustc d"},{"cgroup":"agent.slice/s.scope","pid":21,"cores":0.7,"command":"/usr/bin/claude --resume 0123456789abcdef0123456789abcdef"},{"cgroup":"agent.slice/s.scope","pid":22,"cores":0.3,"command":"codex resume 01234567-89ab-cdef-0123-456789abcdef"},{"cgroup":"agent.slice/s.scope","pid":23,"cores":0.2,"command":"claude --resume 0123456789abcdef0123456789abcdef"},{"cgroup":"agent.slice/s.scope","pid":24,"cores":0.6,"command":"claude --model x"}]}"#;

const OLD_JS_OUTPUT: &str = r#"{"heavy":[{"cgroup":"agent.slice/builder.scope/inner.scope","cores":4.99,"top":[{"pid":11,"cores":4.2,"command":"cargo build"}]},{"cgroup":"user.slice/other.scope","cores":2.4,"top":[{"pid":12,"cores":1.1,"command":"rustc a"},{"pid":13,"cores":0.9,"command":"rustc b"},{"pid":14,"cores":0.5,"command":"rustc c"}]}],"sessions":[{"pid":21,"session":"0123456789abcdef0123456789abcdef","cores":0.7,"command":"/usr/bin/claude --resume 0123456789abcdef0123456789abcdef"},{"pid":22,"session":"01234567-89ab-cdef-0123-456789abcdef","cores":0.3,"command":"codex resume 01234567-89ab-cdef-0123-456789abcdef"},{"pid":24,"session":null,"cores":0.6,"command":"claude --model x"}],"agentResidualCores":2.97,"cores":10.36,"windowSeconds":10}"#;

fn parse(text: &str) -> Value {
    serde_json::from_str(text).unwrap()
}

#[test]
fn the_unleased_sample_reads_as_the_old_host_script_read_it() {
    let at_sample_plus_five_seconds = 1_793_217_605_000.0;
    assert_eq!(summarize(&parse(SAMPLE), at_sample_plus_five_seconds), Some(parse(OLD_JS_OUTPUT)));
}

#[test]
fn a_sample_older_than_ninety_seconds_reads_as_nothing() {
    assert_eq!(summarize(&parse(SAMPLE), 1_793_217_600_000.0 + 90_001.0), None);
    assert!(summarize(&parse(SAMPLE), 1_793_217_600_000.0 + 90_000.0).is_some());
}
