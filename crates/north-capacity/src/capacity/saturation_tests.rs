#![cfg(test)]

use super::saturation::parse;

fn reading(backlog: &str) -> String {
    format!("65536000\n40000000\n/dev/sda3 629145600 156237824 441450496 27% /\n#backlog\n{backlog}\n#token\n")
}

#[test]
fn a_host_loaded_by_capped_leases_takes_a_session_while_its_unleased_agent_work_barely_stalls() {
    assert_eq!(parse(&reading("0 56.19 20 35")).unwrap().refusal("gex45"), None);
    assert_eq!(parse(&reading("0 56.19 20 1500")).unwrap().refusal("gex45").as_deref(), Some("gex45 skipped: load 56 on 20 threads with unleased agent work stalled 15% of the time"));
    assert_eq!(parse(&reading("0 56.19 20")).unwrap().refusal("gex45").as_deref(), Some("gex45 skipped: load 56 on 20 threads"));
}

#[test]
fn per_machine_admission_reads_memory_and_cpu_pressure_not_a_session_count() {
    let roomy = parse(&reading("0 4.0 20 35")).unwrap();
    assert_eq!(roomy.refusal("gex45"), None);
    assert!(roomy.session_room() >= 40, "{} sessions fit in 40 GB available of 64 GB", roomy.session_room());

    let squeezed = parse("65536000\n3000000\n/dev/sda3 629145600 156237824 441450496 27% /\n#backlog\n0 4.0 20 35\n#token\n").unwrap();
    assert_eq!(squeezed.session_room(), 0);
    assert!(squeezed.refusal("gex45").is_some_and(|why| why.contains("memory available")));

    let stalled = parse(&reading("0 56.19 20 1500")).unwrap();
    assert!(stalled.refusal("gex45").is_some_and(|why| why.contains("stalled 15%")));
}

#[test]
fn a_host_under_its_disk_bound_takes_nothing_and_takes_work_again_once_over_it() {
    let free = |gib: u64| parse(&format!("65536000\n40000000\n/dev/sda3 981467136 0 {} 90% /\n#backlog\n0 4.0 20 35\n#token\n", gib * 1024 * 1024)).unwrap().bounded(Some(100.0));
    let under = free(95);
    assert_eq!(under.refusal("gex45").as_deref(), Some("gex45 skipped: 95.0 GiB free on the root disk, under its 100 GiB disk bound"));
    assert_eq!(under.session_room(), 0);
    let over = free(120);
    assert_eq!(over.refusal("gex45"), None);
    assert_eq!(over.session_room(), 2);
}
