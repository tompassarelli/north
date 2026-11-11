use super::*;

fn ticks_past(directory: &std::path::Path, mark: &std::path::Path) {
    let probe = directory.join(".tick");
    let modified = |path: &std::path::Path| fs::symlink_metadata(path).unwrap().modified().unwrap();
    let past = north_base::platform::wait::until(Duration::from_secs(5), || {
        fs::write(&probe, "").unwrap();
        modified(&probe) > modified(mark)
    });
    fs::remove_file(&probe).unwrap();
    assert!(past, "the file clock never moved past {}", mark.display());
}

#[test]
fn republishing_onto_the_live_runtime_keeps_the_time_it_went_live() {
    let state = tempfile::tempdir().unwrap();
    let layout = Layout { state: state.path().to_path_buf() };
    let link = layout::link(&"d".repeat(40));
    swap(&layout, &link).unwrap();
    let live_at = fs::symlink_metadata(layout.current()).unwrap().modified().unwrap();
    ticks_past(state.path(), &layout.current());
    swap(&layout, &link).unwrap();
    assert_eq!(fs::symlink_metadata(layout.current()).unwrap().modified().unwrap(), live_at);
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
fn a_copy_interrupted_over_an_hour_ago_leaves_no_staging_directory_while_a_fresh_one_stays() {
    let state = tempfile::tempdir().unwrap();
    let layout = Layout { state: state.path().to_path_buf() };
    let (stale, fresh) = (format!("{}.staging", "a".repeat(40)), format!("{}.staging", "b".repeat(40)));
    for name in [&stale, &fresh] {
        fs::create_dir_all(layout.runtimes().join(name)).unwrap();
        fs::write(layout.runtimes().join(name).join("north"), "half").unwrap();
    }
    let hours_ago = std::time::SystemTime::now() - Duration::from_secs(2 * 60 * 60);
    File::open(layout.runtimes().join(&stale)).unwrap().set_modified(hours_ago).unwrap();
    collect(&layout, &"c".repeat(40), None).unwrap();
    assert!(!layout.runtimes().join(&stale).exists());
    assert!(layout.runtimes().join(&fresh).exists());
}

fn fake_north(layout: &Layout, sha: &str, refusals: &[&str]) {
    use std::os::unix::fs::PermissionsExt;
    fs::create_dir_all(layout.runtime(sha).join(TREE)).unwrap();
    let said: String = refusals.iter().map(|refusal| format!("echo 'north check calls: ✗ {refusal}'\n")).collect();
    let north = layout.runtime(sha).join("north");
    fs::write(&north, format!("#!/bin/sh\n[ \"$1 $2 $3\" = \"check calls --tree\" ] || exit 99\n{said}exit 1\n")).unwrap();
    fs::set_permissions(&north, fs::Permissions::from_mode(0o755)).unwrap();
}

#[test]
fn a_pre_swap_failure_the_live_runtime_shares_does_not_hold_the_swap() {
    let state = tempfile::tempdir().unwrap();
    let layout = Layout { state: state.path().to_path_buf() };
    let (old, new, newer) = ("a".repeat(40), "b".repeat(40), "c".repeat(40));
    let stale = "~/.config/systemd/user/skill-review-queue.service:7: north skill review-queue: this runtime has no north skill review-queue command";
    let introduced = "/etc/systemd/user/pace-enforcement.service:4: north pace sample: this runtime has no north pace command";
    fake_north(&layout, &old, &[stale]);
    fake_north(&layout, &new, &[stale]);
    fake_north(&layout, &newer, &[stale, introduced]);
    let previous = layout::link(&old);
    let mut pages = Vec::new();
    for _ in 0..2 {
        let passed = preswap::check(&layout, &new, Some(&previous), &mut |text: &str| pages.push(text.to_owned())).unwrap();
        assert!(passed.contains("host incident") && passed.contains("skill-review-queue.service:7"), "{passed}");
    }
    assert_eq!(pages.len(), 1, "one page per episode: {pages:?}");
    assert!(pages[0].contains("host incident") && pages[0].contains("skill-review-queue.service:7"), "{}", pages[0]);

    std::os::unix::fs::symlink(&previous, layout.current()).unwrap();
    for _ in 0..2 {
        let held = sync(&layout, Some(&previous), &newer, &mut |text: &str| pages.push(text.to_owned())).unwrap_err();
        assert!(held.message.contains("pace-enforcement.service:4") && !held.message.contains("skill-review-queue"), "{}", held.message);
        assert_eq!(fs::read_link(layout.current()).unwrap(), previous);
    }
    assert_eq!(pages.len(), 2, "a refusal the new runtime introduced holds it with one page: {pages:?}");
    assert!(pages[1].contains(&format!("stays on {}", layout::short(&old))), "{}", pages[1]);
}

#[test]
fn a_host_with_a_dirty_main_checkout_still_promotes_the_shipped_runtime() {
    let state = tempfile::tempdir().unwrap();
    let layout = Layout { state: state.path().join("state") };
    let checkout = state.path().join("main");
    let git = |args: &[&str]| {
        let output = process::run("git", Some(&checkout), args).unwrap();
        assert!(output.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&output.stderr));
        String::from_utf8_lossy(&output.stdout).trim().to_owned()
    };
    fs::create_dir_all(&checkout).unwrap();
    git(&["init", "-q", "-b", "main"]);
    git(&["remote", "add", "origin", &state.path().join("gone").display().to_string()]);
    let mut shas = Vec::new();
    for round in 0..2 {
        fs::write(checkout.join("file"), round.to_string()).unwrap();
        git(&["add", "file"]);
        git(&["-c", "user.name=t", "-c", "user.email=t@t", "commit", "-q", "-m", "c"]);
        shas.push(git(&["rev-parse", "HEAD"]));
    }
    fs::write(checkout.join("file"), "uncommitted").unwrap();
    let (live, shipped) = (shas[1].clone(), "b".repeat(40));
    for sha in [&shas[0], &live, &shipped] {
        fs::create_dir_all(layout.runtime(sha)).unwrap();
        for program in layout::PROGRAMS.iter().copied().chain([layout::BUILT_HERE]) {
            fs::write(layout.runtime(sha).join(program), "").unwrap();
        }
    }
    let current = layout::link(&live);
    assert_eq!(super::shipped::shipped(&layout, &checkout, &shipped, Some(&current)), super::shipped::Shipped::Promote);
    assert_eq!(super::shipped::shipped(&layout, &checkout, &shas[0], Some(&current)), super::shipped::Shipped::Older(live), "a checkout that knows the order still refuses a step back");
}

#[test]
fn a_waiting_promotion_whose_runtime_is_no_longer_current_leaves_instead_of_waiting_out_the_lock() {
    let state = tempfile::tempdir().unwrap();
    let layout = Layout { state: state.path().to_path_buf() };
    let _holder = named_lock(&layout, PROMOTE_LOCK, FlockOperation::NonBlockingLockExclusive).unwrap().unwrap();
    let left = named_lock_waiting_unless(&layout, PROMOTE_LOCK, Duration::from_secs(600), "north runtime promote", &mut || Some("runtimes/newer".to_owned())).unwrap_err();
    assert_eq!(left.code, DEFERRED);
}

#[test]
fn a_runtime_taken_from_a_build_host_has_its_tree_although_the_main_checkout_moved_on() {
    let state = tempfile::tempdir().unwrap();
    let layout = Layout { state: state.path().join("state") };
    let checkout = state.path().join("main");
    let git = |args: &[&str]| {
        let output = process::run("git", Some(&checkout), args).unwrap();
        assert!(output.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&output.stderr));
        String::from_utf8_lossy(&output.stdout).trim().to_owned()
    };
    fs::create_dir_all(&checkout).unwrap();
    git(&["init", "-q", "-b", "main"]);
    let mut shas = Vec::new();
    for round in ["promoted", "moved on"] {
        fs::write(checkout.join("file"), round).unwrap();
        git(&["add", "file"]);
        git(&["-c", "user.name=t", "-c", "user.email=t@t", "commit", "-q", "-m", "c"]);
        shas.push(git(&["rev-parse", "HEAD"]));
    }
    let runtime = layout.runtime(&shas[0]);
    fs::create_dir_all(&runtime).unwrap();
    for program in layout::PROGRAMS {
        fs::write(runtime.join(program), "").unwrap();
    }
    fs::write(runtime.join(layout::BUILT_ON), "cpx62b\n").unwrap();
    assert!(!runtime.join(TREE).exists(), "a build host's binaries arrive without a tree");
    let taken = built(&layout, &checkout, &shas[0]).unwrap();
    assert_eq!(fs::read_to_string(taken.join(TREE).join("file")).unwrap(), "promoted", "the tree is the promoted commit's, not the checkout's");
}
