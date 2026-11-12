#![cfg(test)]

use serde_json::json;

use super::admission::{Class, Facts, Profile, Reason, admit, decide, default_mib};
use super::native_hold::LeaseFact;
use super::native_hold::{PairRefusal, queue_pair_leases};
use super::tests::{facts, running};

#[test]
fn a_pair_the_wisp_queue_holds_for_a_live_process_is_refused_to_a_native_lease_with_a_different_free_pair() {
    let directory = std::env::temp_dir().join(format!("capture-queue-{}", std::process::id()));
    std::fs::create_dir_all(directory.join("leases")).unwrap();
    let write = |name: &str, host: &str, pair: u32, pid: u32| std::fs::write(directory.join("leases").join(name), json!({ "host": host, "pair": pair, "owner": "wisp-sylv-cloak", "pid": pid, "since": "" }).to_string()).unwrap();
    write("gex45-16.json", "gex45", 16, std::process::id());
    write("gex45-17.json", "gex45", 17, u32::MAX);
    write("elsewhere-18.json", "elsewhere", 18, std::process::id());
    let held = queue_pair_leases(&directory, "gex45");
    std::fs::remove_dir_all(&directory).unwrap();
    assert_eq!(held.iter().map(|lease| (lease.pair, lease.owner.as_str())).collect::<Vec<_>>(), vec![(16, "wisp-sylv-cloak")]);
    let ask = |pair: u32| Facts { owner: Some("sc-lk-gale".to_owned()), pair: Some(pair), pair_leases: held.clone(), ..facts(Class::Native) };
    let refused = admit(&ask(16), |_| None);
    assert_eq!(refused.reason, Reason::PairHeld);
    assert_eq!(refused.pair, Some(PairRefusal { pair: 16, holder: "wisp-sylv-cloak".to_owned(), free_pair: 0 }));
    assert_eq!(decide(&ask(0)), Reason::Run);

    {
        let at = |host_gpu_slots: Option<u32>, gpu_runs: u32| decide(&Facts { profile: Profile::Unattended, host_gpu_slots, gpu_runs, ..facts(Class::Gpu) });
        assert_eq!(at(None, 1), Reason::Run);
        assert_eq!(at(None, 2), Reason::DeferGpuSlots);
        assert_eq!(at(Some(4), 3), Reason::Run);
        assert_eq!(at(Some(4), 4), Reason::DeferGpuSlots);

        let (run, headroom, ramp) = (default_mib("vram_gpu_run_mib"), default_mib("vram_gpu_headroom_mib"), default_mib("gpu_vram_ramp_seconds"));
        let now = 10_000_000.0;
        let at = |vram_free_mib: Option<f64>, ages: &[f64]| {
            let leases: Vec<LeaseFact> = ages.iter().map(|age| LeaseFact { created_at: now - age * 1000.0, ..running("gpu") }).collect();
            decide(&Facts { profile: Profile::Unattended, host_gpu_slots: Some(6), gpu_runs: leases.len() as u32, vram_free_mib, now, leases, ..facts(Class::Gpu) })
        };
        assert_eq!(at(Some(headroom + run), &[]), Reason::Run);
        assert_eq!(at(Some(headroom + run - 1.0), &[]), Reason::DeferGpuMemory);
        assert_eq!(at(Some(headroom + run), &[ramp - 1.0]), Reason::DeferGpuMemory);
        assert_eq!(at(Some(headroom + run), &[ramp + 1.0]), Reason::Run);
        assert_eq!(at(None, &[]), Reason::Run);
    }
}
