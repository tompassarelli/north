use super::*;

fn denied(command: &str) -> bool {
    !decide(command).is_empty()
}

#[test]
fn a_self_matching_pgrep_is_refused_only_when_its_output_is_consumed() {
    for command in [
        "pgrep -af 'cargo test' | head -3",
        "P=$(pgrep -f 'wisp.ts'); ls /proc/$P",
        "git log -1; pgrep -fa 'frontier oracle' | cut -c1-120; ls",
    ] {
        assert!(!denied(command), "{command}");
    }
    for command in [
        "until ! pgrep -f 'cargo test' >/dev/null; do sleep 1; done",
        "timeout 110 tail --pid=$(pgrep -f 'bash bin/safe-push.test.sh' | head -1) -f /dev/null",
        "for p in $(pgrep -f 'cargo test'); do kill $p; done",
        "pkill -f 'cargo build'; pgrep -fa 'safe-push' | head",
        "pkill -f 'cargo build'",
    ] {
        assert!(denied(command), "{command}");
    }
    let script = |wait: &str, file: &str| format!("cat > /tmp/{file}.sh <<'EOF'\n{wait}\ncd /tmp\necho done\nEOF\nbash /tmp/check.sh > /tmp/check.log 2>&1; cat /tmp/check.log");
    let waiting = "while ! [ -f /tmp/w/mobile_sam.pt ] || pgrep -f \"pip install\" >/dev/null; do sleep 5; done";
    let hits = decide(&script(waiting, "check"));
    assert!(matches!(&hits[..], [Hit::SelfMatch { tool: "pgrep", pattern }] if pattern == "pip install"), "{hits:?}");
    assert!(!denied(&script("pgrep -fa 'pip install' | head -3", "check")));
    assert!(!denied(&script(waiting, "other")));
}
