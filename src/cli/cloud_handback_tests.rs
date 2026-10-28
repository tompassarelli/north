use super::*;

const EVENTS: &str = include_str!("../fixtures/cloud/handback-events.jsonl");
const MBOX: &str = include_str!("../fixtures/cloud/handback.mbox");

fn events() -> Vec<Value> {
    EVENTS
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

fn result(text: &str) -> Value {
    serde_json::json!({"type": "user", "message": {"content": [{"type": "tool_result", "content": text}]}})
}

fn encode(patch: &[u8]) -> String {
    let compressed = through("gzip", &["-9", "-n", "-c"], patch, "gzip").unwrap();
    String::from_utf8(through("base64", &["-w", "76"], &compressed, "base64").unwrap()).unwrap()
}

fn handback(patch: &[u8]) -> Vec<Value> {
    vec![result(&format!(
        "BEGIN-HANDBACK part 1/1\n{}END-HANDBACK part 1/1\n",
        encode(patch)
    ))]
}

fn without_part(index: usize) -> Vec<Value> {
    let marker = format!("BEGIN-HANDBACK part {index}/3");
    events()
        .into_iter()
        .filter(|event| !event.to_string().contains(&marker))
        .collect()
}

#[test]
fn collect_round_trips_the_latest_multi_part_handback_and_ignores_a_stale_one() {
    assert_eq!(collect(&events()).unwrap(), MBOX.as_bytes());
}

#[test]
fn collect_joins_parts_in_part_order_when_the_session_sent_them_out_of_order() {
    let mut shuffled = events();
    shuffled.swap(3, 5);
    assert_eq!(collect(&shuffled).unwrap(), MBOX.as_bytes());
}

#[test]
fn collect_refuses_a_handback_missing_any_one_part() {
    for index in 1..=3 {
        let error = collect(&without_part(index)).unwrap_err();
        assert!(
            error.contains(&format!("part {index} of 3 is missing")),
            "{error}"
        );
    }
}

#[test]
fn collect_refuses_a_failed_decode() {
    let damaged = |from: &str, to: &str| {
        events()
            .into_iter()
            .map(|event| serde_json::from_str(&event.to_string().replace(from, to)).unwrap())
            .collect::<Vec<Value>>()
    };
    for case in [
        damaged("H4sIAAAAAAAC", "H4sIAAA!AAAC"),
        damaged("H4sIAAAAAAAC", "AAAAAAAAAAAA"),
    ] {
        assert!(collect(&case).unwrap_err().contains("corrupt"));
    }
}

#[test]
fn collect_refuses_output_that_is_not_a_patch_and_names_an_empty_one() {
    assert!(
        collect(&handback(b"Subject: nothing\n"))
            .unwrap_err()
            .contains("'From '")
    );
    assert!(
        collect(&handback(b""))
            .unwrap_err()
            .contains("committed nothing")
    );
}

#[test]
fn collect_refuses_a_transcript_without_a_handback() {
    assert!(
        collect(&[result("ok")])
            .unwrap_err()
            .contains("no BEGIN-HANDBACK")
    );
}

#[test]
fn run_writes_the_mbox_and_counts_its_commits() {
    let dir = tempfile::tempdir().unwrap();
    let (transcript, out) = (dir.path().join("events.jsonl"), dir.path().join("out.mbox"));
    std::fs::write(&transcript, EVENTS).unwrap();
    let mut report = Vec::new();
    run(&transcript, &out, &mut report).unwrap();
    assert_eq!(std::fs::read_to_string(&out).unwrap(), MBOX);
    assert_eq!(
        String::from_utf8(report).unwrap(),
        format!("1 commits in {}\n", out.display())
    );
    std::fs::write(&transcript, "").unwrap();
    assert!(run(&transcript, &out, &mut Vec::new()).is_err());
}

fn split_into_parts(patch: &[u8], total: usize) -> Vec<Value> {
    let encoded = encode(patch);
    let lines = encoded.lines().collect::<Vec<_>>();
    let size = lines.len().div_ceil(total);
    lines
        .chunks(size)
        .enumerate()
        .map(|(position, chunk)| {
            let index = position + 1;
            result(&format!(
                "BEGIN-HANDBACK part {index}/{total}\n{}\nEND-HANDBACK part {index}/{total}\n",
                chunk.join("\n")
            ))
        })
        .collect()
}

#[test]
fn collect_ignores_an_earlier_handback_with_the_same_part_count() {
    let stale = format!(
        "From {} Mon Sep 17 00:00:00 2001\nSubject: [PATCH] stale\n\n{}",
        "0".repeat(40),
        (0..400)
            .map(|n| format!("stale line {n} {}\n", n * n * 7919 % 10007))
            .collect::<String>()
    );
    let mut transcript = split_into_parts(stale.as_bytes(), 3);
    assert_eq!(transcript.len(), 3);
    transcript.extend(events());
    assert_eq!(collect(&transcript).unwrap(), MBOX.as_bytes());
}

#[test]
fn collect_accepts_a_part_the_session_sent_twice() {
    let mut resent = events();
    resent.insert(5, resent[4].clone());
    assert_eq!(collect(&resent).unwrap(), MBOX.as_bytes());
}
