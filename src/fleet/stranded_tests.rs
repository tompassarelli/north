use super::*;

const CAPACITY_REPORT: &str = "Not done: the citation check is written and committed in a local lane, but it has never been built or run.\n\nStop: capacity. The lead's order to start no cargo build on gex45 arrived while my test run was still queued.\n\n**What exists** (commit `f6b5dea6f`, lane `n245-citations`, based on origin/main `a0beeab4`, not pushed):\n- **The check:** `north citation-check` is in `src/ops/citation_check/`.\n\nWhen gex45 is over 15 GB free, the next steps are:\n1. Build with `CARGO_INCREMENTAL=0`.";
const PARKED_REPORT: &str = "The `n247 cargo test finishing` monitor expired with no events.\n\nMy report stands: Not done, with Stop: capacity pending. Nothing of mine is running, and the edits are uncommitted and untested in `/home/tom/code/north/worktrees/n247-brief-proof`. I'm waiting for the lead to say gex45 is over 15 GB, then I'll build.";
const PORCELAIN: &str = "worktree /home/tom/code/north/main\nHEAD 5c1ea2529\nbranch refs/heads/main\n\nworktree /home/tom/code/north/worktrees/n245-citations\nHEAD f6b5dea6f\nbranch refs/heads/n245-citations\n\nworktree /home/tom/code/north/worktrees/n245-sweep\nHEAD e584358a9\nbranch refs/heads/n245-sweep\n\nworktree /home/tom/code/north/worktrees/n247-brief-proof\nHEAD a0beeab4d\nbranch refs/heads/n247-brief-proof\n\nworktree /home/tom/.cache/land-offload/code-north-main\nHEAD 505a8a1ba\ndetached\n";

#[test]
fn a_report_stops_for_capacity_or_on_a_pending_wait_and_a_done_report_on_neither() {
    assert_eq!(stop(CAPACITY_REPORT), Some(Stop::Capacity));
    assert_eq!(stop(PARKED_REPORT), Some(Stop::Capacity), "it names the stop mid-line");
    assert_eq!(stop("The land check is queued behind two admission tickets on gex45; the Monitor wakes me when it starts."), Some(Stop::Wait));
    assert_eq!(stop("Done: landed abc123; I waited for CI on the way.\n\nStop: capacity was lifted."), None);
    assert_eq!(stop("Not done: the box needs Tom.\n\nStop: decision (take three or drop it)"), None);
    assert_eq!(stop("Not done: waiting for the lead.\n\nStop: decision"), None, "a final report is no pending wait");
    assert_eq!(stop("Should I take the third try?"), None, "a question waits on its reader");
}

#[test]
fn a_report_names_its_lanes_by_path_directory_or_branch_and_never_the_main_tree() {
    let checkout = Checkout::parse(PORCELAIN);
    assert_eq!(checkout.repo().as_deref(), Some("north"));
    let named = |report: &str| checkout.named(report).into_iter().map(|tree| tree.path.display().to_string()).collect::<Vec<_>>();
    assert_eq!(named(CAPACITY_REPORT), ["/home/tom/code/north/worktrees/n245-citations"], "n245-sweep is another session's lane");
    assert_eq!(named(PARKED_REPORT), ["/home/tom/code/north/worktrees/n247-brief-proof"]);
    assert!(named("rebased on main and pushed n245-citationsx").is_empty());
}

#[test]
fn a_lane_holds_unlanded_work_while_it_has_unmerged_commits_or_uncommitted_changes() {
    let lane = |ahead: u32, dirty: bool, landed: bool| Lane { path: "/l".into(), branch: None, ahead, dirty, landed };
    assert!(lane(1, false, false).unlanded());
    assert!(lane(0, true, true).unlanded());
    assert!(!lane(0, false, false).unlanded(), "every commit is an ancestor of origin/main");
    assert!(!lane(3, false, true).unlanded(), "its change landed in another form");
}
