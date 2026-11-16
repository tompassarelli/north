use std::fs;
use std::path::Path;
use std::time::Duration;

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, DuplexStream, Lines};

use super::super::model::{LaunchRequest, Machine, ProviderName, Role};
use super::super::prompt::PromptMode;
use super::super::remote::{attached, held_under};
use north_warm_spares::fake_host::{alive, holding, machine};
use north_warm_spares::spares::{self, Place};
use super::super::runtime::HostSpec;
use super::*;

const WAIT: Duration = Duration::from_secs(20);

fn held(root: &Path, argv: &[&str], env: Vec<(String, std::ffi::OsString)>) -> Link {
    let shims = machine(root);
    let spec = HostSpec { key: "box one".into(), shared: false, argv: argv.iter().map(|word| (*word).to_owned()).collect(), env, env_remove: Vec::new(), cwd: root.to_path_buf() };
    let placed = held_under(spec, None, &Machine { name: "far".into(), ssh: "far".into(), agent_ssh: None }, root, "box one", &root.join("hops.jsonl")).unwrap();
    assert_eq!(placed.argv[1], "session-link");
    Link { ssh: shims.join("ssh"), ..Link::parse(&placed.argv[2..]).unwrap() }
}

async fn next(lines: &mut Lines<BufReader<DuplexStream>>) -> String {
    tokio::time::timeout(WAIT, lines.next_line()).await.unwrap().unwrap().unwrap()
}

fn chunks(source: DuplexStream) -> mpsc::UnboundedReceiver<Vec<u8>> {
    let (sender, receiver) = mpsc::unbounded_channel();
    tokio::spawn(read_chunks(source, sender));
    receiver
}

fn drop_transport(shims: &Path) {
    let links = fs::read_to_string(shims.join("links")).unwrap();
    for pid in links.lines().last().unwrap().split_whitespace() {
        let _ = std::process::Command::new("kill").args(["-9", pid]).status();
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_held_session_survives_its_transport_dying_mid_wait() {
    let root = tempfile::tempdir().unwrap();
    let waiting = root.path().join("waiting");
    let release = root.path().join("release");
    assert!(std::process::Command::new("mkfifo").arg(&release).status().unwrap().success());
    let cli = format!(r#"while read -r line; do case $line in wait) (read -r _ < {}; echo "waited $$") & : > {} ;; *) echo "got $line $$";; esac; done"#, release.display(), waiting.display());
    let link = held(root.path(), &["sh", "-c", &cli], Vec::new());
    let (mut to_link, input) = tokio::io::duplex(4096);
    let (mut output, from_link) = tokio::io::duplex(4096);
    let relay = tokio::spawn(async move { link.relay(chunks(input), &mut output, std::future::pending()).await });
    let mut lines = BufReader::new(from_link).lines();

    to_link.write_all(b"one\n").await.unwrap();
    let first = next(&mut lines).await;
    let cli_pid = first.strip_prefix("got one ").unwrap().to_owned();
    assert_eq!(fs::read_to_string(root.path().join("shims/links")).unwrap().lines().count(), 1, "one ssh command started the held unit and attached to it");
    let (ssh, units) = (root.path().join("plain-ssh"), [north_session_host::held::unit("box one")]);
    north_foundation::foundation::shim::write_script(&ssh, &format!("while [ \"$1\" != -- ]; do shift; done\nPATH='{}':$PATH exec sh -c \"$2\"", root.path().join("shims").display()));
    let work = || north_session_host::held::work(&ssh, "tom@far", &units).unwrap()[&units[0]];
    assert_eq!(work(), north_session_host::held::Work::Below(0), "an idle CLI has no work below it");
    to_link.write_all(b"wait\n").await.unwrap();
    assert!(north_base::platform::wait::until_async(WAIT, || waiting.exists()).await, "the CLI never took the wait");
    assert!(matches!(work(), north_session_host::held::Work::Below(below) if below > 0), "the reaper sees the background wait the CLI runs on its machine (north#770)");
    drop_transport(&root.path().join("shims"));
    to_link.write_all(b"two\n").await.unwrap();

    let mut after = vec![next(&mut lines).await];
    tokio::task::spawn_blocking(move || fs::write(release, "go\n")).await.unwrap().unwrap();
    after.push(next(&mut lines).await);
    after.sort();
    assert_eq!(after, [format!("got two {cli_pid}"), format!("waited {cli_pid}")], "the background wait and the input sent while the link was down both reach the same CLI, once each");
    let links = fs::read_to_string(root.path().join("shims/links")).unwrap();
    assert!(links.lines().count() >= 2, "the link reconnected: {links}");
    assert!(alive(&cli_pid));

    drop(to_link);
    assert_eq!(tokio::time::timeout(WAIT, relay).await.unwrap().unwrap().unwrap(), 0);
    north_base::platform::wait::until_async(WAIT, || !alive(&cli_pid)).await;
    assert!(!alive(&cli_pid), "closing the host's input stops the held service");
    let held = root.path().join(".local/state/north/held");
    north_base::platform::wait::until_async(WAIT, || holding(&held).is_empty()).await;
    assert_eq!(holding(&held), Vec::<String>::new(), "no process of the held service outlives its CLI, so a test run leaks none into the session that ran it");
}


#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_session_held_on_another_machine_links_from_here_reads_it_through_the_standing_master_and_takes_a_spare_there_only_when_one_fits_and_under_an_agent_login_holds_under_that_users_home() {
    let root = tempfile::tempdir().unwrap();
    let lane = Path::new("/home/tom/code/north/worktrees/only-on-far");
    let spec = HostSpec { key: "box one".into(), shared: false, argv: vec!["opencode".into()], env: Vec::new(), env_remove: Vec::new(), cwd: lane.to_path_buf() };
    let placed = held_under(spec, None, &Machine { name: "far".into(), ssh: "far".into(), agent_ssh: None }, root.path(), "box one", &root.path().join("hops.jsonl")).unwrap();
    assert_eq!(placed.cwd, root.path());
    assert!(Link::parse(&placed.argv[2..]).unwrap().launch.contains(&format!("cd {}", quote(&lane.to_string_lossy()))));
    let first = Link::parse(&placed.argv[2..]).unwrap();
    assert!(first.launch.ends_with(&format!("{} {} {} {}", quote(&first.dir), quote(&first.unit), super::super::remote::UNREGISTERED.as_secs(), quote("opencode"))), "a launch's held unit outlives a link gone before its session registers by the 12 hours a registered one keeps, so 19 dispatch launches that never registered left their units running on cpx62 (2026-11-15): {}", first.launch);
    let launched = &first.launch[..first.launch.find(" && systemd-run").unwrap()];
    let prepare = &launched[launched.find("; mkdir -p ").unwrap() + 2..launched.rfind(" && cd ").unwrap()];
    let bound = Path::new(&first.dir).join(north_base::platform::session_host_files::BOUND_CONVERSATION);
    fs::create_dir_all(&first.dir).unwrap();
    fs::write(&bound, "c0ffee-earlier").unwrap();
    let ran = std::process::Command::new("sh").args(["-c", prepare]).output().unwrap();
    assert!(ran.status.success(), "{prepare}: {}", String::from_utf8_lossy(&ran.stderr));
    assert!(!bound.exists(), "a fresh session that reuses an earlier one's name started bound to that session's conversation, and so inside its finish scope (north#1352)");
    let fresh = HostSpec { key: "box two".into(), shared: false, argv: vec!["codex".into()], env: Vec::new(), env_remove: Vec::new(), cwd: lane.to_path_buf() };
    let started = Link::parse(&held_under(fresh, Some("c0ffee-fresh"), &Machine { name: "far".into(), ssh: "far".into(), agent_ssh: None }, root.path(), "box two", &root.path().join("hops.jsonl")).unwrap().argv[2..]).unwrap();
    let launched = &started.launch[..started.launch.find(" && systemd-run").unwrap()];
    let ran = std::process::Command::new("sh").args(["-c", &launched[launched.find("; mkdir -p ").unwrap() + 2..launched.rfind(" && cd ").unwrap()]]).output().unwrap();
    assert!(ran.status.success(), "{}", String::from_utf8_lossy(&ran.stderr));
    assert_eq!(fs::read_to_string(Path::new(&started.dir).join(north_base::platform::session_host_files::BOUND_CONVERSATION)).unwrap(), "c0ffee-fresh", "the script that starts a held unit left it unbound until its record is mirrored, which can come first and find no directory (evalb sessions on cpx62b, north#1352)");
    let agent = Machine { name: "far".into(), ssh: "tom@far".into(), agent_ssh: Some("agent@far".into()) };
    let spec = HostSpec { key: "box one".into(), shared: false, argv: vec!["claude".into()], env: Vec::new(), env_remove: Vec::new(), cwd: root.path().to_path_buf() };
    let placed = held_under(spec, None, &agent, root.path(), "box one", &root.path().join("hops.jsonl")).unwrap();
    let held_on_agent = Link::parse(&placed.argv[2..]).unwrap().dir;
    assert!(held_on_agent.starts_with("/home/agent/.local/state/north/held/"), "{held_on_agent}");
    let (ssh, sent) = (root.path().join("ssh"), root.path().join("sent"));
    north_foundation::foundation::shim::write_script(&ssh, &format!("cat > '{}'\nfor word; do script=$word; done\nprintf %s \"$script\" > '{}'\n", root.path().join("record").display(), sent.display()));
    let record: crate::fleet::model::SessionRecord = serde_json::from_value(serde_json::json!({
        "name": "agent-first-turn", "provider": "claude", "account": "e", "cwd": "/home/tom/code/north/main", "model": null, "created_ms": 0,
        "conversation": "conversation-one", "machine": {"name": "cpx62", "ssh": "tom@cpx62", "agent_ssh": "agent@cpx62"}
    }))
    .unwrap();
    super::super::remote::mirror_through(&ssh, &record, Path::new("/home/tom/.local/state/north/fleet/placed"), Path::new("/home/tom")).unwrap();
    let script = fs::read_to_string(&sent).unwrap();
    assert!(script.contains("/home/agent/.local/state/north/fleet/placed/conversation-one/"), "the record is not written under the agent's fleet root: {script}");
    assert!(script.contains("/home/agent/.local/state/north/held/north-fleet-agent-first-turn-"), "the bound conversation is not written in the agent's held unit: {script}");
    assert!(!script.contains("/home/tom"), "the agent login was sent a path under the operator's home: {script}");
    assert!(script.ends_with(&format!("echo {} > '{}/limit'; }}; }}", super::super::remote::DETACHED.as_secs(), super::super::remote::HeldOn::of(&record, None, Path::new("/home/tom")).unwrap().dir.display())), "only a mirrored record lets the held unit outlive its link by the registered limit: {script}");
    let mirrored: serde_json::Value = serde_json::from_slice(&fs::read(root.path().join("record")).unwrap()).unwrap();
    assert_eq!(mirrored["cwd"], "/home/agent/code/north/main", "the record mirrored to an agent login named the operator's checkout, which the host's finish broker serves no project of (north#1508)");
    let (runtime, said) = (root.path().join("runtime"), root.path().join("said"));
    fs::create_dir_all(runtime.join("north-links")).unwrap();
    let _standing = std::os::unix::net::UnixListener::bind(runtime.join("north-links/agent_far")).unwrap();
    let master = standing_master_under(&runtime, &ssh, "agent@far");
    assert_eq!(master, runtime.join("north-links/agent_far"), "a launch read names the master its link rides");
    north_foundation::foundation::shim::write_script(&ssh, &format!("printf '%s\\n' \"$@\" > '{}'\necho answered\n", said.display()));
    assert!(matches!(super::super::remote::answer_through(&ssh, Some(&master), &agent, "true"), crate::fleet::runtime::Seen::Text(text) if text == "answered\n"));
    let words = fs::read_to_string(&said).unwrap();
    assert!(words.contains(&format!("ControlMaster=no\n-S\n{}\nagent@far\n", master.display())), "a launch read of a build host opened its own connection instead of a channel of the standing master (north#1054): {words}");

    let shims = machine(root.path());
    let (ssh, far, home) = (shims.join("ssh"), Machine { name: "far".into(), ssh: "far".into(), agent_ssh: None }, root.path());
    let place = Place { ssh: &ssh, machine: &far, home };
    let cli = r#"while read -r line; do echo "got $line as $(tr "\n" " " < "$NORTH_FLEET_BINDING")"; done"#;
    let spec = HostSpec { key: "spare-one".into(), shared: false, argv: vec!["sh".into(), "-c".into(), cli.into()], env: Vec::new(), env_remove: Vec::new(), cwd: home.to_path_buf() };
    let spools = home.join(".local/state/north/fleet/spool/sessions");
    fs::create_dir_all(spools.join("spare-one")).unwrap();
    fs::write(spools.join("spare-one/left-by-the-spare"), "").unwrap();
    spares::start(&place, &spares::spare_spec(spec, &place, "spare-one"), "kind", "fp-one", "spare-one", "conversation-one").unwrap();
    assert_eq!(spares::take(&place, "fp-two", "box-two", home, &spares::binding("box-two", None), ":"), None, "a launch whose spec fits no spare starts its own client");
    let taken = spares::take(&place, "fp-one", "box-two", home, &spares::binding("box-two", None), ":").expect("the launch takes the spare that fits it");
    assert_eq!(taken.conversation, "conversation-one");
    assert_eq!(spares::take(&place, "fp-one", "box-three", home, &spares::binding("box-three", None), ":"), None, "a second take of the one spare loses the race and its launch starts cold");
    assert!(spools.join("box-two").is_dir() && !spools.join("spare-one").exists(), "the session takes the spare's spool, the directory its tool server may write");
    assert_eq!(fs::read_dir(spools.join("box-two")).unwrap().count(), 0, "the session's spool holds nothing the spare left in it");
    let bound = Path::new(&taken.running.dir).join("bound");
    for (file, text) in [("binding", "NORTH_FLEET_SESSION=other\n"), ("forged", "")] {
        assert!(fs::write(bound.join(file), text).is_err(), "the worker's file tools cannot rewrite or add to the binding a take sealed: {file}");
    }

    let host = HostSpec { key: "box-two".into(), shared: false, argv: Vec::new(), env: Vec::new(), env_remove: Vec::new(), cwd: home.to_path_buf() };
    let link = Link { ssh, ..Link::parse(&attached(host, &far, &taken.running, "box-two", &home.join("hops.jsonl")).unwrap().argv[2..]).unwrap() };
    let (mut to_link, input) = tokio::io::duplex(4096);
    let (mut output, from_link) = tokio::io::duplex(4096);
    let relay = tokio::spawn(async move { link.relay(chunks(input), &mut output, std::future::pending()).await });
    to_link.write_all(b"brief\n").await.unwrap();
    assert_eq!(next(&mut BufReader::new(from_link).lines()).await, "got brief as NORTH_FLEET_SESSION=box-two THREADS_ACTOR=box-two ", "the taken client reads its binding by the time the brief reaches it");
    drop(to_link);
    assert_eq!(tokio::time::timeout(WAIT, relay).await.unwrap().unwrap().unwrap(), 0);
    let held = home.join(".local/state/north/held");
    north_base::platform::wait::until_async(WAIT, || holding(&held).is_empty()).await;
    assert_eq!(holding(&held), Vec::<String>::new(), "the taken spare stops with its session");
    fs::set_permissions(&bound, std::os::unix::fs::PermissionsExt::from_mode(0o700)).unwrap();


    let worker = LaunchRequest { verification: false, gates_aside: false, urgent: false, container_shell: false, container_dir: None, name: "box-one".into(), provider: ProviderName::new("claude"), account: "a".into(), cwd: lane.to_path_buf(), model: None, effort: None, prompt: "Item: north#1 brief".into(), system_prompt: Some("/tmp/box-one-prompt.md".into()), foreground_cap_secs: None, prompt_mode: PromptMode::North, idle_exit_minutes: None, parent: Some("lead".into()), task_ref: Some("north#1".into()), claim_box: Some("the box".into()), placement: None, machine: Some(far.clone()), capacity_need: Some("4".into()), role: Some(Role::Worker), resume: None, compact_window: Some(200_000), dispatched: false, surface: Default::default(), route: None };
    let template = worker.clone().spare_template("spare-two".into());
    assert_eq!(template, LaunchRequest { name: "spare-two".into(), prompt: String::new(), system_prompt: None, parent: None, task_ref: None, claim_box: None, capacity_need: None, ..worker }, "a fill's template carries no task, box, brief or prompt file of the worker it was modelled on, and keeps its compaction window, which the client reads at start");
    takes_lane_spares(&shims, &far, home);
}

/// A launch into a fresh lane takes the spare whose lane was made ahead, and no git of the take runs a program a lane names.
fn takes_lane_spares(shims: &Path, far: &Machine, home: &Path) {
    let git = |dir: &Path, args: &[&str]| {
        let ran = std::process::Command::new("git").args(["-c", "user.name=t", "-c", "user.email=t@t", "-c", "commit.gpgsign=false", "-c", "init.defaultBranch=main"]).arg("-C").arg(dir).args(args).output().unwrap();
        assert!(ran.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&ran.stderr));
        String::from_utf8_lossy(&ran.stdout).trim().to_owned()
    };
    let main = home.join("code/eval/main");
    fs::create_dir_all(&main).unwrap();
    git(&main, &["init", "-q"]);
    fs::write(main.join("f"), "base\n").unwrap();
    fs::write(main.join(".gitattributes"), "f filter=m\n").unwrap();
    git(&main, &["add", "f", ".gitattributes"]);
    git(&main, &["commit", "-q", "-m", "base"]);
    let task = home.join("code/eval/worktrees/task-l1");
    git(&main, &["worktree", "add", "-q", "-b", "task-l1", &task.to_string_lossy(), "main"]);
    let marked = |repo: &Path, marker: &Path| {
        fs::create_dir_all(repo).unwrap();
        git(repo, &["init", "-q"]);
        fs::write(repo.join("f"), "other\n").unwrap();
        fs::write(repo.join(".gitattributes"), "f filter=m\n").unwrap();
        git(repo, &["add", "f", ".gitattributes"]);
        git(repo, &["commit", "-q", "-m", "other"]);
        let filter = marker.with_extension("filter");
        north_foundation::foundation::shim::write_script(&filter, &format!("touch '{}'\ncat\n", marker.display()));
        git(repo, &["config", "filter.m.clean", &filter.to_string_lossy()]);
    };
    let (sub, sub_ran) = (task.join("sub"), home.join("sub-ran"));
    marked(&sub, &sub_ran);
    fs::write(sub.join("f"), "changed\n").unwrap();
    let sub_head = git(&sub, &["rev-parse", "HEAD"]);
    git(&task, &["update-index", "--add", "--cacheinfo", &format!("160000,{sub_head},sub")]);
    git(&task, &["commit", "-q", "-m", "gitlink"]);
    git(&main, &["merge", "-q", "--ff-only", "task-l1"]);
    let ssh = shims.join("ssh");
    let place = Place { ssh: &ssh, machine: far, home };
    let ahead = spares::spare_lane(&main, "spare-lane");
    let spec = HostSpec { key: "spare-lane".into(), shared: false, argv: vec!["sh".into(), "-c".into(), "cat > /dev/null".into()], env: Vec::new(), env_remove: Vec::new(), cwd: ahead.clone() };
    spares::start(&place, &spares::spare_spec(spec, &place, "spare-lane"), "kind", "fp-lane", "spare-lane", "conversation-lane").unwrap();
    let other = home.join("other");
    fs::create_dir_all(&other).unwrap();
    git(&other, &["init", "-q"]);
    let (monitor, ran) = (home.join("monitor"), home.join("monitor-ran"));
    north_foundation::foundation::shim::write_script(&monitor, &format!("touch '{}'\n", ran.display()));
    git(&other, &["config", "core.fsmonitor", &monitor.to_string_lossy()]);
    let rogue = home.join("code/eval/worktrees/rogue");
    fs::create_dir_all(&rogue).unwrap();
    fs::write(rogue.join(".git"), format!("gitdir: {}\n", other.join(".git").display())).unwrap();
    assert_eq!(spares::take(&place, "fp-lane", "rogue-run", &rogue, &spares::binding("rogue-run", None), ":"), None, "a lane whose .git names another repository is no fit");
    assert!(!ran.exists(), "no program the other repository's configuration names ran");
    let taken = spares::take(&place, "fp-lane", "task-l1-run", &task, &spares::binding("task-l1-run", None), ":").expect("a launch into a fresh lane takes the spare whose lane was made ahead at the same commit");
    assert_eq!((taken.cwd.as_path(), git(&ahead, &["symbolic-ref", "--short", "HEAD"]).as_str(), task.exists()), (ahead.as_path(), "task-l1", true), "the session runs in the lane made ahead, on the launch's branch, and the launch's own lane stays for its first prompt to be built from");
    spares::remove_left_lane(&place, &task);
    assert!(!task.exists(), "the launch's own lane is gone once its first prompt is built");
    assert!(!sub_ran.exists(), "no program the configuration of a gitlinked repository in the lane names ran");
    spares::release(&place, &taken);

    let task = home.join("code/eval/worktrees/task-l2");
    git(&main, &["worktree", "add", "-q", "-b", "task-l2", &task.to_string_lossy(), "main"]);
    let ahead = spares::spare_lane(&main, "spare-lane-2");
    let spec = HostSpec { key: "spare-lane-2".into(), shared: false, argv: vec!["sh".into(), "-c".into(), "cat > /dev/null".into()], env: Vec::new(), env_remove: Vec::new(), cwd: ahead };
    spares::start(&place, &spares::spare_spec(spec, &place, "spare-lane-2"), "kind", "fp-lane", "spare-lane-2", "conversation-lane-2").unwrap();
    let (swapped, swapped_ran) = (home.join("swapped"), home.join("swapped-ran"));
    marked(&swapped, &swapped_ran);
    let real = String::from_utf8(std::process::Command::new("sh").args(["-c", "command -v git"]).output().unwrap().stdout).unwrap();
    let shim = shims.join("git");
    north_foundation::foundation::shim::write_script(&shim, &format!("case \" $* \" in *\" switch \"*) printf 'gitdir: %s\\n' '{}' > '{}';; esac\nexec '{}' \"$@\"\n", swapped.join(".git").display(), task.join(".git").display(), real.trim()));
    let taken = spares::take(&place, "fp-lane", "task-l2-run", &task, &spares::binding("task-l2-run", None), ":").expect("the second lane fits its spare");
    fs::remove_file(&shim).unwrap();
    assert!(!swapped_ran.exists(), "a lane whose .git is swapped after the fit runs no program of the repository it then names");
    spares::release(&place, &taken);
}

#[test]
fn a_session_on_an_agent_login_stages_in_that_logins_own_checkout_north_1041() {
    let root = tempfile::tempdir().unwrap();
    let agent = Machine { name: "far".into(), ssh: "tom@far".into(), agent_ssh: Some("agent@far".into()) };
    let operator_checkout = Path::new("/home/tom/code/north/main");
    let spec = HostSpec { key: "box one".into(), shared: false, argv: vec!["claude".into()], env: Vec::new(), env_remove: Vec::new(), cwd: operator_checkout.to_path_buf() };
    let placed = held_under(spec, None, &agent, Path::new("/home/tom"), "box one", &root.path().join("hops.jsonl")).unwrap();
    let launch = Link::parse(&placed.argv[2..]).unwrap().launch;
    assert!(launch.contains(&format!("cd {}", quote("/home/agent/code/north/main"))), "the session did not stage in its agent login's own checkout: {launch}");
    assert!(!launch.contains("/home/tom"), "the agent login was sent a path under the operator's home: {launch}");
}
