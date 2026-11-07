use super::decide;
use super::revive::{Step, Strand};
use super::tests::{fleet, survey};
use crate::fleet::stranded::{Lane, Stop, Stranded};

fn strand(name: &str, stop: Stop, machine: Option<&str>, resumed: bool) -> Strand {
    let lane = Lane { path: format!("/work/{name}").into(), branch: Some(name.into()), ahead: 1, dirty: false, landed: false };
    let session = Stranded { name: name.into(), provider: "claude".into(), account: "b".into(), status: "exited".into(), repo: Some("north".into()), tasks: vec!["north#790".into()], machine: machine.map(str::to_owned), stop, ended_ms: 10, resumed, lanes: vec![lane] };
    Strand { session, registry: "gex45".into() }
}

/// north#790: dispatch revive and the fleet resume pass both woke a stopped session on 2026-11-07, so one stop could send two messages.
#[test]
fn one_stop_wakes_once_a_local_capacity_stop_is_the_resume_passs_and_a_session_it_already_woke_is_not_revived() {
    let (hosts, accounts) = fleet();
    let mut surveyed = survey(Vec::new(), super::plan::Cloud::Unreachable("none".into()), hosts, accounts);
    surveyed.strands = vec![
        strand("local-capacity", Stop::Capacity, None, false),
        strand("remote-capacity", Stop::Capacity, Some("gex45"), false),
        strand("local-wait", Stop::Wait, None, false),
        strand("woken", Stop::Wait, None, true),
    ];
    let revival = decide(&surveyed, "test").revival;
    let revived: Vec<&str> = revival.steps.iter().filter_map(|step| if let Step::Revive(revive) = step { Some(revive.name.as_str()) } else { None }).collect();
    assert_eq!(revived, ["remote-capacity", "local-wait"], "{:?}", revival.waiting);
    let waiting: Vec<&str> = revival.waiting.iter().map(|(name, _)| name.as_str()).collect();
    assert_eq!(waiting, ["local-capacity", "woken"]);
}
