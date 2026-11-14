use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, DuplexStream, Lines};

use super::super::model::{LaunchRequest, Machine, ProviderName, Role};
use super::super::prompt::PromptMode;
use super::super::remote::{attached, held_under};
use super::super::spares::{self, Place};
use super::super::runtime::HostSpec;
use super::*;

const WAIT: Duration = Duration::from_secs(20);

/// Stand-ins for ssh and the remote user manager. The fake ssh relays through two FIFOs to the
/// remote command, which runs in its own session as sshd runs it, so killing the relay drops the
/// transport while the remote side lives on; the fake systemd-run starts a detached process group
/// per unit and systemctl answers for it.
fn machine(root: &Path) -> PathBuf {
    let shims = root.join("shims");
    fs::create_dir_all(&shims).unwrap();
    let shim = |name: &str, body: String| {
        let path = shims.join(name);
        crate::foundation::shim::write_script(&path, &format!("SHIMS='{}'\nexport PATH=\"$SHIMS:$PATH\"\n{body}", shims.display()));
    };
    shim(
        "ssh",
        r#"while [ "$1" != -- ]; do shift; done
t=$(mktemp -d "$SHIMS/link.XXXXXX"); mkfifo "$t/i" "$t/o"
exec 3<&0
cat <&3 > "$t/i" & up=$!
cat "$t/o" & down=$!
echo "$$ $up $down" >> "$SHIMS/links"
setsid sh -c "$2" < "$t/i" > "$t/o"
code=$?
kill "$up" 2>/dev/null; wait "$down"
exit "$code"
"#
        .into(),
    );
    shim(
        "systemd-run",
        r#"for a; do case $a in --unit=*) unit=${a#--unit=};; --) break;; esac; done
while [ "$1" != -- ]; do case $1 in --setenv=*) export "${1#--setenv=}";; esac; shift; done
shift
setsid "$@" < /dev/null > /dev/null 2>&1 &
echo $! > "$SHIMS/unit-$unit"
"#
        .into(),
    );
    shim(
        "systemctl",
        r#"for a; do unit=$a; done
pid=$(cat "$SHIMS/unit-$unit" 2>/dev/null)
case "$*" in
  *stop*) [ -n "$pid" ] && kill -s TERM -- "-$pid" 2>/dev/null; exit 0;;
  *show*) if [ -n "$pid" ] && kill -0 "$pid" 2>/dev/null; then echo "$pid"; else echo 0; fi;;
  *is-active*) [ -n "$pid" ] && kill -0 "$pid" 2>/dev/null;;
esac
"#
        .into(),
    );
    shims
}

fn held(root: &Path, argv: &[&str], env: Vec<(String, std::ffi::OsString)>) -> Link {
    let shims = machine(root);
    let spec = HostSpec { key: "box one".into(), shared: false, argv: argv.iter().map(|word| (*word).to_owned()).collect(), env, env_remove: Vec::new(), cwd: root.to_path_buf() };
    let placed = held_under(spec, &Machine { name: "far".into(), ssh: "far".into(), agent_ssh: None }, root, "box one", &root.join("hops.jsonl")).unwrap();
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

fn alive(pid: &str) -> bool {
    fs::read_to_string(Path::new("/proc").join(pid).join("stat")).is_ok_and(|stat| stat.rsplit_once(") ").is_some_and(|(_, rest)| !rest.starts_with('Z')))
}

fn holding(held: &Path) -> Vec<String> {
    let needle = held.to_string_lossy().into_owned();
    let pids = fs::read_dir("/proc").into_iter().flatten().flatten().filter_map(|entry| entry.file_name().to_str().filter(|name| name.chars().all(|c| c.is_ascii_digit())).map(str::to_owned));
    pids.filter(|pid| alive(pid) && fs::read(Path::new("/proc").join(pid).join("cmdline")).is_ok_and(|line| String::from_utf8_lossy(&line).contains(&needle))).collect()
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
    let (ssh, units) = (root.path().join("plain-ssh"), [north_session_host::held::unit("box one")]);
    crate::foundation::shim::write_script(&ssh, &format!("while [ \"$1\" != -- ]; do shift; done\nPATH='{}':$PATH exec sh -c \"$2\"", root.path().join("shims").display()));
    let work = || north_session_host::held::work(&ssh, "tom@far", &units).unwrap()[&units[0]];
    assert_eq!(work(), 0, "an idle CLI has no work below it");
    to_link.write_all(b"wait\n").await.unwrap();
    assert!(north_base::platform::wait::until_async(WAIT, || waiting.exists()).await, "the CLI never took the wait");
    assert!(work() > 0, "the reaper sees the background wait the CLI runs on its machine (north#770)");
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
async fn a_session_held_on_another_machine_links_from_here_and_takes_a_spare_there_only_when_one_fits_and_under_an_agent_login_holds_under_that_users_home() {
    let root = tempfile::tempdir().unwrap();
    let lane = Path::new("/home/tom/code/north/worktrees/only-on-far");
    let spec = HostSpec { key: "box one".into(), shared: false, argv: vec!["opencode".into()], env: Vec::new(), env_remove: Vec::new(), cwd: lane.to_path_buf() };
    let placed = held_under(spec, &Machine { name: "far".into(), ssh: "far".into(), agent_ssh: None }, root.path(), "box one", &root.path().join("hops.jsonl")).unwrap();
    assert_eq!(placed.cwd, root.path());
    assert!(Link::parse(&placed.argv[2..]).unwrap().launch.contains(&format!("cd {}", quote(&lane.to_string_lossy()))));
    let first = Link::parse(&placed.argv[2..]).unwrap();
    let launched = &first.launch[..first.launch.find(" && systemd-run").unwrap()];
    let prepare = &launched[launched.find("; mkdir -p ").unwrap() + 2..launched.rfind(" && cd ").unwrap()];
    let bound = Path::new(&first.dir).join(north_base::platform::session_host_files::BOUND_CONVERSATION);
    fs::create_dir_all(&first.dir).unwrap();
    fs::write(&bound, "c0ffee-bound").unwrap();
    let ran = std::process::Command::new("sh").args(["-c", prepare]).output().unwrap();
    assert!(ran.status.success(), "{prepare}: {}", String::from_utf8_lossy(&ran.stderr));
    assert_eq!(fs::read_to_string(&bound).unwrap(), "c0ffee-bound", "starting the held unit again removed the conversation its record mirror had just bound it to (gex45, sb-box-trailer, north#1352)");
    let agent = Machine { name: "far".into(), ssh: "tom@far".into(), agent_ssh: Some("agent@far".into()) };
    let spec = HostSpec { key: "box one".into(), shared: false, argv: vec!["claude".into()], env: Vec::new(), env_remove: Vec::new(), cwd: root.path().to_path_buf() };
    let placed = held_under(spec, &agent, root.path(), "box one", &root.path().join("hops.jsonl")).unwrap();
    let held_on_agent = Link::parse(&placed.argv[2..]).unwrap().dir;
    assert!(held_on_agent.starts_with("/home/agent/.local/state/north/held/"), "{held_on_agent}");
    let (ssh, sent) = (root.path().join("ssh"), root.path().join("sent"));
    crate::foundation::shim::write_script(&ssh, &format!("cat >/dev/null\nfor word; do script=$word; done\nprintf %s \"$script\" > '{}'\n", sent.display()));
    let record: crate::fleet::model::SessionRecord = serde_json::from_value(serde_json::json!({
        "name": "agent-first-turn", "provider": "claude", "account": "e", "cwd": "/home/agent/code/north/main", "model": null, "created_ms": 0,
        "conversation": "conversation-one", "machine": {"name": "cpx62", "ssh": "tom@cpx62", "agent_ssh": "agent@cpx62"}
    }))
    .unwrap();
    super::super::remote::mirror_through(&ssh, &record, Path::new("/home/tom/.local/state/north/fleet/placed"), Path::new("/home/tom")).unwrap();
    let script = fs::read_to_string(&sent).unwrap();
    assert!(script.contains("/home/agent/.local/state/north/fleet/placed/conversation-one/"), "the record is not written under the agent's fleet root: {script}");
    assert!(script.contains("/home/agent/.local/state/north/held/north-fleet-agent-first-turn-"), "the bound conversation is not written in the agent's held unit: {script}");
    assert!(!script.contains("/home/tom"), "the agent login was sent a path under the operator's home: {script}");

    let shims = machine(root.path());
    let (ssh, far, home) = (shims.join("ssh"), Machine { name: "far".into(), ssh: "far".into(), agent_ssh: None }, root.path());
    let place = Place { ssh: &ssh, machine: &far, home };
    let cli = r#"while read -r line; do echo "got $line as $(tr "\n" " " < "$NORTH_FLEET_BINDING")"; done"#;
    let spec = HostSpec { key: "spare-one".into(), shared: false, argv: vec!["sh".into(), "-c".into(), cli.into()], env: Vec::new(), env_remove: Vec::new(), cwd: home.to_path_buf() };
    let spools = home.join(".local/state/north/fleet/spool/sessions");
    fs::create_dir_all(spools.join("spare-one")).unwrap();
    fs::write(spools.join("spare-one/left-by-the-spare"), "").unwrap();
    spares::start(&place, &spares::spare_spec(spec, &place, "spare-one"), "kind", "fp-one", "spare-one", "conversation-one").unwrap();
    assert_eq!(spares::take(&place, "fp-two", "box-two", &spares::binding("box-two", None)), None, "a launch whose spec fits no spare starts its own client");
    let taken = spares::take(&place, "fp-one", "box-two", &spares::binding("box-two", None)).expect("the launch takes the spare that fits it");
    assert_eq!(taken.conversation, "conversation-one");
    assert_eq!(spares::take(&place, "fp-one", "box-three", &spares::binding("box-three", None)), None, "a second take of the one spare loses the race and its launch starts cold");
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

    let worker = LaunchRequest { verification: false, gates_aside: false, urgent: false, container_shell: false, container_dir: None, name: "box-one".into(), provider: ProviderName::new("claude"), account: "a".into(), cwd: lane.to_path_buf(), model: None, effort: None, prompt: "Item: north#1 brief".into(), system_prompt: Some("/tmp/box-one-prompt.md".into()), foreground_cap_secs: None, prompt_mode: PromptMode::North, idle_exit_minutes: None, parent: Some("lead".into()), task_ref: Some("north#1".into()), claim_box: Some("the box".into()), placement: None, machine: Some(far.clone()), capacity_need: Some("4".into()), role: Some(Role::Worker), resume: None, compact_window: Some(200_000), dispatched: false, route: None };
    let template = spares::template(worker.clone(), "spare-two".into());
    assert_eq!(template, LaunchRequest { name: "spare-two".into(), prompt: String::new(), system_prompt: None, parent: None, task_ref: None, claim_box: None, capacity_need: None, compact_window: None, ..worker }, "a fill's template carries no task, box, brief or prompt file of the worker it was modelled on");
}

#[test]
fn a_session_on_an_agent_login_stages_in_that_logins_own_checkout_north_1041() {
    let root = tempfile::tempdir().unwrap();
    let agent = Machine { name: "far".into(), ssh: "tom@far".into(), agent_ssh: Some("agent@far".into()) };
    let operator_checkout = Path::new("/home/tom/code/north/main");
    let spec = HostSpec { key: "box one".into(), shared: false, argv: vec!["claude".into()], env: Vec::new(), env_remove: Vec::new(), cwd: operator_checkout.to_path_buf() };
    let placed = held_under(spec, &agent, Path::new("/home/tom"), "box one", &root.path().join("hops.jsonl")).unwrap();
    let launch = Link::parse(&placed.argv[2..]).unwrap().launch;
    assert!(launch.contains(&format!("cd {}", quote("/home/agent/code/north/main"))), "the session did not stage in its agent login's own checkout: {launch}");
    assert!(!launch.contains("/home/tom"), "the agent login was sent a path under the operator's home: {launch}");
}

fn spare_in(place: &Place, cwd: &Path, name: &str) {
    let spec = HostSpec { key: name.into(), shared: false, argv: vec!["sleep".into(), "600".into()], env: Vec::new(), env_remove: Vec::new(), cwd: cwd.to_path_buf() };
    spares::start(place, &spares::spare_spec(spec, place, name), "kind", "fp", name, &format!("conversation-{name}")).unwrap();
}

fn pooled(place: &Place) -> Vec<String> {
    spares::survey(place).unwrap().entries.into_iter().map(|entry| entry.rsplit('-').next().unwrap().to_owned()).collect()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_spare_whose_directory_was_removed_is_retired_by_the_next_survey_and_its_slot_counts_free() {
    let root = tempfile::tempdir().unwrap();
    let shims = machine(root.path());
    let (far, home) = (Machine { name: "far".into(), ssh: "far".into(), agent_ssh: None }, root.path());
    let place = Place { ssh: &shims.join("ssh"), machine: &far, home };
    let (kept, gone) = (home.join("lanes/kept"), home.join("lanes/gone"));
    fs::create_dir_all(&kept).unwrap();
    fs::create_dir_all(&gone).unwrap();
    spare_in(&place, &kept, "spare-kept");
    spare_in(&place, &gone, "spare-gone");
    assert_eq!(pooled(&place).len(), 2);

    fs::remove_dir_all(&gone).unwrap();
    assert_eq!(pooled(&place), vec!["kept".to_owned()], "the survey retires the spare whose directory is gone, so it fills no slot");
    let log = fs::read_to_string(home.join(".local/state/north/spares-retired.log")).unwrap();
    assert!(log.lines().count() == 1 && log.contains("spare-gone age ") && log.contains("retired unused"), "one line names the spare, its age and why: {log}");

    fs::remove_dir_all(&kept).unwrap();
    fs::create_dir_all(&kept).unwrap();
    assert_eq!(pooled(&place), Vec::<String>::new(), "a directory made again under the same name is not the worktree the spare started in");
    let quiet = spares::Pool { load: 0, threads: 8, memory_kb: u64::MAX, ..spares::survey(&place).unwrap() };
    assert_eq!(spares::wanted(&quiet, "kind", "fp").1, spares::PER_KIND, "the retired spares' slots are free");
    let held = home.join(".local/state/north/held");
    north_base::platform::wait::until_async(WAIT, || holding(&held).is_empty()).await;
    assert_eq!(holding(&held), Vec::<String>::new(), "retiring leaves no process of the spare behind");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn retiring_a_directory_tree_stops_every_spare_under_it_and_none_outside() {
    let root = tempfile::tempdir().unwrap();
    let shims = machine(root.path());
    let (far, home) = (Machine { name: "far".into(), ssh: "far".into(), agent_ssh: None }, root.path());
    let place = Place { ssh: &shims.join("ssh"), machine: &far, home };
    let tree = home.join("eval/worktrees");
    let (one, two, outside) = (tree.join("one"), tree.join("two"), home.join("eval/worktrees-other"));
    for dir in [&one, &two, &outside] {
        fs::create_dir_all(dir).unwrap();
    }
    spare_in(&place, &one, "spare-one");
    spare_in(&place, &two, "spare-two");
    spare_in(&place, &outside, "spare-out");

    assert_eq!(spares::retire_under(&place, &home.join("empty")), Some(Vec::new()), "a tree with no spare retires nothing");
    let lines = spares::retire_under(&place, &tree).unwrap();
    assert_eq!(lines.len(), 2, "{lines:?}");
    assert!(lines.iter().all(|line| line.contains("age ") && line.contains(&format!("directory tree {} was retired", tree.display()))), "{lines:?}");
    assert_eq!(pooled(&place), vec!["out".to_owned()], "the spare outside the tree stays");
    assert_eq!(spares::retire_under(&place, Path::new("relative")), None);
    let of = |spare: &str| super::super::remote::held_dir(home, &format!("north-{spare}.service"));
    north_base::platform::wait::until_async(WAIT, || holding(&of("spare-one")).is_empty() && holding(&of("spare-two")).is_empty()).await;
    assert!(holding(&of("spare-one")).is_empty() && holding(&of("spare-two")).is_empty(), "no process of a retired spare remains");
    assert!(!holding(&of("spare-out")).is_empty(), "the outside spare still runs");
}
