use super::*;

#[test]
fn session_ids_cannot_escape_the_grounding_state_dir() {
    assert!(session_path("../escape").is_none());
    assert!(session_path("").is_none());
    assert!(session_path("895929a7-6218").unwrap().ends_with("sessions/895929a7-6218.json"));
}

