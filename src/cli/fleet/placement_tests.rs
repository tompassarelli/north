use std::path::{Path, PathBuf};

#[test]
fn a_reviewer_placed_on_the_build_host_that_holds_its_lane_is_not_refused_for_naming_that_lane_while_a_state_file_still_is() {
    let home = Path::new("/home/tom");
    let lane = home.join("code/north/worktrees/north-1109-d0208");
    let brief = crate::dispatch::deepseek::review::brief("north-1109-d0208", "north#1109", "mechanical", None, &lane, "abc", "medium");
    assert_eq!(crate::platform::home_paths::outside(&brief, home, &lane), Vec::<PathBuf>::new(), "the lane and the archive the brief names are on the machine the reviewer starts in its lane");
    let state = format!("{brief}\nRead /home/tom/.local/state/north/decisions/admission/x.json");
    assert_eq!(crate::platform::home_paths::outside(&state, home, &lane), vec![home.join(".local/state/north/decisions/admission/x.json")], "a file outside the lane stays refused");
    assert_eq!(crate::platform::home_paths::outside(&state, home, home).len(), 2, "a session started in the home directory exempts nothing");
}
