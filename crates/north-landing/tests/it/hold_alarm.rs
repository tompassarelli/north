use super::promote::{ELSEWHERE, Held, Promoted, replay_script, scenario, unfit_signature};

fn held_for(minutes: u64) -> Promoted {
    scenario(replay_script("failed", &[ELSEWHERE]), Some(Held { script: replay_script("failed", &[]), change: None, held_for: Some(minutes * 60) }), unfit_signature)
}

#[test]
fn promote_records_one_incident_and_messages_the_proxy_once_the_lander_link_has_been_held_fifteen_minutes() {
    let quiet = held_for(5);
    assert!(!quiet.status, "{}", quiet.stdout);
    assert_eq!(quiet.alarms, "", "a hold under fifteen minutes is quiet");

    let promoted = held_for(16);
    assert!(!promoted.status, "{}", promoted.stdout);
    let alarms: Vec<&str> = promoted.alarms.lines().collect();
    assert_eq!(alarms.iter().filter(|call| call.starts_with("incident record --key lander-held:") && call.contains("--class lander-held")).count(), 1, "{alarms:?}");
    assert_eq!(alarms.iter().filter(|call| call.starts_with("fleet send --from service:north-land proxy ") && call.contains("has been held for 16 minutes")).count(), 1, "{alarms:?}");
}
