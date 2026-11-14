use std::path::PathBuf;

use super::operator_launch;

#[test]
fn a_reviewer_placed_on_the_build_host_that_holds_its_lane_is_not_refused_for_naming_that_lane_while_a_state_file_still_is_and_a_launch_the_tick_spooled_never_dials_as_the_operator() {
    let root = tempfile::tempdir().unwrap();
    let home = root.path();
    let lane = home.join("code/north/worktrees/north-1109-d0208");
    let held = home.join(".local/state/north/decisions/admission/x.json");
    std::fs::create_dir_all(&lane).unwrap();
    std::fs::create_dir_all(held.parent().unwrap()).unwrap();
    std::fs::write(&held, "{}").unwrap();
    let brief = crate::dispatch::deepseek::review::brief("north-1109-d0208", "north#1109", "mechanical", None, &lane, "abc", "medium");
    assert_eq!(crate::platform::home_paths::outside(&brief, home, &lane), Vec::<PathBuf>::new(), "the lane and the archive the brief names are on the machine the reviewer starts in its lane");
    let state = format!("{brief}\nRead {}", held.display());
    assert_eq!(crate::platform::home_paths::outside(&state, home, &lane), vec![held], "a file outside the lane that this host holds stays refused");
    assert_eq!(crate::platform::home_paths::outside(&state, home, home).len(), 2, "a session started in the home directory exempts nothing");
    assert!(operator_launch(false, false, false, || true));
    assert!(!operator_launch(false, true, false, || true), "a sandboxed lead's spooled launch picked the operator login");
    assert!(!operator_launch(true, false, false, || true));
    assert!(!operator_launch(false, false, true, || true));
    assert!(!operator_launch(false, false, false, || false));
}
