use std::path::Path;

use super::*;

#[test]
fn a_launch_for_another_repository_gets_its_lane_or_is_refused() {
    let north = Path::new("/home/tom/code/north/main");
    let brief = "Item: south#42 A south box\nCategory: bug\n\nWork the box.";
    let refused = refusal(brief, north).expect("a north checkout for a south item is refused");
    assert!(refused.contains("--cwd /home/tom/code/south/main"), "{refused}");
    assert!(refused.contains("finish refuses them"), "{refused}");

    let lane = Path::new("/home/tom/code/north/worktrees/worker-1");
    assert_eq!(refusal("Item: tompasarelli/north#42 A north box", lane), None, "a lane of the item's own repository starts");
    assert_eq!(refusal("Item: firn#7 A firn box", Path::new("/home/tom/code/nixos-config/main")), None, "firn's checkout is nixos-config");
    assert_eq!(refusal("Item: west#7 A west box", Path::new("/home/tom/code/west/worktrees/lane")), None);

    assert_eq!(refusal("no item line at all", north), None, "a brief with no item has no repository to check");
    assert_eq!(refusal(brief, Path::new("/tmp/scratch")), None, "a directory outside a code checkout names no repository to compare");
}
