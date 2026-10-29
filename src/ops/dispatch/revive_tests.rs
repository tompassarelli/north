use super::revive::{Step, Strand};
use super::slots::Account;
use super::tests::{fleet, issue, survey};
use super::plan::Cloud;
use super::decide;
use crate::capacity::saturation::Saturation;
use crate::fleet::stranded::{Lane, Stop, Stranded};
use std::collections::BTreeMap;

fn lane(ahead: u32, landed: bool) -> Lane {
    Lane { path: "/home/tom/code/north/worktrees/n245-citations".into(), branch: Some("n245-citations".into()), ahead, dirty: false, landed }
}

fn strand(lanes: Vec<Lane>) -> Strand {
    let session = Stranded {
        name: "n-b245b".into(),
        provider: "claude".into(),
        account: "b".into(),
        status: "exited".into(),
        repo: Some("north".into()),
        tasks: vec!["north#245".into()],
        machine: Some("gex45".into()),
        stop: Stop::Capacity,
        ended_ms: 1_793_286_089_516,
        lanes,
    };
    Strand { session, registry: "nexus".into() }
}

fn saturated(reading: Saturation) -> BTreeMap<String, String> {
    reading.refusal("gex45").map(|why| ("gex45".to_owned(), why)).into_iter().collect()
}

fn revival(strands: Vec<Strand>, reading: Saturation, accounts: Vec<Account>) -> super::Outcome {
    let (hosts, _) = fleet();
    let survey = super::survey::Survey { strands, saturated: saturated(reading), ..survey(vec![issue("north", 245, "- [ ] a", &[])], Cloud::Closed(String::new()), hosts, accounts) };
    decide(&survey, "test")
}

const FULL_DISK: Saturation = Saturation { queued: 0, cores: 20.0, load5: 3.0, disk_free_gib: 0.7 };
const FREED: Saturation = Saturation { disk_free_gib: 120.0, ..FULL_DISK };

#[test]
fn an_exited_session_with_an_unlanded_lane_and_a_capacity_stop_is_revived_once_its_host_frees_and_not_before() {
    let accounts = fleet().1;
    let held = revival(vec![strand(vec![lane(1, false)])], FULL_DISK, accounts.clone());
    assert!(held.revival.steps.is_empty(), "{held:?}");
    assert_eq!(held.revival.waiting, vec![("n-b245b".to_owned(), "gex45 skipped: 0.7 GiB free on the root disk, under 15".to_owned())]);
    assert!(held.launches.is_empty(), "the stranded session holds north#245, so no fresh launch starts beside it");

    let freed = revival(vec![strand(vec![lane(1, false)])], FREED, accounts.clone());
    let [Step::Revive(revive)] = freed.revival.steps.as_slice() else { panic!("{freed:?}") };
    assert_eq!((revive.name.as_str(), revive.registry.as_str(), revive.host.as_str(), revive.account.as_str()), ("n-b245b", "nexus", "gex45", "claude:b"));
    assert_eq!((revive.lanes.clone(), revive.local.clone()), (vec!["n245-citations".to_owned()], None), "its conversation lives on gex45, so placement runs it there");
    assert!(freed.launches.is_empty(), "{freed:?}");
    assert!(super::revive::prompt(revive).contains("n245-citations"));

    let out = accounts.iter().map(|account| Account { refusal: Some(format!("{} unavailable: exhausted: 5h 96% >= 95%", account.name)), ..account.clone() }).collect();
    let paced = revival(vec![strand(vec![lane(1, false)])], FREED, out);
    assert!(paced.revival.steps.is_empty(), "pace admission holds it as it would a launch");
    assert!(paced.revival.waiting[0].1.contains("exhausted"), "{paced:?}");
}

#[test]
fn a_session_is_revived_once_for_a_report_until_it_reports_again() {
    let (hosts, accounts) = fleet();
    let strands = vec![strand(vec![lane(1, false)])];
    let revived = BTreeMap::from([("n-b245b".to_owned(), 1_793_286_089_516)]);
    let base = survey(vec![], Cloud::Closed(String::new()), hosts, accounts);
    let again = decide(&super::survey::Survey { strands: strands.clone(), revived: revived.clone(), ..base }, "test");
    assert!(again.revival.steps.is_empty(), "{again:?}");
    let (hosts, accounts) = fleet();
    let mut later = strands;
    later[0].session.ended_ms += 1;
    let reported = decide(&super::survey::Survey { strands: later, revived, ..survey(vec![], Cloud::Closed(String::new()), hosts, accounts) }, "test");
    assert!(matches!(reported.revival.steps.as_slice(), [Step::Revive(_)]), "{reported:?}");
}

#[test]
fn a_stranded_session_whose_lane_work_already_landed_is_forgotten_instead_of_revived() {
    let accounts = fleet().1;
    for lanes in [vec![lane(0, false)], vec![lane(2, true)]] {
        let outcome = revival(vec![strand(lanes)], FREED, accounts.clone());
        assert_eq!(outcome.revival.steps, vec![Step::Forget { name: "n-b245b".into(), registry: "nexus".into() }]);
        assert!(outcome.revival.holds.is_empty(), "a forgotten session holds nothing");
    }
    let dirty = Lane { dirty: true, ..lane(0, false) };
    let outcome = revival(vec![strand(vec![dirty])], FREED, accounts);
    assert!(matches!(outcome.revival.steps.as_slice(), [Step::Revive(_)]), "uncommitted work is not landed: {outcome:?}");
}
