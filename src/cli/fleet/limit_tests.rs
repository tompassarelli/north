use std::process::Command;

use super::*;

/// north#1065 box 1: the capacity read model reads the same Fable meter `north fleet usage` shows. When a
/// provider reports no scoped Fable window, Fable's headroom is the account's shared weekly headroom.
#[test]
fn a_subscription_account_with_no_scoped_fable_window_reads_a_fable_reading_from_its_week() {
    let account = Label("a".to_owned());
    let week = Reading { row: RowId(1), account: account.clone(), window: Window::Week, used: Percent(40.0), resets_at: Some(Unix(1_800_000_000)), read_at: Unix(1_700_000_000), source: Source::Cli };
    let observed = with_fable(Observed { readings: vec![week], ..Observed::default() });
    let fable = observed.readings.iter().find(|reading| reading.window == Window::Scoped(ModelFamily::Fable)).expect("the account has a Fable meter");
    assert_eq!((fable.account.clone(), fable.used, fable.resets_at, fable.read_at), (account, Percent(40.0), Some(Unix(1_800_000_000)), Unix(1_700_000_000)));
}

/// A provider that does report the scoped window keeps that reading, not the shared week's.
#[test]
fn a_reported_scoped_fable_window_is_not_replaced_by_the_shared_week() {
    let account = Label("a".to_owned());
    let reading = |row, window, used| Reading { row: RowId(row), account: account.clone(), window, used: Percent(used), resets_at: Some(Unix(1_800_000_000)), read_at: Unix(1_700_000_000), source: Source::Cli };
    let observed = with_fable(Observed { readings: vec![reading(1, Window::Week, 60.0), reading(2, Window::Scoped(ModelFamily::Fable), 3.0)], ..Observed::default() });
    let fable: Vec<&Reading> = observed.readings.iter().filter(|reading| reading.window == Window::Scoped(ModelFamily::Fable)).collect();
    assert_eq!(fable.len(), 1, "{fable:?}");
    assert_eq!(fable[0].used, Percent(3.0));
}

fn git(dir: &Path, args: &[&str]) {
    let out = Command::new("git").arg("-C").arg(dir).args(["-c", "user.name=t", "-c", "user.email=t@t", "-c", "commit.gpgsign=false"]).args(args).output().unwrap();
    assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
}

/// On 2026-11-07 chr-north-1107-B, launched in the primary checkout whose HEAD held the unpushed north#791 commit, was
/// handed over for that commit as if it were its own (north#801): a primary checkout's HEAD is no session's lane, while
/// the same commit in a linked worktree it runs in, or a land ticket still in the queue, is.
#[test]
fn a_worker_in_a_primary_checkout_holding_a_foreign_unpushed_commit_is_not_handed_over_for_it_north_801() {
    let root = tempfile::tempdir().unwrap();
    let (origin, main, lane) = (root.path().join("origin.git"), root.path().join("main"), root.path().join("lane"));
    git(root.path(), &["init", "-q", "--bare", "-b", "main", origin.to_str().unwrap()]);
    git(root.path(), &["clone", "-q", origin.to_str().unwrap(), main.to_str().unwrap()]);
    git(&main, &["commit", "-q", "--allow-empty", "-m", "base"]);
    git(&main, &["push", "-q", "origin", "HEAD:main"]);
    git(&main, &["fetch", "-q", "origin"]);
    std::fs::write(main.join("placement.rs"), "class").unwrap();
    git(&main, &["add", "placement.rs"]);
    git(&main, &["commit", "-q", "-m", "north#791 placement class"]);
    git(&main, &["worktree", "add", "-q", "-b", "lane", lane.to_str().unwrap()]);
    let none = BTreeMap::new();
    assert!(!holds_unlanded_lane("chr-north-1107-B", &main, &none));
    assert!(holds_unlanded_lane("chr-north-1107-B", &lane, &none));
    let queues = root.path().join("land-queue");
    std::fs::create_dir_all(queues.join("north")).unwrap();
    std::fs::write(queues.join("north/1794000000000000-lane.json"), r#"{"session":"chr-north-1107-B","branch":"lane"}"#).unwrap();
    let tickets = crate::land_ticket::lanes_by_session(&queues).unwrap();
    assert!(tickets.contains_key("chr-north-1107-B"), "{tickets:?}");
    assert!(holds_unlanded_lane("chr-north-1107-B", &main, &tickets));
}

/// On 2026-11-09 root-from-github-91 ran in the primary checkout south/main and worked in a worktree it made under
/// south/worktrees with its commit ready and no land ticket: neither test saw its work, so the tick would have moved it
/// nowhere (north#1007). The worktree its output names is its lane; a primary checkout whose output names none stays itself.
#[test]
fn a_worker_in_a_primary_checkout_is_handed_over_for_the_worktree_it_made_beside_it_north_1007() {
    let root = tempfile::tempdir().unwrap();
    let (origin, main) = (root.path().join("origin.git"), root.path().join("main"));
    let lane = root.path().join("worktrees/ready");
    git(root.path(), &["init", "-q", "--bare", "-b", "main", origin.to_str().unwrap()]);
    git(root.path(), &["clone", "-q", origin.to_str().unwrap(), main.to_str().unwrap()]);
    git(&main, &["commit", "-q", "--allow-empty", "-m", "base"]);
    git(&main, &["push", "-q", "origin", "HEAD:main"]);
    git(&main, &["worktree", "add", "-q", "-b", "ready", lane.to_str().unwrap()]);
    std::fs::write(lane.join("fix.rs"), "fix").unwrap();
    git(&lane, &["add", "fix.rs"]);
    git(&lane, &["commit", "-q", "-m", "ready"]);
    let none = BTreeMap::new();
    let made = lane_of(&main, |worktrees| Some(worktrees.join("ready")));
    assert_eq!(made, lane);
    assert!(holds_unlanded_lane("root-from-github-91", &made, &none));
    assert_eq!(lane_of(&main, |_| None), main);
    assert_eq!(lane_of(&lane, |_| panic!("a linked worktree is read as it is")), lane);
}
