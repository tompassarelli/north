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
