use super::*;

fn ticket(branch: &str, red_fix: Option<RedFix>) -> Ticket {
    Ticket {
        origin: "https://github.com/example/repo.git".into(),
        root: PathBuf::from("/lanes").join(branch),
        host: "laptop".into(),
        branch: branch.into(),
        head: "a".repeat(40),
        session: Some("worker-1".into()),
        queued_at: "2026-11-04T03:00:00Z".into(),
        red_fix,
        ..Default::default()
    }
}

#[test]
fn the_key_is_the_first_sixteen_hex_of_the_origin_hash_the_bash_lock_uses() {
    assert_eq!(key("https://github.com/tompassarelli/north.git").len(), 16);
    assert_eq!(key("https://github.com/tompassarelli/north.git"), "de2e0e8479546ba9");
    assert_eq!(key("https://github.com/tompassarelli/north"), "de2e0e8479546ba9");
    assert_eq!(key("https://github.com/tompassarelli/north/"), "de2e0e8479546ba9");
}

#[test]
fn red_fix_round_trips_as_an_issue_number_the_marker_string_or_null() {
    for (red_fix, json) in [(Some(RedFix::Issue(482)), "482"), (Some(RedFix::Marker), "\"marker\""), (None, "null")] {
        let text = serde_json::to_string(&ticket("lane", red_fix)).unwrap();
        assert!(text.contains(&format!("\"red_fix\":{json}")), "{text}");
        assert_eq!(serde_json::from_str::<Ticket>(&text).unwrap().red_fix, red_fix);
    }
    assert!(serde_json::from_str::<RedFix>("\"other\"").is_err());
}

#[test]
fn pending_orders_red_fix_tickets_first_then_oldest_and_decide_moves_a_ticket_to_done() {
    let state = tempfile::tempdir().unwrap();
    let queue = directory(state.path(), "origin");
    write(&queue.join(file_name(30, "late/red")), &ticket("late/red", Some(RedFix::Marker))).unwrap();
    write(&queue.join(file_name(10, "oldest")), &ticket("oldest", None)).unwrap();
    write(&queue.join(file_name(20, "newer")), &ticket("newer", None)).unwrap();
    fs::write(queue.join(".half.json.1.tmp"), "{").unwrap();

    let order: Vec<String> = pending(&queue).unwrap().into_iter().map(|(_, ticket)| ticket.branch).collect();
    assert_eq!(order, ["late/red", "oldest", "newer"]);

    let (path, first) = pending(&queue).unwrap().remove(0);
    let done = decide(&path, first, "landed abc".into()).unwrap();
    assert_eq!(read(&done).unwrap().verdict.as_deref(), Some("landed abc"));
    assert_eq!(pending(&queue).unwrap().len(), 2);
}
