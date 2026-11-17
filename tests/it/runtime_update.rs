use std::path::Path;

#[path = "runtime_update/harness.rs"]
mod harness;

use harness::*;

fn skewed() -> Host {
    let host = Host::new();
    host.built(&host.target);
    host.built(PREVIOUS);
    host.select(PREVIOUS);
    host
}

#[test]
fn a_failed_sync_keeps_the_new_runtime_and_alerts_once_per_cause_without_the_secret() {
    for launched in [true, false] {
        let host = skewed();
        if !launched {
            host.lose_agents();
        }
        let output = host.update(42);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(output.status.success(), "{stderr}");
        assert_eq!(host.current(), Path::new("runtimes").join(&host.target));
        let expected = format!("agents sync exited {}", if launched { 42 } else { 127 });
        assert_eq!(stderr.matches("the harness generation is not published").count(), 1, "{stderr}");
        assert!(stderr.contains("north runtime update: ") && stderr.contains(&expected), "{stderr}");
        assert!(!stderr.contains(SECRET), "{stderr}");
        assert!(!String::from_utf8_lossy(&output.stdout).contains(SECRET));
        let again = host.update(42);
        let again_stderr = String::from_utf8_lossy(&again.stderr);
        assert!(again.status.success(), "{again_stderr}");
        assert!(!again_stderr.contains("the harness generation is not published"), "the same cause alerts once: {again_stderr}");
    }
}
