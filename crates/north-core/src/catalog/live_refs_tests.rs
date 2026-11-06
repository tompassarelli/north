

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

#[test]
fn a_dangling_user_unit_link_costs_its_own_unit_not_the_whole_unit_read() {
    let scratch = tempfile::tempdir().expect("scratch");
    let systemctl = scratch.path().join("systemctl");
    crate::foundation::shim::write_script(
        &systemctl,
        "case \"$2\" in\nlist-unit-files) printf 'blueman-applet.service bad ignored\\nrun-p1-i2.service transient -\\ncapacity-watchdog.service linked enabled\\n' ;;\ncat) for unit in \"$@\"; do case \"$unit\" in blueman-applet.service|run-p1-i2.service) echo \"No files found for $unit.\" >&2; exit 1 ;; esac; done\n  printf '# /home/tom/.config/systemd/user/capacity-watchdog.service\\n[Service]\\nExecStart=/home/tom/.local/share/north/bin/capacity-watchdog sample\\n' ;;\nesac\n",
    );
    let units = user_units(&systemctl).expect("one dangling unit link fails the whole read");
    assert_eq!(unit_exec_starts(&units), vec![UnitFact { unit: "/home/tom/.config/systemd/user/capacity-watchdog.service".into(), path: "/home/tom/.local/share/north/bin/capacity-watchdog".into() }]);
}

