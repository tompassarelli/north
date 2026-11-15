use std::fs;
use std::path::{Path, PathBuf};

use regex::Regex;

use super::{Inputs, sweep};

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

