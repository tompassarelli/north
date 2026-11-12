use super::*;
use north_base::platform::wait;
use std::os::unix::fs::{PermissionsExt, symlink};
use std::sync::atomic::{AtomicBool, Ordering};

const OLDER: &str = "runtimes/aaaa";
const NEWER: &str = "runtimes/bbbb";
const WITHIN: Duration = Duration::from_secs(15);

fn replay_script(state: &Path) -> String {
    format!(
        "#!/bin/sh\nstate={state}\ntarget=$(readlink \"$state/current\")\nheld=$(readlink \"$state/lander\")\necho \"$target\" >> \"$state/replays\"\nln -sfn \"$target\" \"$state/lander\"\n\
         case \"$target\" in\n\
           {OLDER}) sleep 600 ;;\n\
           *) if [ -e \"$state/diverges\" ]; then ln -sfn \"$held\" \"$state/lander\"; echo \"$target\" > \"$state/lander-refused\"; echo \"north land promote: refused: the replay diverged; the lander link is unchanged\"; exit 1; fi ;;\n\
         esac\n",
        state = state.display()
    )
}

fn fixture(diverges: bool) -> (tempfile::TempDir, Layout) {
    let dir = tempfile::tempdir().unwrap();
    let layout = Layout { state: dir.path().to_owned() };
    for runtime in [OLDER, NEWER] {
        let bin = layout.state.join(runtime).join(TREE).join("bin");
        fs::create_dir_all(&bin).unwrap();
        fs::write(bin.join("north"), replay_script(&layout.state)).unwrap();
        fs::set_permissions(bin.join("north"), fs::Permissions::from_mode(0o755)).unwrap();
    }
    symlink(OLDER, layout.current()).unwrap();
    symlink("runtimes/held", layout.lander()).unwrap();
    if diverges {
        fs::write(layout.state.join("diverges"), "").unwrap();
    }
    (dir, layout)
}

fn lander_at(layout: &Layout, runtime: &str) -> bool {
    fs::read_link(layout.lander()).is_ok_and(|link| link == Path::new(runtime))
}

fn newer_arrives_during_the_older_replay(layout: &Layout) -> Result<(), Failure> {
    std::thread::scope(|scope| {
        let older = scope.spawn(|| follow_lander(layout));
        assert!(wait::until(WITHIN, || lander_at(layout, OLDER)), "the older runtime's replay never started");
        let moved = layout.state.join(".current.tmp");
        symlink(NEWER, &moved).unwrap();
        fs::rename(&moved, layout.current()).unwrap();
        follow_lander(layout).unwrap();
        assert!(wait::until(WITHIN, || fs::read_to_string(layout.state.join("replays")).is_ok_and(|replays| replays.lines().count() == 2)), "the newer runtime was never replayed while the older replay ran");
        older.join().unwrap()
    })
}

#[test]
fn a_newer_runtime_moves_the_lander_link_while_an_older_replay_runs() {
    let (_state, layout) = fixture(false);
    newer_arrives_during_the_older_replay(&layout).unwrap();
    assert!(lander_at(&layout, NEWER), "the link follows the newer runtime although the older runtime's replay would run for 10 min");
    assert_eq!(fs::read_to_string(layout.state.join("replays")).unwrap(), format!("{OLDER}\n{NEWER}\n"));

    let (_state, layout) = fixture(true);
    let held = newer_arrives_during_the_older_replay(&layout).unwrap_err();
    assert!(held.message.contains("lander link held"), "{}", held.message);
    assert!(lander_at(&layout, OLDER), "a divergence found by the newer runtime's replay sends the link back to the runtime held before it");
    assert_eq!(fs::read_to_string(layout.state.join("lander-refused")).unwrap().trim(), NEWER);
}

#[test]
fn a_follow_retakes_the_lock_while_a_starting_childs_copy_of_it_is_still_open() {
    let (_state, layout) = fixture(false);
    let held = lander_lock(&layout, Duration::ZERO).unwrap().unwrap();
    let copy = held.try_clone().unwrap();
    drop(held);
    assert!(lander_lock(&layout, Duration::ZERO).unwrap().is_none(), "a copy of the released descriptor keeps the lock, as a child between fork and exec does");
    let retaking = AtomicBool::new(false);
    std::thread::scope(|scope| {
        scope.spawn(|| {
            wait::until(WITHIN, || retaking.load(Ordering::SeqCst));
            drop(copy);
        });
        retaking.store(true, Ordering::SeqCst);
        assert!(lander_lock(&layout, WITHIN).unwrap().is_some(), "the follow left the lock to a copy that closed a moment later, so the newest runtime was never replayed");
    });
}
