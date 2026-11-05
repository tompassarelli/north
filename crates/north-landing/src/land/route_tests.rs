use super::{Route, Settings, decide};
use crate::landing::safe_push_plan::Arguments;

fn settings(queue: &str, hosts: &[&str], committed: bool) -> Settings {
    Settings { queue: queue.to_owned(), hosts: hosts.iter().map(|&host| host.to_owned()).collect(), committed }
}

#[test]
fn a_queue_other_than_lander_lands_per_lane_on_any_host() {
    for queue in ["lock", "autoland", "direct"] {
        assert_eq!(decide(&settings(queue, &["lander"], true), "lander", "main", &Arguments::default(), false), Route::PerLane);
    }
}

#[test]
fn machine_settings_queue_only_a_one_commit_lane_on_a_named_host() {
    let machine = settings("lander", &["lander.local"], false);
    assert_eq!(decide(&machine, "lander", "main", &Arguments::default(), false), Route::Lander);
    assert_eq!(decide(&machine, "laptop", "main", &Arguments::default(), false), Route::PerLane);
    let several = Arguments { several_commits: Some("bisect points".to_owned()), ..Arguments::default() };
    assert_eq!(decide(&machine, "lander", "main", &several, false), Route::PerLane);
    let direct = Arguments { direct: true, ..Arguments::default() };
    assert_eq!(decide(&machine, "lander", "main", &direct, false), Route::PerLane);
}

#[test]
fn committed_settings_refuse_direct_and_an_unnamed_host_and_queue_the_rest() {
    let committed = settings("lander", &["lander", "remote"], true);
    let direct = Arguments { direct: true, ..Arguments::default() };
    assert!(matches!(decide(&committed, "lander", "main", &direct, false), Route::Refused(reason) if reason.starts_with("--direct is refused here: landing.queue is lander")));
    assert!(matches!(decide(&committed, "other.example", "main", &Arguments::default(), false), Route::Refused(reason) if reason.contains("add other to landing.remote-hosts")));
    let several = Arguments { several_commits: Some("bisect points".to_owned()), ..Arguments::default() };
    assert_eq!(decide(&committed, "remote", "main", &several, false), Route::Lander);
}

#[test]
fn machine_settings_land_per_lane_while_no_lander_serves_the_queue_and_committed_settings_still_queue() {
    let machine = settings("lander", &["lander"], false);
    assert_eq!(decide(&machine, "lander", "main", &Arguments::default(), true), Route::PerLane);
    let committed = settings("lander", &["lander"], true);
    assert_eq!(decide(&committed, "lander", "main", &Arguments::default(), true), Route::Lander);
}
