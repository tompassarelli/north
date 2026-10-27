use super::decide;
use super::measure::{self, Measure};
use super::plan::Cloud;
use super::slots::{Account, Host, Room};
use super::tests::{issue, survey};
use super::wait;
use crate::capacity::admission::Class;

const NEXUS: Measure = Measure { cores: 3.0, memory_total_mib: 7680.0, memory_available_mib: 5200.0, load: 0.4 };
const GEX45: Measure = Measure { cores: 20.0, memory_total_mib: 64084.0, memory_available_mib: 31000.0, load: 41.0 };

fn host(name: &str, running: Result<i64, &str>, measure: Option<Measure>) -> Host {
    Host { name: name.into(), cap: Some(8), running: running.map_err(str::to_owned), measure }
}

fn accounts() -> Vec<Account> {
    let room = Room { five_hour: 80.0, week: 80.0, week_reset_h: 100.0 };
    let account = |name: &str, hosts: &[&str]| Account { name: name.into(), hosts: hosts.iter().map(|host| (*host).to_owned()).collect(), plan: 1.0, room, spending: false, refusal: None };
    vec![account("claude:a", &["nexus", "whiterabbit"]), account("claude:b", &["gex45"])]
}

fn placed(body: &str, hosts: Vec<Host>) -> Result<String, String> {
    let outcome = decide(&survey(vec![issue("smashcraft", 597, body, &[])], Cloud::Closed(String::new()), hosts, accounts()), "test");
    outcome.launches.first().map(|launch| launch.host.clone()).ok_or_else(|| outcome.skipped.first().map(|(_, why)| why.clone()).unwrap_or_default())
}

#[test]
fn a_heavy_smashcraft_box_goes_to_gex45_or_the_laptop_and_never_to_a_7_gb_host() {
    let heavy = "- [ ] (headless run) hit regions";
    let free_gex45 = || host("gex45", Ok(0), Some(GEX45));
    let nexus = || host("nexus", Ok(0), Some(NEXUS));
    assert_eq!(placed(heavy, vec![free_gex45(), nexus(), host("whiterabbit", Ok(0), None)]), Ok("whiterabbit".into()), "gex45's run queue is past its cores, the idle laptop starts it sooner");
    assert_eq!(placed(heavy, vec![free_gex45(), nexus(), host("whiterabbit", Err("laptop unavailable"), None)]), Ok("gex45".into()), "a busy gex45 still beats a host that can never admit it");
    let full_gex45 = host("gex45", Ok(8), Some(GEX45));
    let refused = placed(heavy, vec![full_gex45, nexus(), host("whiterabbit", Err("laptop unavailable"), None)]).unwrap_err();
    assert!(refused.contains("nexus cannot admit a heavy run of 8192 MiB in 7680 MiB"), "{refused}");
    assert_eq!(placed("Verify: none\n- [ ] read it", vec![free_gex45(), nexus(), host("whiterabbit", Err("laptop unavailable"), None)]), Ok("nexus".into()), "an agent-only box fits Nexus, whose queue is shorter");
}

#[test]
fn the_wait_counts_the_run_queue_past_the_cores_and_one_run_when_memory_would_defer() {
    assert_eq!(wait::class("Verify: local-test"), Class::Heavy);
    assert_eq!(wait::class("Verify: native"), Class::Heavy);
    assert_eq!(wait::class("Verify: none"), Class::Agent);
    assert_eq!(wait::wait(Class::Heavy, &GEX45), Ok(41.0 / 20.0 - 1.0));
    assert_eq!(wait::wait(Class::Agent, &NEXUS), Ok(0.0));
    let tight = Measure { memory_available_mib: 9000.0, load: 10.0, ..GEX45 };
    assert_eq!(wait::wait(Class::Heavy, &tight), Ok(1.0), "8192 MiB more would cut into gex45's 8192 MiB floor");
    assert!(wait::wait(Class::Heavy, &NEXUS).is_err());
}

#[test]
fn a_hosts_measure_is_read_from_nproc_loadavg_and_meminfo() {
    let probe = "20\n41.02 37.72 35.15 31/5160 3351912\nMemTotal:       65621040 kB\nMemFree:        12404044 kB\nMemAvailable:   31894972 kB\n";
    let read = measure::parse(probe).unwrap();
    assert_eq!((read.cores, read.load), (20.0, 41.02));
    assert!((read.memory_total_mib - 64083.04).abs() < 0.01 && (read.memory_available_mib - 31147.43).abs() < 0.01, "{read:?}");
    assert_eq!(measure::parse("20\n"), None);
}
