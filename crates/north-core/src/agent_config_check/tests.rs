use std::fs;
use std::path::{Path, PathBuf};

use tempfile::TempDir;

use super::programs::skill_programs;

fn gex45_with(programs: &[&str]) -> (TempDir, PathBuf, Vec<super::programs::HostClass>) {
    let dir = tempfile::tempdir().unwrap();
    let bin = dir.path().join("gex45-bin");
    fs::create_dir_all(&bin).unwrap();
    for program in programs {
        let file = bin.join(program);
        crate::foundation::shim::write_script(&file, "");
    }
    let host = super::programs::HostClass { name: "gex45".into(), bins: vec![bin.clone()] };
    (dir, bin, vec![host])
}

fn machine_capacity_skill(root: &Path, text: &str) {
    let skill = root.join("harness/skills/machine-capacity");
    fs::create_dir_all(&skill).unwrap();
    fs::write(skill.join("SKILL.md"), text).unwrap();
}

#[test]
fn skill_programs_refuse_the_machine_capacity_line_694b2a05_ran_and_pass_once_it_is_installed_or_renamed() {
    let committed = include_str!("fixtures/machine-capacity-f1745de2e.md");
    let (dir, bin, hosts) = gex45_with(&["bun", "north"]);
    machine_capacity_skill(dir.path(), committed);
    let runtime = dir.path().join("runtime-bin");

    let refused = skill_programs(dir.path(), &runtime, &hosts);
    assert!(!refused.passed, "{}", refused.text);
    assert!(refused.text.contains("harness/skills/machine-capacity/SKILL.md:18: machine-capacity is on no PATH of gex45"), "{}", refused.text);

    crate::foundation::shim::write_script(&bin.join("machine-capacity"), "");
    let after_install = skill_programs(dir.path(), &runtime, &hosts);
    assert!(after_install.passed, "{}", after_install.text);
    fs::remove_file(bin.join("machine-capacity")).unwrap();

    let renamed = committed.replace("`machine-capacity run ", "`bun run ").replace("`machine-capacity estimate ", "`bun estimate ");
    machine_capacity_skill(dir.path(), &renamed);
    let after_rename = skill_programs(dir.path(), &runtime, &hosts);
    assert!(after_rename.passed, "{}", after_rename.text);
}

fn slow_first_session_start(dir: &Path) -> PathBuf {
    let north = dir.join("north");
    let body = format!(
        "case \"$1\" in\ncapacity) echo leased >>'{log}'; while [ \"$1\" != -- ]; do shift; done; shift; exec \"$@\" ;;\nhook) [ -e '{marker}' ] || {{ touch '{marker}'; exec sleep 5; }}; cat >/dev/null; printf context ;;\nesac\n",
        log = dir.join("leases").display(),
        marker = dir.join("stalled-once").display(),
    );
    crate::foundation::shim::write_script(&north, &body);
    north
}

#[test]
fn a_session_start_probe_that_stalls_on_a_host_loaded_past_its_cpus_is_retried_under_a_capacity_lease() {
    let dir = tempfile::tempdir().unwrap();
    let north = slow_first_session_start(dir.path());
    let start = super::gate::session_start_probe(north.into(), dir.path().to_path_buf(), std::time::Duration::from_secs(1), || Some(1.9));
    assert_eq!(start(true), Ok("context".len()));
    assert_eq!(fs::read_to_string(dir.path().join("leases")).unwrap(), "leased\n");
}

#[test]
fn a_session_start_probe_that_stalls_on_an_idle_host_fails_the_generation_without_a_lease() {
    let dir = tempfile::tempdir().unwrap();
    let north = slow_first_session_start(dir.path());
    let start = super::gate::session_start_probe(north.into(), dir.path().to_path_buf(), std::time::Duration::from_secs(1), || Some(0.4));
    let refused = start(true).unwrap_err();
    assert!(refused.contains("did not finish within 1 seconds"), "{refused}");
    assert!(!dir.path().join("leases").exists());
}

#[test]
fn the_rolling_config_issue_carries_the_class_line_dispatch_needs_to_staff_a_bug() {
    let body = super::advisory::body("whiterabbit", &["policy-contract: drift".into()]);
    assert_eq!(crate::router::brief::class_refusal(&body), None, "{body}");
}
