use north_base::foundation::shim::write_script;

use super::super::{OFF_HOST_PATIENCE, OFF_HOST_RETRY};
use super::super::arguments::Environment;
use super::{Placed, chosen, placed, runner_host};

#[tokio::test]
async fn checks_before_queueing_go_to_an_idle_build_host_when_this_laptop_has_the_most_room() {
    let directory = tempfile::tempdir().unwrap();
    let north = directory.path().join("north");
    write_script(&north, "case \"$*\" in\n  *'--avoid whiterabbit'*) echo gex45 ;;\n  *'--avoid busy'*) echo 'north capacity offload: no host has room' >&2; exit 1 ;;\n  *) echo 'north capacity offload: whiterabbit has the most room, so the land check runs here' >&2 ;;\nesac\n");
    let north = north.to_str().unwrap();
    assert_eq!(chosen(north, "build-host", &["whiterabbit".to_owned()]).await.as_deref(), Some("gex45"), "on 2026-11-06 the laptop at load 12 of 24 out-roomed gex45 at 6 of 20, so every lane's checks before queueing got no verdict");
    assert!(chosen(north, "build-host", &[]).await.is_some_and(|host| !host.is_empty()), "on 2026-11-08 a probe the laptop out-roomed built in its own cold worktree target for 8 minutes beside the laptop's warm offload slots, so this host is named and its checks run in those slots");
    assert_eq!(chosen(north, "build-host", &["busy".to_owned()]).await, None, "with no host having room the checks run in place under the land-check capacity lease");
}

#[tokio::test(start_paused = true)]
async fn a_check_before_queueing_waits_in_line_past_the_patience_while_every_host_is_full_and_runs_once_a_slot_frees() {
    let directory = tempfile::tempdir().unwrap();
    let north = directory.path().join("north");
    let calls = directory.path().join("calls");
    write_script(&north, &format!("echo x >>{calls}\nif [ \"$(wc -l <{calls})\" -le 40 ]; then echo 'north capacity offload: no host has room: gex45: 2 cores free of 20; ccx33: unreachable or unreadable' >&2; exit 1; fi\necho gex45\n", calls = calls.display()));
    let placed = placed(north.to_str().unwrap(), "build-host", "whiterabbit", &[]).await;
    assert_eq!(placed, Placed::Host("gex45".to_owned()), "on 2026-11-08 at 10:18 AM n882-climb's enqueue was refused for want of a build host with room");
    let asked = u32::try_from(std::fs::read_to_string(&calls).unwrap().lines().count()).unwrap();
    assert!(OFF_HOST_RETRY * (asked - 1) > OFF_HOST_PATIENCE, "the wait is not capped at the patience for hosts that answer");
}

#[tokio::test(start_paused = true)]
async fn a_check_before_queueing_gives_up_only_when_no_build_host_answers_and_runs_here_only_when_none_is_configured() {
    let directory = tempfile::tempdir().unwrap();
    let silent = directory.path().join("silent");
    write_script(&silent, "echo 'north capacity offload: no host has room: gex45: unreachable or unreadable' >&2\nexit 1\n");
    assert_eq!(placed(silent.to_str().unwrap(), "build-host", "whiterabbit", &[]).await, Placed::Unanswered);
    let hostless = directory.path().join("hostless");
    write_script(&hostless, "echo 'north capacity offload: no host has room: none has an ssh destination' >&2\nexit 1\n");
    assert_eq!(placed(hostless.to_str().unwrap(), "build-host", "whiterabbit", &[]).await, Placed::Here);
    let answers = directory.path().join("answers");
    write_script(&answers, "echo gex45\n");
    let replay = Environment { off_host: true, ..Environment::local("north".to_owned()) };
    assert_eq!(runner_host(answers.to_str().unwrap(), &replay, None, &[]).await.as_deref(), Some("gex45"), "on 2026-11-12 a promotion replay with no runner configured was placed on the lander's host, and its test listing ran cargo there before any host was chosen");
    assert_eq!(runner_host(silent.to_str().unwrap(), &replay, Some("build-host"), &[]).await, None, "an off-host replay with no build host answering reaches no verdict rather than building on the lander's host");
    assert_eq!(runner_host(silent.to_str().unwrap(), &Environment::local("north".to_owned()), None, &[]).await.as_deref(), Some(""), "a set that is not off-host and names no runner runs where it is");
}
