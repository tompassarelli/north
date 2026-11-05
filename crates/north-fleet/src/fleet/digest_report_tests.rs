use super::*;

fn session(name: &str, status: &str, updated: &str, tasks: &[&str]) -> SessionRow {
    SessionRow {
        name: name.into(),
        host: "laptop".into(),
        status: status.into(),
        created: "2026-11-05T01:00:00Z".into(),
        updated: updated.into(),
        provider: "claude".into(),
        tasks: tasks.iter().map(|task| (*task).to_owned()).collect(),
        ..SessionRow::default()
    }
}

#[test]
fn the_digest_prints_landed_in_flight_open_gates_and_stalls_in_that_order() {
    let now = Moment::parse("2026-11-05T12:00:00Z").unwrap();
    let landed = [Landing { task_ref: "north#522".into(), sha: "0123456789abcdef".into(), wall_ms: None, landed_ms: 1_000_000 }];
    let sessions = [
        session("worker-a", "working", "2026-11-05T11:50:00Z", &["north#522"]),
        session("worker-b", "working", "2026-11-05T08:00:00Z", &[]),
        session("worker-c", "exited", "2026-11-05T07:00:00Z", &[]),
    ];
    let gates = [GateLine { label: "laptop:7".into(), session: "worker-a".into(), question: "Which cut?".into(), recommended: "1. the small one".into(), default_at_ms: 4_000_000 }];
    let text = render(&Facts { now, now_ms: 4_600_000, since_label: "24h", landed: &landed, sessions: &sessions, gates: &gates });
    assert_eq!(
        text,
        "Landed (last 24h): 1\n  north#522 012345678 1h 00m ago\n\
         In flight: 2\n  worker-a (laptop, claude) working: north#522\n  worker-b (laptop, claude) working: no task\n\
         Open gates: 1\n  laptop:7 worker-a: Which cut? Recommended: 1. the small one\n\
         Stalls: 2\n  worker-b (laptop) working with no update for 4h 00m\n  gate laptop:7 is past its default time by 10m\n"
    );
}

#[test]
fn an_empty_fleet_prints_all_four_sections_with_zero() {
    let now = Moment::parse("2026-11-05T12:00:00Z").unwrap();
    let text = render(&Facts { now, now_ms: 0, since_label: "24h", landed: &[], sessions: &[], gates: &[] });
    assert_eq!(text, "Landed (last 24h): 0\nIn flight: 0\nOpen gates: 0\nStalls: 0\n");
}
