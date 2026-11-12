use std::collections::BTreeMap;

use super::{adopt, body, clear, cleared, held, mark, names_low_credit, refresh, until};

const REFUSAL: &str = "HTTP 400: {\"type\":\"error\",\"error\":{\"type\":\"invalid_request_error\",\"message\":\"Your credit balance is too low to access the Anthropic API.\"}}";

#[test]
fn an_exhausted_mark_keeps_the_refusals_body_and_a_refusal_that_does_not_name_credit_writes_nothing()
 {
    let dir = tempfile::tempdir().unwrap();
    assert!(names_low_credit(REFUSAL));
    assert!(!names_low_credit("HTTP 400: invalid x-api-key"));

    assert!(
        mark(
            dir.path(),
            "g",
            "north-1",
            "2026-11-10",
            Some("2026-11-30"),
            REFUSAL
        )
        .unwrap()
    );
    let text = std::fs::read_to_string(dir.path().join("g")).unwrap();
    assert_eq!(body(&text).trim_end(), REFUSAL);
    assert_eq!(until(dir.path(), "g").as_deref(), Some("2026-11-30"));
    assert!(held(dir.path(), "g", "2026-11-10"));

    assert!(
        !mark(
            dir.path(),
            "p",
            "north-2",
            "2026-11-10",
            None,
            "HTTP 400: invalid x-api-key"
        )
        .unwrap()
    );
    assert!(!held(dir.path(), "p", "2026-11-10"));
}

#[test]
fn clearing_a_mark_leaves_a_tombstone_so_a_peer_resending_that_same_mark_cannot_restore_it() {
    let dir = tempfile::tempdir().unwrap();
    mark(
        dir.path(),
        "g",
        "north-1",
        "2026-11-10",
        Some("2026-11-30"),
        REFUSAL,
    )
    .unwrap();
    let stale = std::fs::read_to_string(dir.path().join("g")).unwrap();

    assert!(clear(dir.path(), "g", 1_794_305_400).unwrap());
    assert!(!held(dir.path(), "g", "2026-11-10"));
    assert_eq!(cleared(dir.path(), "g").unwrap(), stale.trim_end());

    let mut peer = BTreeMap::new();
    peer.insert("g".to_owned(), stale);
    assert!(adopt(dir.path(), &peer, "2026-11-10").unwrap().is_empty());
    assert!(
        !held(dir.path(), "g", "2026-11-10"),
        "a peer's stale copy restored the cleared mark"
    );

    let mut fresh = BTreeMap::new();
    fresh.insert(
        "g".to_owned(),
        "peer 2026-11-11\nHTTP 400: Your credit balance is too low again\n".to_owned(),
    );
    assert_eq!(adopt(dir.path(), &fresh, "2026-11-11").unwrap(), ["g"]);
    assert!(held(dir.path(), "g", "2026-11-11"));

    let bodyless = "north-1 2026-11-30\n";
    std::fs::write(dir.path().join("h"), bodyless).unwrap();
    assert!(refresh(dir.path(), "h", REFUSAL).unwrap());
    assert!(clear(dir.path(), "h", 1_794_470_400).unwrap());
    let mut peer = BTreeMap::new();
    peer.insert("h".to_owned(), bodyless.to_owned());
    assert!(adopt(dir.path(), &peer, "2026-11-30").unwrap().is_empty(), "a peer's bodyless copy restored a mark a probe had rewritten and then cleared");
    assert!(!held(dir.path(), "h", "2026-11-30"));
}
