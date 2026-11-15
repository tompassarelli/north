use super::*;
use super::sync::swap;

fn ticks_past(directory: &std::path::Path, mark: &std::path::Path) {
    let probe = directory.join(".tick");
    let modified = |path: &std::path::Path| fs::symlink_metadata(path).unwrap().modified().unwrap();
    let past = north_base::platform::wait::until(Duration::from_secs(3600), || {
        fs::write(&probe, "").unwrap();
        modified(&probe) > modified(mark)
    });
    fs::remove_file(&probe).unwrap();
    assert!(past, "the file clock never moved past {}", mark.display());
}

#[test]
fn swapping_a_runtime_in_leaves_its_lease_file_so_a_read_only_lander_can_lease_it() {
    let state = tempfile::tempdir().unwrap();
    let layout = Layout { state: state.path().to_path_buf() };
    let sha = "e".repeat(40);
    fs::create_dir_all(layout.runtime(&sha)).unwrap();
    swap(&layout, &layout::link(&sha)).unwrap();
    assert!(layout.runtime(&sha).join("lease").is_file(), "the swap left the runtime without the lease file head-north opens read-only");
}

#[test]
fn collecting_old_runtimes_keeps_the_runtime_the_lander_link_pins() {
    let state = tempfile::tempdir().unwrap();
    let layout = Layout { state: state.path().to_path_buf() };
    let shas: Vec<String> = (0..7).map(|digit| digit.to_string().repeat(40)).collect();
    for sha in &shas {
        fs::create_dir_all(layout.runtime(sha)).unwrap();
        ticks_past(state.path(), &layout.runtime(sha));
    }
    std::os::unix::fs::symlink(layout::link(&shas[0]), layout.lander()).unwrap();
    std::os::unix::fs::symlink(layout::link(&shas[6]), layout.current()).unwrap();
    collect(&layout, &shas[6], None).unwrap();
    assert!(layout.runtime(&shas[0]).is_dir());
    assert!(!layout.runtime(&shas[1]).exists());
}

#[test]
fn collecting_old_runtimes_keeps_the_runtime_a_lander_rollback_would_go_back_to() {
    let state = tempfile::tempdir().unwrap();
    let layout = Layout { state: state.path().to_path_buf() };
    let shas: Vec<String> = (0..7).map(|digit| digit.to_string().repeat(40)).collect();
    for sha in &shas {
        fs::create_dir_all(layout.runtime(sha)).unwrap();
        ticks_past(state.path(), &layout.runtime(sha));
    }
    std::os::unix::fs::symlink(layout::link(&shas[6]), layout.lander()).unwrap();
    std::os::unix::fs::symlink(layout::link(&shas[6]), layout.current()).unwrap();
    fs::write(layout.lander_rollback(), format!("{}\n", layout::link(&shas[0]).display())).unwrap();
    collect(&layout, &shas[6], None).unwrap();
    assert!(layout.runtime(&shas[0]).is_dir());
    assert!(!layout.runtime(&shas[1]).exists());
}

#[test]
fn collecting_old_runtimes_keeps_the_runtime_a_live_session_was_launched_on_and_one_rooting_what_the_agent_link_names_and_drops_the_unused_rest() {
    let state = tempfile::tempdir().unwrap();
    let layout = Layout { state: state.path().to_path_buf() };
    let shas: Vec<String> = (0..8).map(|digit| digit.to_string().repeat(40)).collect();
    let agent = state.path().join("published-agent-runtime");
    for sha in &shas {
        fs::create_dir_all(layout.runtime(sha)).unwrap();
        if shas[1..4].contains(sha) {
            std::os::unix::fs::symlink(&agent, layout.runtime(sha).join("agent-runtime")).unwrap();
        }
        ticks_past(state.path(), &layout.runtime(sha));
    }
    let staging = layout.runtimes().join(format!("{}.staging", "9".repeat(40)));
    fs::create_dir_all(&staging).unwrap();
    std::os::unix::fs::symlink(&agent, staging.join("agent-runtime")).unwrap();
    std::os::unix::fs::symlink(layout::link(&shas[7]), layout.current()).unwrap();
    let proc = tempfile::tempdir().unwrap();
    fs::create_dir_all(proc.path().join("42")).unwrap();
    let north = layout.runtime(&shas[0]).join("north");
    fs::write(proc.path().join("42/cmdline"), format!("claude\0--mcp-config\0{{\"command\":\"{}\"}}\0", north.display())).unwrap();
    collect::collect_beside(&layout, &shas[7], None, proc.path(), &[agent]).unwrap();
    assert!(layout.runtime(&shas[0]).is_dir());
    assert!(layout.runtime(&shas[3]).is_dir(), "the newest runtime rooting the store path the agent link names was collected");
    assert!(!layout.runtime(&shas[1]).exists() && !layout.runtime(&shas[2]).exists(), "every older runtime rooting the same store path was kept too, so no runtime was ever collected");
}

#[test]
fn a_copy_interrupted_over_an_hour_ago_leaves_no_staging_directory_while_a_fresh_one_stays() {
    let state = tempfile::tempdir().unwrap();
    let layout = Layout { state: state.path().to_path_buf() };
    let (stale, fresh) = (format!("{}.staging", "a".repeat(40)), format!("{}.staging", "b".repeat(40)));
    for name in [&stale, &fresh] {
        fs::create_dir_all(layout.runtimes().join(name)).unwrap();
        fs::write(layout.runtimes().join(name).join("north"), "half").unwrap();
    }
    let hours_ago = fs::metadata(layout.runtimes().join(&fresh)).unwrap().modified().unwrap() - Duration::from_secs(2 * 60 * 60);
    File::open(layout.runtimes().join(&stale)).unwrap().set_modified(hours_ago).unwrap();
    collect(&layout, &"c".repeat(40), None).unwrap();
    assert!(!layout.runtimes().join(&stale).exists());
    assert!(layout.runtimes().join(&fresh).exists());
}

#[test]
fn a_host_that_never_answers_is_skipped_at_its_deadline_and_a_second_promotion_exits_at_once() {
    use super::super::deadline::{Reached, each_within};
    let (_never, silent) = std::sync::mpsc::channel::<()>();
    let reached = each_within(vec![
        ("gex45".to_owned(), Duration::from_millis(300), Box::new(move || silent.recv().map_or("gave up", |()| "answered")) as Box<dyn FnOnce() -> &'static str + Send>),
        ("whiterabbit".to_owned(), Duration::from_secs(600), Box::new(|| "promoted")),
    ]);
    assert_eq!(reached, vec![("gex45".to_owned(), Reached::Skipped(Duration::from_millis(300))), ("whiterabbit".to_owned(), Reached::Answered("promoted"))]);
    let state = tempfile::tempdir().unwrap();
    let layout = Layout { state: state.path().to_path_buf() };
    let _holder = named_lock(&layout, PROMOTE_LOCK, FlockOperation::NonBlockingLockExclusive).unwrap().unwrap();
    let mut swapped = false;
    assert_eq!(flight(&layout, &mut || swapped = true).unwrap(), Flight::Joined);
    assert!(!swapped);
}

#[test]
fn an_update_by_a_caller_who_cannot_write_the_agent_link_is_refused_and_a_rollback_that_cannot_move_it_leaves_a_record_until_it_moves() {
    use std::os::unix::fs::PermissionsExt;
    let directory = tempfile::tempdir().unwrap();
    let link = directory.path().join("agent");
    let state = tempfile::tempdir().unwrap();
    let layout = Layout { state: state.path().to_path_buf() };
    let (failed, previous) = ("f".repeat(40), layout.runtime(&"e".repeat(40)));
    fs::create_dir_all(&previous).unwrap();
    let published = directory.path().join("published-agent-runtime");
    std::os::unix::fs::symlink(&published, previous.join("agent-runtime")).unwrap();
    let claude = directory.path().join("published-agent-claude");
    std::os::unix::fs::symlink(&claude, previous.join(north_base::platform::agent_runtime::CLAUDE_LINK)).unwrap();
    fs::set_permissions(directory.path(), fs::Permissions::from_mode(0o500)).unwrap();
    let refused = agent_link::require_writable(&link);
    agent_link::roll_back_at(&layout, Some(&link), &previous, &failed);
    fs::set_permissions(directory.path(), fs::Permissions::from_mode(0o700)).unwrap();
    assert!(refused.unwrap_err().message.contains("cannot write the agent runtime link"));
    assert!(agent_link::require_writable(&link).is_ok());
    let stuck = fs::read_to_string(layout.agent_link_stuck()).expect("a rollback that could not move the agent link left no record of it");
    assert!(stuck.starts_with(&format!("{failed} ")), "{stuck}");
    agent_link::roll_back_at(&layout, Some(&link), &previous, &failed);
    assert_eq!(fs::read_link(&link).unwrap(), published);
    assert_eq!(fs::read_link(north_base::platform::agent_runtime::claude_link(&link)).unwrap(), claude, "the agent's Claude Code stayed on the runtime rolled back from");
    assert_eq!(agent_link::targets_of(&link), [published, claude], "a runtime prune may take the root of what an agent link names");
    assert!(!layout.agent_link_stuck().exists(), "the link moved, yet its stuck record stayed");
}

#[test]
fn a_host_names_its_agent_link_by_variable_else_by_the_first_line_of_a_file_only_its_root_owns_and_naming_none_publishes_and_follows_nothing() {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    let directory = tempfile::tempdir().unwrap();
    let file = directory.path().join("agent-runtime-link");
    let chosen = |env: Option<&str>, owner: u32| agent_link::chosen(env.map(Into::into), &file, owner);
    assert_eq!(chosen(None, 0), None, "no file names nothing");
    fs::write(&file, "/run/agent/runtime\nignored\n").unwrap();
    fs::set_permissions(&file, fs::Permissions::from_mode(0o644)).unwrap();
    let owner = fs::metadata(&file).unwrap().uid();
    assert_eq!(chosen(None, owner), Some("/run/agent/runtime".into()));
    assert_eq!(chosen(Some("/from/env"), owner), Some("/from/env".into()), "the variable wins over the file");
    assert_eq!(chosen(Some(""), owner), Some("/run/agent/runtime".into()), "an empty variable names nothing");
    assert_eq!(chosen(None, owner + 1), None, "a file not owned by the host's root is ignored");
    fs::set_permissions(&file, fs::Permissions::from_mode(0o664)).unwrap();
    assert_eq!(chosen(None, owner), None, "a file the group can write is ignored");
    fs::set_permissions(&file, fs::Permissions::from_mode(0o644)).unwrap();
    for unnamed in ["", "\n/run/agent/runtime\n", "relative/runtime\n"] {
        fs::write(&file, unnamed).unwrap();
        assert_eq!(chosen(None, owner), None, "{unnamed:?} names no absolute link");
    }
    assert!(std::env::var_os(agent_link::ENV).is_none());
    let state = tempfile::tempdir().unwrap();
    assert!(agent_link::guard().is_ok());
    assert!(agent_link::publish(state.path()).unwrap().is_none());
    assert!(agent_link::follow(&Layout { state: state.path().to_path_buf() }, None).is_ok());
    agent_link::roll_back(&Layout { state: state.path().to_path_buf() }, state.path(), &"f".repeat(40));
    assert_eq!(fs::read_dir(state.path()).unwrap().count(), 0);
}

#[test]
fn the_published_agent_runtime_carries_the_gh_shim() {
    let runtime = tempfile::tempdir().unwrap();
    for program in layout::PROGRAMS {
        fs::write(runtime.path().join(program), "").unwrap();
    }
    let tree = runtime.path().join(TREE);
    fs::create_dir_all(tree.join("harness")).unwrap();
    fs::create_dir_all(tree.join("bin")).unwrap();
    fs::write(tree.join("bin/gh"), format!("#!/bin/sh\n# {}\n", north_base::platform::gh_shim::MARKER)).unwrap();
    let staging = runtime.path().join("staging");
    agent_link::stage(runtime.path(), &staging).unwrap();
    assert!(north_base::platform::gh_shim::is_shim(&staging.join("gh")), "the agent runtime has no gh shim beside its north, so the agent login's gh answers as no north session");
}

#[test]
fn a_planted_checkout_ref_changes_no_runtime_decision() {
    let git = |directory: &Path, arguments: &[&str]| {
        let output = Command::new("git").current_dir(directory).args(arguments).env_remove("GIT_DIR").output().unwrap();
        assert!(output.status.success(), "git {arguments:?}: {}", String::from_utf8_lossy(&output.stderr));
        String::from_utf8_lossy(&output.stdout).trim().to_owned()
    };
    let commit = |checkout: &Path, file: &str, message: &str| {
        fs::write(checkout.join(file), message).unwrap();
        git(checkout, &["add", file]);
        git(checkout, &["-c", "user.name=t", "-c", "user.email=t@example.com", "-c", "commit.gpgsign=false", "commit", "-qm", message]);
        git(checkout, &["rev-parse", "HEAD"])
    };
    let dir = tempfile::tempdir().unwrap();
    let (origin, main) = (dir.path().join("origin.git"), dir.path().join("main"));
    git(dir.path(), &["init", "-q", "--bare", "-b", "main", &origin.to_string_lossy()]);
    git(dir.path(), &["clone", "-q", &origin.to_string_lossy(), &main.to_string_lossy()]);
    let pinned = commit(&main, "README.md", "pinned");
    let head = commit(&main, "README.md", "head");
    git(&main, &["push", "-q", "origin", "HEAD:refs/heads/main"]);
    let store = trusted_store_of(&main).unwrap();
    git(&main, &["checkout", "-q", "--detach", &pinned]);
    let forged = commit(&main, "Cargo.toml", "forged\n\nLive-fix: planted in the checkout's store");
    let planted = commit(&main, "README.md", "planted");
    git(&main, &["replace", &head, &planted]);
    git(&main, &["update-ref", "refs/remotes/origin/main", &planted]);
    let reads = |store: &Path| (git(store, &["rev-parse", "refs/remotes/origin/main"]) == planted, cadence::fix_since(store, &pinned, &head), reuse::unchanged_by(store, &pinned, &head, north_base::build_inputs::rust_build_input), ancestor(store, &forged, &head));
    assert_eq!(reads(&main), (true, Some(forged.clone()), false, true), "the planted refs reach every read of the checkout's own store");
    assert_eq!(main_commit(&store).unwrap(), head, "a promotion takes origin's head, not the origin/main planted in the checkout's store");
    assert_eq!(cadence::fix_since(&store, &pinned, &head), None, "a Live-fix planted through a replace ref opens no fast path");
    assert!(reuse::unchanged_by(&store, &pinned, &head, north_base::build_inputs::rust_build_input), "a planted Cargo.toml change forces no rebuild of a head that changes no build input");
    assert!(!ancestor(&store, &forged, &head), "a catch-up takes no planted commit as an ancestor of origin/main");
}
