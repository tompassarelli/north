use serde_json::json;

use super::host_facts::parse;
use super::waste::{REPEAT_SECS, waste};
use super::*;

const NOW: u64 = 1_793_750_000;

fn queued(count: usize, waited: u64) -> CapacityView {
    let ticket = Ticket { owner: "w".into(), class: "heavy".into(), queued_since: NOW - waited, wait_p90_secs: None };
    CapacityView { queued: vec![ticket; count], cpu_some_avg10: 0.0, pressure_target: 25.0, pressure_since: None }
}

fn host(name: &str, capacity: Option<CapacityView>, waste: WasteView) -> HostView {
    HostView { name: name.into(), local: name == "whiterabbit", down_since: None, capacity, waste: Some(waste) }
}

fn laptop() -> WasteView {
    WasteView { cores: 24.0, busy_cores: 8.0, memory_total_mib: 96_000.0, memory_available_mib: 70_000.0, committed_cpus: 19.5, cpu_limit: 20.0, ..WasteView::default() }
}

fn keys(context: &WatchContext) -> Vec<String> {
    waste(context).into_iter().map(|alert| alert.key).collect()
}

#[test]
fn waste_pages_the_laptop_queue_held_by_commit_accounting_while_cores_idle() {
    let context = WatchContext { now: NOW, hosts: vec![host("whiterabbit", Some(queued(14, 90)), laptop())], ..WatchContext::default() };
    let alerts = waste(&context);
    assert_eq!(alerts.iter().map(|alert| alert.key.as_str()).collect::<Vec<_>>(), ["work:waste:whiterabbit:commit-limit"]);
    assert_eq!((alerts[0].severity, alerts[0].repeat_secs), (Severity::Page, Some(REPEAT_SECS)));
    assert!(alerts[0].summary.starts_with("14 waiting up to 90 s on whiterabbit while 8.0 of 24 cores are busy"), "{}", alerts[0].summary);
}

#[test]
fn waste_names_a_game_reserve_left_on_with_no_game_seen() {
    let view = WasteView { game_reserve_on: true, game_seen: Some(NOW - 4 * 3600), committed_cpus: 4.0, ..laptop() };
    let context = WatchContext { now: NOW, hosts: vec![host("whiterabbit", Some(queued(3, 120)), view)], ..WatchContext::default() };
    assert_eq!(keys(&context), ["work:waste:whiterabbit:game-reserve"]);
}

#[test]
fn waste_names_all_offload_slots_held_on_an_idle_host() {
    let view = WasteView {
        cores: 20.0,
        busy_cores: 8.35,
        memory_total_mib: 64_000.0,
        memory_available_mib: 54_000.0,
        slot_pools: vec![SlotPool { name: "code-north-main".into(), held: 3, total: 3 }],
        offloads_waiting: vec![130, 75],
        ..WasteView::default()
    };
    let context = WatchContext { now: NOW, hosts: vec![host("gex45", Some(queued(0, 0)), view)], ..WatchContext::default() };
    assert_eq!(keys(&context), ["work:waste:gex45:offload-slots"]);
}

#[test]
fn waste_names_a_stale_lease_and_an_unexplained_idle_queue() {
    let stale = WasteView { stale_leases: vec!["old (4 CPUs, 3 h)".into()], committed_cpus: 4.0, ..laptop() };
    let plain = WasteView { committed_cpus: 4.0, ..laptop() };
    let context = WatchContext { now: NOW, hosts: vec![host("whiterabbit", Some(queued(2, 300)), stale), host("nexus", Some(queued(1, 300)), plain)], ..WatchContext::default() };
    assert_eq!(keys(&context), ["work:waste:whiterabbit:stale-lease", "work:waste:nexus:unexplained"]);
}

#[test]
fn waste_stays_quiet_for_short_waits_busy_cores_or_scarce_memory() {
    let busy = WasteView { busy_cores: 20.0, ..laptop() };
    let tight = WasteView { memory_available_mib: 10_000.0, ..laptop() };
    let hosts = vec![host("a", Some(queued(5, 30)), laptop()), host("b", Some(queued(5, 300)), busy), host("c", Some(queued(5, 300)), tight), host("d", Some(queued(0, 0)), laptop())];
    assert!(waste(&WatchContext { now: NOW, hosts, ..WatchContext::default() }).is_empty());
}

#[test]
fn host_facts_parse_cores_memory_slots_waiters_and_leases() {
    let facts = "cores 20\ncpu  100 0 100 800 0 0 0 0 0 0\ncpu  150 0 150 900 0 0 0 0 0 0\nMemTotal:       65536000 kB\nMemAvailable:   55296000 kB\nreserve {\"on\":true,\"auto\":true}\nseen 1793740000\nslot code-north-main held\nslot code-north-main-1 held\nslot code-north-main-2 held\nslot code-makgora-worktrees-test-loop free\noffload 900\noffload 800\noffload 700\noffload 130\noffload 75\n";
    let status = json!({"committedBatchCpus": 18.0, "aggregateCpuLimit": 20, "holding": [{"owner": "old", "committedCpus": 4, "heldSeconds": 7200, "measured": {"liveCores": 0}}, {"owner": "live", "committedCpus": 2, "heldSeconds": 7200, "measured": {"liveCores": 1.5}}]});
    let view = parse(facts, Some(&status));
    assert_eq!((view.cores, view.busy_cores, view.memory_total_mib, view.memory_available_mib), (20.0, 10.0, 64_000.0, 54_000.0));
    assert_eq!((view.game_reserve_on, view.game_seen), (true, Some(1_793_740_000)));
    assert_eq!(view.slot_pools, [SlotPool { name: "code-makgora-worktrees-test-loop".into(), held: 0, total: 1 }, SlotPool { name: "code-north-main".into(), held: 3, total: 3 }]);
    assert_eq!(view.offloads_waiting, [75, 130]);
    assert_eq!(view.stale_leases, ["old (4 CPUs, 2 h)"]);
}
