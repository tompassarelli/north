use super::*;

const PATCH: &str = "0123456789abcdef0123456789abcdef01234567";

#[test]
fn a_review_accept_request_carries_a_patch_id_alone_and_is_recorded_for_the_calling_session_north_1036() {
    let words = |text: &[&str]| text.iter().map(|word| (*word).to_owned()).collect::<Vec<_>>();
    assert_eq!(Accept::asked(&words(&["review-accept", PATCH])), Some(Ok(Accept { patch_id: PATCH.to_owned(), withdraw: false })));
    assert!(Accept::asked(&words(&["review-accept", "main"])).is_some_and(|asked| asked.is_err()));
    assert!(Accept::asked(&words(&["review-accept", PATCH, "reviewer-b"])).is_some_and(|asked| asked.is_err()));
    assert_eq!(Accept::asked(&words(&["review-hold", "lift"])), None);
    for name in ["abc1234", &"a".repeat(64)] {
        assert!(command(&[name.to_owned()], true, |_: Accept| -> Result<String, String> { panic!("a name that is not a 40-digit sha reached the accept") }).is_err(), "an accept names a 40-digit sha, never a short name or a 64-digit name a worker could plant");
    }

    let store = tempfile::tempdir().unwrap();
    let accepts = Accepts(store.path().join("accepts"));
    let accept = Accept { patch_id: PATCH.to_owned(), withdraw: false };
    let caller = |session: &str| Caller { session: session.to_owned(), coordinator: false, reviewer: true };
    accept.serve(&accepts, &caller("reviewer-a"), 1).unwrap();
    accept.serve(&accepts, &caller("reviewer-a"), 2).unwrap();
    assert_eq!(accepts.sessions(PATCH).unwrap().len(), 1, "one session accepting twice is one accept");
    accept.serve(&accepts, &caller("reviewer-b"), 3).unwrap();
    assert_eq!(accepts.sessions(PATCH).unwrap().into_iter().collect::<Vec<_>>(), ["reviewer-a", "reviewer-b"]);
    assert!(accept.serve(&accepts, &caller("../escape"), 4).is_err());
    let worker = Caller { session: "worker-c".to_owned(), coordinator: false, reviewer: false };
    assert!(accept.serve(&accepts, &worker, 5).is_err());
    assert_eq!(accepts.sessions(PATCH).unwrap().len(), 2, "a session the fleet did not launch as a reviewer records no accept");
    assert!(accept.serve(&accepts, &caller("review-call"), 6).is_err(), "a reviewer session records no accept under a review call's name");
    assert!(accepts.record_call(&Record { patch_id: PATCH.to_owned(), session: "reviewer-c".to_owned(), at: 7 }).is_err(), "the review call records under its own names alone");
    accepts.record_call(&Record { patch_id: PATCH.to_owned(), session: "review-call".to_owned(), at: 8 }).unwrap();
    assert_eq!(accepts.sessions(PATCH).unwrap().len(), 3);
}
