#![cfg(test)]

use super::saturation::parse;

fn reading(backlog: &str) -> String {
    format!("65536000\n40000000\n/dev/sda3 629145600 156237824 441450496 27% /\n#backlog\n{backlog}\n#token\n")
}

// KEEP(bug): a host loaded by capped land-check leases takes a session while the sessions beside them barely stall; regression of 2026-11-08 10:00 AM, when gex45 read load 56 on 20 threads with its sessions stalled 0.14 to 0.35% and six north#892 workers waited for placement
#[test]
fn a_host_loaded_by_capped_leases_takes_a_session_while_its_unleased_agent_work_barely_stalls() {
    assert_eq!(parse(&reading("0 56.19 20 35")).unwrap().refusal("gex45"), None);
    assert_eq!(parse(&reading("0 56.19 20 1500")).unwrap().refusal("gex45").as_deref(), Some("gex45 skipped: load 56 on 20 threads with unleased agent work stalled 15% of the time"));
    assert_eq!(parse(&reading("0 56.19 20")).unwrap().refusal("gex45").as_deref(), Some("gex45 skipped: load 56 on 20 threads"));
}
