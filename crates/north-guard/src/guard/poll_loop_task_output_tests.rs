#![cfg(test)]

use super::poll_loop_task_output::polls_task_output;

const OUTPUT: &str = "/tmp/claude-1000/-home-tom-code-north-main/6c1a/tasks/bq1x9.output";

#[test]
fn poll_loop_on_a_background_task_output_file_is_refused_in_each_loop_shape() {
    let refused = [
        format!("timeout 29 bash -c 'until test -s {OUTPUT}; do sleep 1; done'"),
        format!("until [ -s {OUTPUT} ]; do sleep 2; done; cat {OUTPUT}"),
        format!("while ! grep -q done {OUTPUT}; do sleep 3; done"),
        format!("for i in $(seq 1 5); do [ -s {OUTPUT} ] && break; sleep 5; done; tail -5 {OUTPUT}"),
    ];
    for command in refused {
        assert!(polls_task_output(&command), "{command}");
    }
}

#[test]
fn poll_loop_on_anything_else_is_not_the_task_output_refusal() {
    let allowed = [
        "until git fetch origin main; do sleep 2; done".to_owned(),
        "sleep 5".to_owned(),
        format!("tail -n 20 {OUTPUT}"),
        format!("sleep 5; cat {OUTPUT}"),
        "for f in a b; do cat $f; done; sleep 1".to_owned(),
    ];
    for command in allowed {
        assert!(!polls_task_output(&command), "{command}");
    }
}
