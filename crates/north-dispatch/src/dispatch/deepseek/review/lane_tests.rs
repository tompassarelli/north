use super::lane::Worktree;
use super::verdict::rejected_lane;
use std::path::{Path, PathBuf};
use std::process::Command;

fn init(directory: &Path) {
    std::fs::create_dir_all(directory).unwrap();
    assert!(Command::new("git").arg("init").arg("-q").arg(directory).status().unwrap().success());
}

#[test]
fn the_reviewer_cwd_is_the_main_checkout_and_a_verdict_still_finds_its_lane() {
    let lane = PathBuf::from("/home/tom/code/north/worktrees/ds-1");
    let checkout = PathBuf::from("/home/tom/code/north/main");
    let here = Worktree { path: lane.clone(), checkout: checkout.clone(), machine: None };
    assert_eq!(here.checkout_cwd(), "/home/tom/code/north/main");
    let placed = Worktree { path: lane, checkout, machine: Some(("whiterabbit".to_owned(), "tom@whiterabbit".to_owned())) };
    assert_eq!(placed.checkout_cwd(), "whiterabbit:/home/tom/code/north/main");

    let root = tempfile::tempdir().unwrap();
    let main = root.path().join("main");
    let made = root.path().join("worktrees").join("ds-1");
    init(&main);
    init(&made);
    let top = made.canonicalize().unwrap();
    assert_eq!(rejected_lane(&made).map(|top| Path::new(&top).canonicalize().unwrap()), Some(top));
    assert_eq!(rejected_lane(&main), None, "the main checkout is not a lane to reject");
}
