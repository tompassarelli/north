use super::admission::{Admission, Class, Facts, Profile, Reason, admit, decide};
use super::native_hold::QuietNative;

pub(super) fn facts(class: Class) -> Facts {
    Facts {
        profile: Profile::Attended,
        class,
        cores: 24,
        memory_total_mib: 96343.0,
        memory_available_mib: 70000.0,
        cpu_some_avg10_basis_points: 0,
        protected_cpu_some_avg10_basis_points: 0,
        leased_memory_mib: 0.0,
        leased_batch_cpus: 0.0,
        unleased_heavy_cores: 0.0,
        leased_native_cpus: 0.0,
        requested_cpus: 2.0,
        requested_memory_mib: 2048.0,
        peer_batch_runs: 0,
        peer_exclusive_runs: 0,
        unbounded_runs: 0,
        queued_ahead: 0,
        exclusive_waiting: 0,
        before_drain: false,
        gpu_busy_percent: 0,
        gpu_clients: 0,
        host_warcraft_clients: None,
        bar1_free_mib: None,
        bar1_timed_out: false,
        vram_free_mib: None,
        host_bar1_margin_mib: None,
        new_clients: 0,
        new_desktops: 0,
        gpu_runs: 0,
        native_waiting: 0,
        quiet_native: Vec::new(),
        alone: false,
        alone_queued_ahead: 0,
        owner: None,
        pair: None,
        pair_leases: Vec::new(),
        leases: Vec::new(),
        queue: Vec::new(),
        now: 0.0,
        beside_native: false,
        quiet: false,
        exclusive_leases: Vec::new(),
        within_exclusive: false,
        gaming: false,
        gaming_class: None,
        game_reserve_agent_cpus: None,
        queued_ahead_classes: Vec::new(),
        queued_ahead_new_clients: Vec::new(),
        run_queue_load: None,
        need: None,
        disk_free_gib: None,
        ticket: None,
    }
}

const OVERLOAD: &str = include_str!("../../harness/skills/machine-capacity/scripts/capacity-run-queue-overload.recorded.json");

fn overload(class: &str, need: Option<&str>) -> (Facts, toml_edit::DocumentMut) {
    let recorded: serde_json::Value = serde_json::from_str(OVERLOAD).unwrap();
    let mut facts = recorded["facts"].clone();
    facts["class"] = class.into();
    facts["need"] = need.into();
    (serde_json::from_value(facts).unwrap(), recorded["hosts"].as_str().unwrap().parse().unwrap())
}

fn overload_admission(class: &str, need: Option<&str>) -> Admission {
    let (facts, hosts) = overload(class, need);
    admit(&facts, |needs| crate::ops::pacing::placement::gate::serving(&hosts, needs, "here"))
}

#[test]
fn run_queue_over_thread_count_refuses_heavy_and_exclusive_naming_the_serving_host() {
    for class in ["heavy", "exclusive"] {
        let refused = |offload: Option<&str>| Admission { reason: Reason::DeferRunQueue, offload: offload.map(str::to_owned), aperture: None, disk_free_gib: None, pair: None, quiet_owners: Vec::new() };
        assert_eq!(overload_admission(class, Some("native:smashcraft")), refused(Some("gpu-host")), "{class}");
        assert_eq!(overload_admission(class, Some("private:smashcraft")), refused(Some("gpu-host")), "{class}");
        assert_eq!(overload_admission(class, Some("private:galileo")), refused(None), "{class}");
        assert_eq!(overload_admission(class, None), refused(None), "{class}");
    }
}

#[test]
fn run_queue_offload_names_only_another_host_that_serves_every_need() {
    let refused = |offload: Option<&str>| Admission { reason: Reason::DeferRunQueue, offload: offload.map(str::to_owned), aperture: None, disk_free_gib: None, pair: None, quiet_owners: Vec::new() };
    assert_eq!(overload_admission("heavy", Some("native:smashcraft,private:smashcraft")), refused(Some("gpu-host")));
    assert_eq!(overload_admission("heavy", Some("native:smashcraft,private:galileo")), refused(None));
    let (facts, hosts) = overload("heavy", Some("native:smashcraft"));
    assert_eq!(admit(&facts, |needs| crate::ops::pacing::placement::gate::serving(&hosts, needs, "gpu-host")), refused(None));
}

#[test]
fn run_queue_over_thread_count_still_admits_moderate_critical_and_tom() {
    for class in ["moderate", "critical", "tom"] {
        assert_eq!(overload_admission(class, Some("native:smashcraft")), Admission { reason: Reason::Run, offload: None, aperture: None, disk_free_gib: None, pair: None, quiet_owners: Vec::new() }, "{class}");
    }
}

#[test]
fn run_queue_at_thread_count_still_admits_heavy() {
    let (mut facts, _) = overload("heavy", Some("native:smashcraft"));
    facts.run_queue_load = Some(f64::from(facts.cores));
    assert_eq!(decide(&facts), Reason::Run);
}

fn beside(class: Class, quiet_native_runs: u32, beside_native: bool) -> Reason {
    let busy = |index: u32| QuietNative { owner: format!("quiet-{index}"), held_seconds: 60.0, idle_seconds: None, run_seconds: Some(3600.0) };
    decide(&Facts { quiet_native: (0..quiet_native_runs).map(busy).collect(), beside_native, ..facts(class) })
}

#[test]
fn live_quiet_native_session_holds_batch_work_until_it_ends() {
    for class in [Class::Moderate, Class::Heavy, Class::Gpu, Class::Exclusive] {
        assert_eq!(beside(class, 1, false), Reason::DeferNativeQuiet, "{class:?}");
        assert_eq!(beside(class, 0, false), Reason::Run, "{class:?}");
    }
}

#[test]
fn heavy_tests_and_builds_pass_busy_quiet_clients_that_are_not_a_protected_play_session() {
    let quiet = |class: Class, run_seconds: Option<f64>| {
        let lease = QuietNative { owner: "pool".into(), held_seconds: 43_560.0, idle_seconds: Some(0.0), run_seconds };
        admit(&Facts { quiet_native: vec![lease], ..facts(class) }, |_| None)
    };
    assert_eq!(quiet(Class::Heavy, None).reason, Reason::Run);
    assert_eq!(quiet(Class::Heavy, Some(43_200.0)).reason, Reason::Run);
    let held = quiet(Class::Heavy, Some(86_400.0));
    assert_eq!((held.reason, held.quiet_owners), (Reason::DeferNativeQuiet, vec!["pool".to_owned()]));
    for class in [Class::Moderate, Class::Gpu, Class::Exclusive] {
        assert_eq!(quiet(class, None).reason, Reason::DeferNativeQuiet, "{class:?}");
    }
}

#[test]
fn light_native_landing_and_tom_leases_run_beside_native() {
    for class in [Class::Agent, Class::Native, Class::Critical, Class::Tom] {
        assert_eq!(beside(class, 2, false), Reason::Run, "{class:?}");
    }
}

#[test]
fn native_owner_steps_opt_out_explicitly() {
    assert_eq!(beside(Class::Moderate, 1, true), Reason::Run);
    assert_eq!(beside(Class::Gpu, 1, true), Reason::Run);
}

#[test]
fn four_core_history_request_admits_only_at_its_clamped_grant() {
    let host = Facts {
        profile: Profile::Unattended,
        cores: 4,
        memory_total_mib: 16384.0,
        memory_available_mib: 14000.0,
        requested_memory_mib: 2429.0,
        ..facts(Class::Heavy)
    };
    assert_eq!(decide(&Facts { requested_cpus: 5.0, ..host.clone() }), Reason::DeferCpuCapacity);
    assert_eq!(decide(&Facts { requested_cpus: 4.0, ..host }), Reason::Run);
}

#[test]
fn unleased_heavy_cpu_reduces_admitted_capacity() {
    let mut host = facts(Class::Moderate);
    host.leased_batch_cpus = 16.0;
    host.unleased_heavy_cores = 3.0;
    assert_eq!(decide(&host), Reason::DeferCpuCapacity);
    host.unleased_heavy_cores = 2.0;
    assert_eq!(decide(&host), Reason::Run);
}

#[test]
fn gaming_holds_native_and_heavy_admissions_and_lets_light_work_run() {
    for class in [Class::Native, Class::Heavy, Class::Exclusive, Class::Gpu] {
        assert_eq!(decide(&Facts { gaming: true, ..facts(class) }), Reason::DeferGaming, "{class:?}");
        assert_eq!(decide(&Facts { gaming: false, ..facts(class) }), Reason::Run, "{class:?}");
    }
    for class in [Class::Agent, Class::Moderate, Class::Critical, Class::Tom] {
        assert_eq!(decide(&Facts { gaming: true, ..facts(class) }), Reason::Run, "{class:?}");
    }
}

#[test]
fn gaming_holds_a_landing_check_by_the_class_it_asked_for() {
    let landing = Facts { gaming: true, gaming_class: Some(Class::Heavy), ..facts(Class::Critical) };
    assert_eq!(decide(&landing), Reason::DeferGaming);
    assert_eq!(decide(&Facts { gaming: false, ..landing }), Reason::Run);
}

#[test]
fn gaming_deferred_tickets_ahead_do_not_queue_light_work_behind_them() {
    let ahead = |gaming, classes: &[Class]| Facts { gaming, queued_ahead: 2, queued_ahead_classes: classes.iter().copied().map(Some).collect(), ..facts(Class::Moderate) };
    assert_eq!(decide(&ahead(true, &[Class::Heavy, Class::Gpu])), Reason::Run);
    assert_eq!(decide(&ahead(true, &[Class::Heavy, Class::Moderate])), Reason::DeferQueued);
    assert_eq!(decide(&ahead(false, &[Class::Heavy, Class::Gpu])), Reason::DeferQueued);
    assert_eq!(decide(&ahead(true, &[Class::Critical, Class::Tom])), Reason::DeferQueued);
}

#[test]
fn quiet_native_timing_run_defers_beside_a_live_exclusive_lease_while_a_client_launch_still_runs() {
    let beside = |quiet: bool, peer_exclusive_runs: u32| decide(&Facts { quiet, peer_exclusive_runs, ..facts(Class::Native) });
    assert_eq!(beside(true, 1), Reason::DeferExclusive);
    assert_eq!(beside(true, 0), Reason::Run);
    assert_eq!(beside(false, 1), Reason::Run);
}

#[test]
fn quiet_native_run_inside_its_own_exclusive_lease_runs_but_not_beside_a_second_exclusive() {
    let within = |peer_exclusive_runs: u32| decide(&Facts { quiet: true, within_exclusive: true, peer_exclusive_runs, ..facts(Class::Native) });
    assert_eq!(within(1), Reason::Run);
    assert_eq!(within(2), Reason::DeferExclusive);
}

#[test]
fn batch_work_defers_beside_a_live_exclusive_lease() {
    for class in [Class::Moderate, Class::Heavy, Class::Gpu, Class::Critical, Class::Tom] {
        assert_eq!(decide(&Facts { peer_exclusive_runs: 1, ..facts(class) }), Reason::DeferExclusive, "{class:?}");
    }
}

fn nvidia_memory(name: &str) -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("harness/skills/machine-capacity/scripts").join(name)
}

fn bar1_free(path: &std::path::Path) -> Option<f64> {
    match super::aperture::read_memory(path).bar1 {
        super::aperture::Bar1::Free(free) => Some(free),
        _ => None,
    }
}

#[test]
fn a_native_client_start_waits_behind_an_earlier_native_ticket_while_a_run_on_live_clients_does_not() {
    let start = |new_clients: u32, queued_ahead: u32| decide(&Facts { new_clients, queued_ahead, ..facts(Class::Native) });
    assert_eq!(start(1, 1), Reason::DeferQueued);
    assert_eq!(start(1, 0), Reason::Run);
    assert_eq!(start(0, 1), Reason::Run);
    assert_eq!(decide(&Facts { new_clients: 0, new_desktops: 1, queued_ahead: 1, ..facts(Class::Native) }), Reason::DeferQueued);
}

fn desktop_pair(bar1_free_mib: Option<f64>) -> Facts {
    Facts { new_clients: 2, new_desktops: 2, bar1_free_mib, ..facts(Class::Native) }
}

fn measured(desktops: u32, clients: u32) -> f64 {
    use super::admission::default_mib as default;
    f64::from(desktops) * default("bar1_desktop_mib") + f64::from(clients) * default("bar1_client_mib") + default("bar1_margin_mib")
}

#[test]
fn a_full_bar1_aperture_defers_a_new_desktop_pair_and_a_gpu_run() {
    let free = bar1_free(&nvidia_memory("nvidia-memory-bar1-17-free.txt"));
    assert_eq!(free, Some(17.0));
    let admission = admit(&desktop_pair(free), |_| None);
    assert_eq!(admission.reason, Reason::DeferGpuAperture);
    assert_eq!(serde_json::to_value(&admission).unwrap(), serde_json::json!({ "reason": "DEFER_GPU_APERTURE", "bar1FreeMiB": 17.0, "bar1NeedMiB": measured(2, 2) }));
    assert_eq!(decide(&Facts { bar1_free_mib: free, ..facts(Class::Gpu) }), Reason::DeferGpuAperture);
    assert_eq!(decide(&Facts { new_clients: 0, new_desktops: 0, ..desktop_pair(free) }), Reason::Run);
}

#[test]
fn the_aperture_admits_a_request_exactly_when_free_bar1_covers_its_measured_cost() {
    for (desktops, clients) in [(1, 0), (0, 1), (1, 1), (2, 2)] {
        let need = measured(desktops, clients);
        let start = |free: f64| decide(&Facts { new_desktops: desktops, new_clients: clients, bar1_free_mib: Some(free), ..facts(Class::Native) });
        assert_eq!(start(need), Reason::Run, "{desktops} desktops, {clients} clients");
        assert_eq!(start(need - 1.0), Reason::DeferGpuAperture, "{desktops} desktops, {clients} clients");
    }
    let pair = |margin: u32, free: f64| decide(&Facts { host_bar1_margin_mib: Some(margin), ..desktop_pair(Some(free)) });
    assert_eq!(pair(0, measured(2, 2) - super::admission::default_mib("bar1_margin_mib")), Reason::Run);
    assert_eq!(pair(64, measured(2, 2)), Reason::DeferGpuAperture);
}

#[test]
fn a_bar1_aperture_with_200_mib_free_runs_a_new_desktop_with_one_client_but_defers_a_pair() {
    let free = bar1_free(&nvidia_memory("nvidia-memory-bar1-200-free.txt"));
    assert_eq!(free, Some(200.0));
    assert_eq!(decide(&Facts { new_clients: 1, new_desktops: 1, ..desktop_pair(free) }), Reason::Run);
    assert_eq!(decide(&desktop_pair(free)), Reason::DeferGpuAperture);
    assert_eq!(decide(&Facts { bar1_free_mib: free, ..facts(Class::Gpu) }), Reason::Run);
}

#[test]
fn a_host_without_nvidia_smi_runs_a_new_desktop_pair() {
    assert_eq!(super::aperture::query("/nonexistent/nvidia-smi", std::time::Duration::from_secs(5)).bar1, super::aperture::Bar1::Unknown);
    let free = None;
    assert_eq!(bar1_free(&nvidia_memory("no-such-nvidia-memory.txt")), None);
    assert_eq!(super::aperture::parse("GPU 00000000:01:00.0\n    FB Memory Usage\n        Free : 21811 MiB\n"), None);
    assert_eq!(decide(&desktop_pair(free)), Reason::Run);
    assert_eq!(decide(&Facts { bar1_free_mib: free, ..facts(Class::Gpu) }), Reason::Run);
}

#[test]
fn an_nvidia_smi_that_timed_out_defers_gpu_work_with_its_own_reason() {
    let hung = Facts { bar1_timed_out: true, ..desktop_pair(None) };
    assert_eq!(decide(&hung), Reason::DeferGpuApertureTimeout);
    assert_eq!(serde_json::to_value(admit(&hung, |_| None)).unwrap(), serde_json::json!({ "reason": "DEFER_GPU_APERTURE_TIMEOUT" }));
    assert_eq!(decide(&Facts { bar1_timed_out: true, ..facts(Class::Gpu) }), Reason::DeferGpuApertureTimeout);
    assert_eq!(decide(&Facts { new_clients: 0, new_desktops: 0, ..hung }), Reason::Run);
}

#[test]
fn the_warcraft_client_cap_follows_measured_free_bar1_and_vram_under_its_static_ceiling() {
    let on = |fixture: &str, host_warcraft_clients: Option<u32>, gpu_clients: u32| {
        let path = nvidia_memory(fixture);
        let vram_free_mib = super::aperture::read_memory(&path).vram_free_mib;
        Facts { profile: Profile::Unattended, host_warcraft_clients, gpu_clients, bar1_free_mib: bar1_free(&path), vram_free_mib, ..facts(Class::Native) }
    };
    let cap = |facts: &Facts| super::admission::warcraft_client_cap(facts);
    let rebar = on("nvidia-memory-gex45-rebar.txt", None, 6);
    assert_eq!((rebar.bar1_free_mib, rebar.vram_free_mib), (Some(29202.0), Some(21051.0)));
    assert_eq!(cap(&rebar), Some(6 + 15), "VRAM binds a 32 GiB aperture: (21051 - 1024) / 1310 more clients");
    assert_eq!(cap(&on("nvidia-memory-gex45-rebar.txt", Some(6), 6)), Some(6), "the static count stays the ceiling");
    let aperture = on("nvidia-memory-bar1-17-free.txt", Some(6), 2);
    assert_eq!(cap(&aperture), Some(2), "17 MiB of a 256 MiB aperture fits no client beyond the 2 running");
    assert_eq!(decide(&Facts { new_clients: 1, ..on("nvidia-memory-gex45-rebar.txt", Some(6), 5) }), Reason::Run);
    let tight = Facts { vram_free_mib: Some(2000.0), new_clients: 1, ..on("nvidia-memory-gex45-rebar.txt", Some(6), 3) };
    assert_eq!(cap(&tight), Some(3));
    assert_eq!(decide(&tight), Reason::DeferGpuClients, "2000 MiB of free VRAM fits no client after the 1024 MiB margin");
    assert_eq!(cap(&Facts { profile: Profile::Unattended, ..facts(Class::Native) }), None, "no measure and no static count leave the host uncapped");
}

#[test]
fn a_heavy_build_waits_while_the_root_disk_is_under_its_floor_and_names_the_free_space() {
    let low = Facts { disk_free_gib: Some(9.5), ..facts(Class::Heavy) };
    assert_eq!(admit(&low, |_| None), Admission { reason: Reason::DeferDiskFree, offload: None, aperture: None, disk_free_gib: Some(9.5), pair: None, quiet_owners: Vec::new() });
    assert_eq!(decide(&Facts { disk_free_gib: Some(15.0), ..facts(Class::Heavy) }), Reason::Run);
    assert_eq!(decide(&Facts { disk_free_gib: None, ..facts(Class::Heavy) }), Reason::Run);
    assert_eq!(decide(&Facts { disk_free_gib: Some(1.0), ..facts(Class::Agent) }), Reason::Run);
}

fn reserved(class: Class) -> Facts {
    let (requested_cpus, requested_memory_mib) = if class == Class::Heavy { (6.0, 8192.0) } else { (2.0, 2048.0) };
    Facts { gaming: true, game_reserve_agent_cpus: Some(16), requested_cpus, requested_memory_mib, ..facts(class) }
}

#[test]
fn game_reserve_counts_batch_cpus_against_the_agent_cpus_only() {
    let busy = Facts { leased_batch_cpus: 12.0, ..reserved(Class::Heavy) };
    assert_eq!(decide(&busy), Reason::DeferCpuCapacity);
    assert_eq!(decide(&Facts { leased_batch_cpus: 10.0, ..busy.clone() }), Reason::Run);
    assert_eq!(decide(&Facts { gaming: false, game_reserve_agent_cpus: None, ..busy }), Reason::Run);
}

#[test]
fn game_reserve_frees_light_work_queued_behind_held_tickets_but_not_behind_heavy_ones_it_admits() {
    let ahead = |classes: &[Class]| Facts { queued_ahead: 2, queued_ahead_classes: classes.iter().copied().map(Some).collect(), ..reserved(Class::Moderate) };
    assert_eq!(decide(&ahead(&[Class::Native, Class::Exclusive])), Reason::Run);
    assert_eq!(decide(&ahead(&[Class::Heavy, Class::Gpu])), Reason::DeferQueued);
}

fn gex45_on_2026_10_28(class: Class, disk_free_gib: f64, queued_ahead_classes: &[Class]) -> Facts {
    Facts {
        profile: Profile::Unattended,
        cores: 20,
        memory_total_mib: 64000.0,
        memory_available_mib: 54388.0,
        leased_memory_mib: 5220.0,
        leased_batch_cpus: 6.0,
        leased_native_cpus: 1.35,
        requested_cpus: if class == Class::Heavy { 6.0 } else { 2.0 },
        requested_memory_mib: if class == Class::Heavy { 8192.0 } else { 2048.0 },
        peer_batch_runs: 1,
        queued_ahead: u32::try_from(queued_ahead_classes.len()).unwrap(),
        queued_ahead_classes: queued_ahead_classes.iter().copied().map(Some).collect(),
        disk_free_gib: Some(disk_free_gib),
        ..facts(class)
    }
}

#[test]
fn heavy_builds_held_by_the_disk_floor_do_not_queue_moderate_work_behind_them() {
    let low = 12.39;
    assert_eq!(decide(&gex45_on_2026_10_28(Class::Heavy, low, &[])), Reason::DeferDiskFree);
    assert_eq!(decide(&gex45_on_2026_10_28(Class::Moderate, low, &[Class::Heavy, Class::Heavy])), Reason::Run);
    assert_eq!(decide(&gex45_on_2026_10_28(Class::Moderate, low, &[Class::Heavy, Class::Heavy, Class::Moderate])), Reason::DeferQueued);
    assert_eq!(decide(&gex45_on_2026_10_28(Class::Moderate, 20.0, &[Class::Heavy, Class::Heavy])), Reason::DeferQueued);
}

#[test]
fn a_configured_client_cap_prints_its_number_and_an_unconfigured_host_prints_nothing() {
    assert_eq!(super::limit_text(Some(6)), "6");
    assert_eq!(super::limit_text(None), "");
}

fn heavy_beside_clients(leased_batch_cpus: f64, gpu_clients: u32) -> Facts {
    Facts { cores: 20, profile: Profile::Unattended, run_queue_load: Some(25.4), requested_cpus: 6.0, requested_memory_mib: 8192.0, leased_batch_cpus, gpu_clients, leased_native_cpus: f64::from(gpu_clients) * 1.5, ..facts(Class::Heavy) }
}

#[test]
fn heavy_up_to_the_reserve_runs_above_the_thread_count_while_native_clients_run() {
    assert_eq!(decide(&heavy_beside_clients(0.0, 3)), Reason::Run);
    assert_eq!(decide(&heavy_beside_clients(6.0, 3)), Reason::Run);
    assert_eq!(decide(&heavy_beside_clients(6.5, 3)), Reason::DeferRunQueue);
    assert_eq!(decide(&heavy_beside_clients(0.0, 0)), Reason::DeferRunQueue);
}

#[test]
fn the_heavy_reserve_does_not_lift_the_run_queue_gate_for_exclusive_work() {
    let facts = Facts { class: Class::Exclusive, ..heavy_beside_clients(0.0, 3) };
    assert_eq!(decide(&facts), Reason::DeferRunQueue);
}

#[test]
fn either_a_live_client_or_native_cpus_open_the_heavy_reserve_and_untracked_load_closes_it() {
    let clients_only = Facts { leased_native_cpus: 0.0, ..heavy_beside_clients(0.0, 3) };
    let native_only = Facts { gpu_clients: 0, ..heavy_beside_clients(0.0, 3) };
    let untracked = Facts { unleased_heavy_cores: 7.0, ..heavy_beside_clients(0.0, 3) };
    let crowded = Facts { leased_native_cpus: 15.0, ..heavy_beside_clients(0.0, 3) };
    assert_eq!((decide(&clients_only), decide(&native_only), decide(&untracked), decide(&crowded)), (Reason::Run, Reason::Run, Reason::DeferRunQueue, Reason::DeferRunQueue));
}
