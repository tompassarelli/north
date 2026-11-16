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

    assert_eq!(parse(&reading("0 96.0 24 12")).unwrap().refusal("whiterabbit"), None, "agent.slice throttled at its cap with the seat's synced writes quick still takes a session at load 96");
    assert_eq!(parse(&reading("0 4.0 24 6200")).unwrap().refusal("whiterabbit").as_deref(), Some("whiterabbit skipped: a synced write of its seat took 6.2 s within the last minute"), "a seat stall past the trip time keeps a session off at any load");
    assert_eq!(parse(&reading("0 56.19 20")).unwrap().refusal("gex45").as_deref(), Some("gex45 skipped: load 56 on 20 threads"));
}
