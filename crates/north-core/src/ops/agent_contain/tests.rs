use std::fs;
use std::path::{Path, PathBuf};

use regex::Regex;

use super::{Inputs, sweep};

const GOLDEN: &str = include_str!("fixtures/sweep.golden");
const USER: &str = "/user.slice/user-1000.slice/user@1000.service";
const LEAD: &str = "/app.slice/app-spawn-quiet-lead-1.scope";

struct Scenario {
    root: tempfile::TempDir,
    inputs: Inputs,
}

fn add(proc: &Path, pid: u32, comm: &str, ppid: u32, group: &str, cmd: &str) {
    let dir = proc.join(pid.to_string());
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("stat"), format!("{pid} ({comm}) S {ppid} 1 1 0\n")).unwrap();
    fs::write(dir.join("cgroup"), format!("0::{group}\n")).unwrap();
    fs::write(dir.join("cmdline"), cmd.split(' ').map(|word| format!("{word}\0")).collect::<String>()).unwrap();
}

impl Scenario {
    fn new(busctl_body: &str, quirk: bool) -> Self {
        let root = tempfile::tempdir().unwrap();
        let base = root.path();
        let (proc, cgroup) = (base.join("proc"), base.join("cgroup"));
        let (lead, pod) = (format!("{USER}{LEAD}"), format!("{USER}/user.slice/libpod-abc.scope"));
        let farm = format!("{USER}/agent.slice/agent-capacity.slice/agent-capacity-farm.slice/libpod-def.scope");
        add(&proc, 1, "systemd", 0, "/init.scope", "systemd");
        add(&proc, 100, "ghostty", 1, &format!("{USER}/app.slice/app-ghostty-1.scope"), "ghostty");
        add(&proc, 101, "bash", 100, &format!("{USER}/app.slice/app-ghostty-1.scope"), "bash");
        add(&proc, 200, "claude", 1, &lead, "claude");
        add(&proc, 201, "bash", 200, &lead, "bash -c bun test");
        add(&proc, 202, "bun", 201, &lead, "bun test");
        add(&proc, 300, "codex", 1, &format!("{USER}/agent.slice/run-p300.scope"), "codex");
        add(&proc, 301, "bash", 300, &format!("{USER}/agent.slice/run-p300.scope"), "bash -c make");
        add(&proc, 302, "wine", 202, &format!("{USER}/native.slice/agent-capacity-p302.scope"), "wine Warcraft.exe");
        add(&proc, 400, "conmon", 1, &pod, "conmon");
        add(&proc, 401, "lua", 400, &format!("{pod}/container"), "lua perf.lua");
        add(&proc, 500, "conmon", 1, &farm, "conmon");
        add(&proc, 501, "lua", 500, &format!("{farm}/container"), "lua perf.lua");
        if quirk {
            add(&proc, 600, "sh", 200, &format!("{USER}/app.slice/it's.scope"), "sh -c echo it's");
        }
        for scope in [&pod, &farm] {
            let dir = cgroup.join(scope.trim_start_matches('/'));
            fs::create_dir_all(&dir).unwrap();
            fs::write(dir.join("cgroup.freeze"), "0\n").unwrap();
        }
        let busctl = base.join("busctl");
        let body = busctl_body.replace("PROC", &proc.display().to_string()).replace("LOG", &base.join("busctl.log").display().to_string());
        crate::foundation::shim::write_script(&busctl, &body);
        let inputs = Inputs {
            proc,
            cgroup,
            inbox: base.join("inbox.md"),
            uid: 1000,
            host: "testhost".into(),
            busctl: busctl.display().to_string(),
            state: base.join("reported.json"),
            now: 1_792_304_040.0,
        };
        Self { root, inputs }
    }

    fn recording() -> Self {
        Self::new("echo \"$*\" >> LOG\n", false)
    }

    fn sweep(&self, dry: bool) -> String {
        let mut out = Vec::new();
        assert_eq!(sweep(&self.inputs, dry, &mut out, &mut || {}), 0);
        String::from_utf8(out).unwrap()
    }

    fn pod_already_frozen(&self) {
        let pod = self.inputs.cgroup.join(format!("{USER}/user.slice/libpod-abc.scope").trim_start_matches('/')).join("cgroup.freeze");
        fs::write(pod, "1\n").unwrap();
    }

    fn log(&self) -> String {
        fs::read_to_string(self.root.path().join("busctl.log")).unwrap_or_default()
    }

    fn inbox(&self) -> String {
        fs::read_to_string(&self.inputs.inbox).unwrap_or_default()
    }

    fn freeze(&self, scope: &str) -> String {
        let path: PathBuf = self.inputs.cgroup.join(format!("{USER}{scope}").trim_start_matches('/')).join("cgroup.freeze");
        fs::read_to_string(path).unwrap().trim().to_owned()
    }
}

fn masked(text: &str) -> String {
    Regex::new(r"(?m)^INCIDENT \S+ ").unwrap().replace_all(text, "INCIDENT STAMP ").into_owned()
}

#[test]
fn old_and_new_agree_on_the_recorded_plan_and_incident_lines() {
    let (plan, incidents) = GOLDEN.split_once("---\n").unwrap();
    let scenario = Scenario::new("exit 0\n", true);
    assert_eq!(scenario.sweep(true), plan);
    assert_eq!(masked(&scenario.sweep(false)), incidents);
}

#[test]
fn a_claude_child_outside_agent_slice_moves_and_a_container_scope_outside_it_freezes() {
    let scenario = Scenario::recording();
    let plan = scenario.sweep(true);
    let heads: Vec<String> = plan.lines().map(|line| line.split(' ').take(2).collect::<Vec<_>>().join(" ")).collect();
    assert_eq!(heads, ["freeze 400", "move 201", "move 202"]);
    scenario.sweep(false);
    assert_eq!(scenario.freeze("/user.slice/libpod-abc.scope"), "1");
    assert_eq!(scenario.freeze("/agent.slice/agent-capacity.slice/agent-capacity-farm.slice/libpod-def.scope"), "0");
    let moved = Regex::new(r"StartTransientUnit ssa\(sv\)a\(sa\(sv\)\) agent-contain-201-[0-9]+\.scope fail 2 PIDs au 2 201 202 Slice s agent-capacity.slice 0").unwrap();
    assert_eq!(moved.find_iter(&scenario.log()).count(), 1);
    let inbox = scenario.inbox();
    assert_eq!(inbox.lines().filter(|line| line.contains(&format!("action=moved pid=201 cmd='bash -c bun test' cgroup={USER}{LEAD} processes=2"))).count(), 1);
    assert_eq!(inbox.lines().filter(|line| line.contains("action=frozen pid=400 cmd='conmon'")).count(), 1);
}

#[test]
fn an_already_frozen_container_and_a_reported_cgroup_are_not_reported_again_within_the_hour() {
    let scenario = Scenario::recording();
    scenario.sweep(false);
    scenario.sweep(false);
    assert_eq!(scenario.inbox().lines().filter(|line| line.starts_with("INCIDENT")).count(), 2);
    assert_eq!(scenario.log().matches("StartTransientUnit").count(), 2);
}

#[test]
fn a_child_exiting_after_the_scan_leaves_the_live_child_contained_without_a_failure_incident() {
    let scenario = Scenario::recording();
    let proc = scenario.inputs.proc.clone();
    let mut out = Vec::new();
    sweep(&scenario.inputs, false, &mut out, &mut || fs::remove_dir_all(proc.join("201")).unwrap());
    assert_eq!(scenario.inbox().lines().filter(|line| line.contains("action=moved pid=202 ") && line.ends_with(&format!("cgroup={USER}{LEAD}"))).count(), 1);
    assert_eq!(scenario.log().matches("PIDs au 1 202 Slice").count(), 1);
}

#[test]
fn a_failed_move_whose_children_all_exited_produces_no_incident_and_is_not_retried() {
    let scenario = Scenario::new("echo \"$*\" >> LOG\nrm -rf PROC/201 PROC/202\nexit 1\n", false);
    scenario.pod_already_frozen();
    assert_eq!(scenario.sweep(false), "");
    assert_eq!(scenario.log().lines().count(), 1);
}

#[test]
fn a_failed_move_retries_surviving_children_and_reports_their_containment() {
    let scenario = Scenario::new("echo \"$*\" >> LOG\nif [ -d PROC/201 ]; then rm -rf PROC/201; exit 1; fi\nexit 0\n", false);
    scenario.sweep(false);
    assert_eq!(scenario.inbox().lines().filter(|line| line.contains("action=moved pid=202 ") && line.ends_with(&format!("cgroup={USER}{LEAD}"))).count(), 1);
    assert_eq!(scenario.log().matches("PIDs au 1 202 Slice").count(), 1);
}

#[test]
fn a_failed_retry_reports_only_the_live_child_once_per_hour_and_still_retries_each_sweep() {
    let scenario = Scenario::new("echo \"$*\" >> LOG\nrm -rf PROC/201\nexit 1\n", false);
    scenario.pod_already_frozen();
    add(&scenario.inputs.proc, 202, "bun", 200, &format!("{USER}{LEAD}"), "bun test");
    scenario.sweep(false);
    assert_eq!(scenario.inbox().lines().filter(|line| line.contains("action=move-failed pid=202 ") && line.ends_with(&format!("cgroup={USER}{LEAD}"))).count(), 1);
    scenario.sweep(false);
    assert_eq!(scenario.inbox().lines().count(), 1);
    assert_eq!(scenario.log().lines().count(), 4);
}

