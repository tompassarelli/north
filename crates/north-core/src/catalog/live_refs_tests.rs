

use super::*;

const HOUR: u64 = 3_600_000;
const UNITS: &str = "# /home/tom/.config/systemd/user/capacity-watchdog.service -> /nix/store/d1s7-capacity-watchdog.service/capacity-watchdog.service\n[Service]\nEnvironment=PATH=/run/wrappers/bin\nExecStart=/home/tom/.local/share/north/bin/capacity-watchdog sample\nExecStartPre=-/bin/true\n# /home/tom/.config/systemd/user/agents-flow.service\n[Service]\nExecStart=/usr/bin/env bash /home/tom/code/south/main/bin/agents-flow --once\n";

#[test]
fn a_unit_whose_exec_start_runs_capacity_watchdog_blocks_deleting_it_by_name_or_bin_path() {
    let facts = Facts { schema: SCHEMA.into(), host: "gex45".into(), recorded_at_ms: 1_000, hooks: Vec::new(), unit_exec_starts: unit_exec_starts(UNITS) };
    assert_eq!(facts.unit_exec_starts.len(), 4, "{:?}", facts.unit_exec_starts);
    for name in ["capacity-watchdog", "bin/capacity-watchdog"] {
        let report = judge(std::slice::from_ref(&facts), name, &[], 2_000, HOUR);
        assert_eq!(report.naming.len(), 1, "{name}: {report:?}");
        assert!(report.naming[0].contains("capacity-watchdog.service runs /home/tom/.local/share/north/bin/capacity-watchdog"), "{report:?}");
    }
    assert_eq!(judge(std::slice::from_ref(&facts), "agents-flow", &[], 2_000, HOUR).naming.len(), 1);
    assert!(judge(std::slice::from_ref(&facts), "watchdog", &[], 2_000, HOUR).is_clean());
}

