use super::{evidence, unproven};

#[test]
fn a_box_the_done_when_checker_passes_on_a_nonzero_exit_is_proven_so_its_issue_can_close() {
    let body = format!("- [x] Retired. Done when: `rg -n x goals.toml` prints nothing. {}", evidence("rg -n x goals.toml", 1, "0cf42359e77c1a2b", "2026-11-08"));
    assert_eq!(unproven(&body), Vec::<&str>::new(), "{body}");
}
