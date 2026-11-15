use super::{Reading, move_spent, next_subscription, settle, today};
use north_fleet_supervisor::fleet::handover::{AccountReadings, Observed};
use north_fleet_supervisor::fleet::model::{Machine, SessionRecord};
use north_pace::credit::Grant;
use crate::providers::claude::fleet::Claude;
use serde_json::json;
use std::collections::BTreeMap;
use std::path::Path;

fn grant(usd: f64, expires: &str) -> Grant {
    Grant { usd, granted: String::new(), expires: expires.to_owned() }
}

fn low_credit_turn(uuid: &str) -> [serde_json::Value; 3] {
    [
        json!({"type": "user", "uuid": uuid, "session_id": "c", "message": {"content": "carry on"}}),
        json!({"type": "assistant", "session_id": "c", "message": {"model": "<synthetic>", "content": [{"type": "text", "text": "Credit balance is too low"}]}}),
        json!({"type": "result", "session_id": "c", "subtype": "success", "is_error": true, "api_error_status": 400, "result": "Credit balance is too low"}),
    ]
}

fn refused_session(fleet: &north_fleet_supervisor::fleet::registry::Fleet, home: &Path, name: &str, ended: bool, turn: &[serde_json::Value]) {
    use north_fleet_supervisor::fleet::registry::fixtures;
    fixtures::live(fleet, name);
    let dir = home.join(".local/state/north/fleet").join(name);
    let mut record: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(dir.join("session.json")).unwrap()).unwrap();
    record["account"] = json!("api:zenith");
    std::fs::write(dir.join("session.json"), record.to_string()).unwrap();
    std::fs::write(dir.join("g1/stream.jsonl"), turn.iter().map(|frame| frame.to_string() + "\n").collect::<String>()).unwrap();
    if ended {
        fleet.phases().unwrap().moved(fleet.session(name).unwrap().launch(), north_fleet_supervisor::fleet::model::Phase::HostEnded, "its host ended").unwrap();
    }
}

fn pool_tick(open: &[&str], subscription: Option<(String, Option<u64>)>, sessions: &[(&str, bool, Vec<serde_json::Value>)]) -> Vec<(String, String, bool, Option<u64>)> {
    use north_fleet_supervisor::fleet::registry::Fleet;
    let home = tempfile::tempdir().unwrap();
    let here = north_base::platform::host::name();
    let policy = home.path().join("orchestration.toml");
    std::fs::write(&policy, format!("[accounts.claude.api_orgs.zenith]\nid = \"a\"\nkeys = {{ \"{here}\" = [\"/k/a\"] }}\n[accounts.claude.api_orgs.ember]\nid = \"p\"\nkeys = {{ \"{here}\" = [\"/k/p\"] }}\n")).unwrap();
    let fleet = Fleet::at(home.path().into(), north_fleet_supervisor::fleet::runtimes::Runtimes::new(vec![std::sync::Arc::new(Claude)]), std::sync::Arc::new(north_fleet_supervisor::fleet::fakes::Paper::default())).unwrap();
    for (name, ended, turn) in sessions {
        refused_session(&fleet, home.path(), name, *ended, turn);
    }
    let today = today();
    let grants = |id: &str, listed: bool| (id.to_owned(), if listed { vec![grant(200.0, "2099-12-31")] } else { Vec::new() });
    let reading = Reading { grants: [grants("a", false), grants("p", open.contains(&"ember"))].into(), next_credits: BTreeMap::new(), today };
    let mut moved = Vec::new();
    settle(&fleet, &Claude, (&policy, home.path()), &reading, |_: &SessionRecord, _: &Machine| subscription.clone(), |record, _, to, _, failed, resumes| {
        moved.push((record.name.clone(), to.to_owned(), failed, resumes));
        String::new()
    });
    moved
}

fn refusal(name: &str) -> Vec<serde_json::Value> {
    let mut turn = low_credit_turn("p1").to_vec();
    turn.iter_mut().for_each(|frame| frame["session_id"] = json!(name));
    turn
}

#[test]
fn with_no_api_org_open_a_refused_session_goes_to_the_subscription_account_that_resets_first_and_parks_there() {
    let moved = pool_tick(&[], Some(("b".to_owned(), Some(1_800_000_000_000))), &[("n730-arms", true, refusal("n730-arms"))]);
    assert_eq!(moved, [("n730-arms".to_owned(), "b".to_owned(), true, Some(1_800_000_000_000))]);

    use north_pace::usage::decide::world::{Label, LimitEvent, Unix, Window};
    let full = |account: &str, resets_at: i64| LimitEvent { account: Label(account.to_owned()), window: Window::FiveHour, resets_at: Some(Unix(resets_at)), at: Unix(0) };
    let limits = [full("a", 2_000_000), full("a", 3_000_000), full("b", 2_500_000), full("c", 3_500_000)];
    assert_eq!(next_subscription(["a", "b", "c"], &limits, 1_000_000), Some(("b", Some(2_500_000_000))));
    assert_eq!(next_subscription(["a", "b", "c", "d"], &limits, 1_000_000), Some(("d", None)));
    assert_eq!(next_subscription([], &limits, 1_000_000), None);
}

struct Quiet;

impl AccountReadings for Quiet {
    fn observed(&self, _now_ms: u64) -> Observed {
        Observed::default()
    }
}

#[test]
fn a_session_placed_before_its_host_set_an_agent_login_revives_and_settles_under_the_login_it_was_placed_with() {
    use north_fleet_supervisor::fleet::registry::Fleet;
    let home = tempfile::tempdir().unwrap();
    let far = home.path().join("far.toml");
    std::fs::write(&far, "[accounts.claude.profiles.a]\n\"far\" = { path = \"~/.claude-main\", token_file = \"~/a.token\" }\n[hosts.far]\nagent_ssh = \"agent@far\"\n").unwrap();
    let placed: SessionRecord = serde_json::from_value(json!({
        "name": "placed", "provider": "claude", "account": "a", "cwd": "/tmp", "model": null, "created_ms": 1,
        "host_pid": null, "conversation": "c", "host_dir": home.path().join("placed"), "stream_offset": 0,
        "machine": {"name": "far", "ssh": "far"}
    }))
    .unwrap();
    let account = north_fleet_supervisor::fleet::runtimes::Runtimes::new(vec![std::sync::Arc::new(Claude)]).account(&home.path().join(".local/state/north"), home.path(), &far, &north_fleet_supervisor::fleet::model::LaunchRequest::relaunching(&placed)).unwrap();
    assert_eq!(account.home, home.path().join(".claude-main"));
    assert_eq!(account.token_file, Some(home.path().join("a.token")));

    let here = north_base::platform::host::name();
    let policy = home.path().join("orchestration.toml");
    std::fs::write(&policy, format!("[accounts.claude.api_orgs.zenith]\nid = \"a\"\nkeys = {{ \"{here}\" = [\"/k/a\"] }}\n[hosts.\"{here}\"]\nagent_ssh = \"agent@{here}\"\n")).unwrap();
    let fleet = Fleet::at(home.path().into(), north_fleet_supervisor::fleet::runtimes::Runtimes::new(vec![std::sync::Arc::new(Claude)]), std::sync::Arc::new(north_fleet_supervisor::fleet::fakes::Paper::default())).unwrap();
    refused_session(&fleet, home.path(), "n-placed", true, &refusal("n-placed"));
    let dir = home.path().join(".local/state/north/fleet/n-placed");
    let mut record: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(dir.join("session.json")).unwrap()).unwrap();
    record["machine"] = json!({"name": here, "ssh": here});
    std::fs::write(dir.join("session.json"), record.to_string()).unwrap();

    let lines = move_spent(&fleet, &Claude, &policy, home.path(), &Quiet);

    assert_eq!(lines, [
        "api:zenith marked exhausted: n-placed's last turn ended on a credit balance too low",
        "n-placed: api:zenith ran out of credit and no other api org or subscription account is open, so it stays parked",
    ]);
}
