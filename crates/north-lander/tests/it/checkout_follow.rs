use super::serve::{git, report, World};

#[test]
fn a_landing_fast_forwards_the_served_checkout_and_leaves_local_work_where_it_is() {
    let world = World::new();
    let repository = world.repository("south");
    let before = git(&repository.checkout, &["rev-parse", "HEAD"]);
    repository.lane_with("cap", "origin/main", ("agents/orchestration.toml", "resident_cap = 20\n"), ("cap", None));

    let landed = world.lander(&repository, None).wait_with_output().unwrap();
    assert!(landed.status.success(), "{}", report(&landed));

    let origin = git(&repository.origin, &["rev-parse", "main"]);
    let head = git(&repository.checkout, &["rev-parse", "HEAD"]);
    assert_ne!(before, origin, "the landing did not move origin/main\n{}", report(&landed));
    assert_eq!(head, origin, "the served checkout stayed at {before} while origin/main took {origin}, so placement would read the old resident cap until a hand fast-forward\n{}", report(&landed));

    let world = World::new();
    let repository = world.repository("south");
    let before = git(&repository.checkout, &["rev-parse", "HEAD"]);
    std::fs::write(repository.checkout.join("README"), "an edit nobody committed\n").unwrap();
    repository.lane_with("cap", "origin/main", ("agents/orchestration.toml", "resident_cap = 20\n"), ("cap", None));

    let landed = world.lander(&repository, None).wait_with_output().unwrap();
    assert!(landed.status.success(), "{}", report(&landed));
    assert_eq!(git(&repository.checkout, &["rev-parse", "HEAD"]), before, "a checkout with uncommitted changes was moved anyway\n{}", report(&landed));
    assert_eq!(std::fs::read_to_string(repository.checkout.join("README")).unwrap(), "an edit nobody committed\n");
}
