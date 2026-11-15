#![cfg(test)]

use super::saturation::parse;

fn reading(backlog: &str) -> String {
    format!("65536000\n40000000\n/dev/sda3 629145600 156237824 441450496 27% /\n#backlog\n{backlog}\n#token\n")
}

#[test]
fn per_machine_admission_reads_memory_and_cpu_pressure_not_a_session_count() {
    let roomy = parse(&reading("0 4.0 20 35")).unwrap();
    assert_eq!(roomy.refusal("gex45"), None);
    assert!(roomy.session_room() >= 40, "{} sessions fit in 40 GB available of 64 GB", roomy.session_room());

    let squeezed = parse("65536000\n3000000\n/dev/sda3 629145600 156237824 441450496 27% /\n#backlog\n0 4.0 20 35\n#token\n").unwrap();
    assert_eq!(squeezed.session_room(), 0);
    assert!(squeezed.refusal("gex45").is_some_and(|why| why.contains("memory available")));

    let cgroup = tempfile::tempdir().unwrap();
    let psi = |slice: &str, file: &str, some: f64| {
        std::fs::create_dir_all(cgroup.path().join(slice)).unwrap();
        std::fs::write(cgroup.path().join(slice).join(file), format!("some avg10={some:.2} avg60=0.00 avg300=0.00 total=1\nfull avg10=0.00 avg60=0.00 avg300=0.00 total=1\n")).unwrap();
    };
    psi("agent.slice", "cpu.pressure", 84.76);
    std::fs::write(cgroup.path().join("agent.slice/cpu.stat"), "usage_usec 649257736191\nnr_periods 52\nnr_throttled 52\nthrottled_usec 43330000\n").unwrap();
    for slice in ["app.slice", "session.slice"] {
        psi(slice, "cpu.pressure", 0.0);
        psi(slice, "io.pressure", 0.0);
    }
    let stall = super::agent_load::SeatPressure::read(cgroup.path()).stall();
    assert_eq!(parse(&reading(&format!("0 96.0 24 {stall}"))).unwrap().refusal("whiterabbit"), None, "agent.slice throttled at its cap with the seat unpressed still takes a session at load 96");
    assert_eq!(parse(&reading("0 4.0 24 5149")).unwrap().refusal("whiterabbit").as_deref(), Some("whiterabbit skipped: its seat stalled 51% of the time"), "app.slice disk pressure 51% keeps a session off at any load");
    assert_eq!(parse(&reading("0 56.19 20")).unwrap().refusal("gex45").as_deref(), Some("gex45 skipped: load 56 on 20 threads"));
}
