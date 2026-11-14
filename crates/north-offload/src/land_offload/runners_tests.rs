use super::*;

const MESH: &str = "[hosts.cpx62]\nrunners = 12\noffload_builds = true\n\n[hosts.cpx62b]\nruns_no_checks = true\nrunners = 12\noffload_builds = false\n\n[hosts.gex45]\nrunners = 4\noffload_builds = true\n";

#[test]
fn closure_keeps_runners_on_a_closed_host_while_the_other_open_hosts_fall_under_the_floor() {
    let cfg: DocumentMut = MESH.parse().unwrap();
    let refused = closure(&cfg, "cpx62b", |name| name != "cpx62").expect_err("on 2026-11-14 closing cpx62 and cpx62b took 24 of 28 runners off and landings stalled");
    assert!(refused.contains("leave 4 live runners") && refused.contains("floor of 16"), "{refused}");
    assert_eq!(closure(&cfg, "cpx62b", |_| true), Ok(()), "every host up leaves 16, the floor");
    assert!(refused_closures(&cfg).is_empty());
    let both: DocumentMut = MESH.replacen("offload_builds = true", "offload_builds = false", 1).parse().unwrap();
    assert_eq!(refused_closures(&both).len(), 2, "a second closure leaves 4 to each");
    use north_base::platform::orchestration::builds_for_sessions;
    assert!(!builds_for_sessions(&cfg, "cpx62b") && builds_for_sessions(&cfg, "cpx62"), "cpx62b's w5 entry: a session placed there before the close ran cargo test -p north-replay as a local heavy command twice, 7:50 to 8:05 PM on 2026-11-14");
}
