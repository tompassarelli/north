use super::*;

const CACHEDIR_TAG: &str = "Signature: 8a477f597d28d172789f06886806bc55\n";

fn lane(container: &Path, name: &str) -> std::path::PathBuf {
    let lane = container.join(name);
    fs::create_dir_all(&lane).unwrap();
    fs::write(lane.join(".git"), "gitdir: ../../main/.git/worktrees/lane\n").unwrap();
    lane
}

fn cargo_target(lane: &Path) {
    fs::create_dir_all(lane.join("target/debug/deps")).unwrap();
    fs::write(lane.join("target/CACHEDIR.TAG"), CACHEDIR_TAG).unwrap();
    fs::write(lane.join("target/debug/deps/libnorth.rlib"), vec![0u8; 4096]).unwrap();
}

#[test]
fn a_linked_lane_loses_its_cargo_target_and_keeps_its_sources() {
    let scratch = tempfile::tempdir().unwrap();
    let lane = lane(&scratch.path().join("worktrees"), "slug");
    cargo_target(&lane);
    fs::write(lane.join("Cargo.toml"), "[package]\n").unwrap();
    assert_eq!(release(&lane).unwrap(), Some(4096 + CACHEDIR_TAG.len() as u64));
    assert!(!lane.join("target").exists());
    assert!(lane.join("Cargo.toml").is_file());
}

#[test]
fn a_symlinked_target_is_left_alone() {
    let scratch = tempfile::tempdir().unwrap();
    let lane = lane(&scratch.path().join("worktrees"), "slug");
    let shared = scratch.path().join("shared");
    fs::create_dir_all(&shared).unwrap();
    fs::write(shared.join("CACHEDIR.TAG"), "").unwrap();
    std::os::unix::fs::symlink(&shared, lane.join("target")).unwrap();
    assert_eq!(release(&lane).unwrap(), None);
    assert!(shared.join("CACHEDIR.TAG").is_file());
}
