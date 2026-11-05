use super::{Failure, Filed, Filing, filing};

#[test]
fn the_debt_issue_keeps_an_unticked_done_when_box_when_it_rewrites_a_body_that_lacks_one() {
    let existing = Filed { number: 572, body: "Gates that run after landing and failed.\n\n<!-- after-landing-gate: aaaaaaaaaaaa code-health -->\n### aaaaaaaaaaaa (2026-11-05) gates: code-health\n".into() };
    let failures = [Failure { gate: "code-health".into(), output: "hotspot".into() }];
    let Filing::Edit { body, .. } = filing("bbbbbbbbbbbbbbbb", "2026-11-06", Some(&existing), &failures) else { panic!("an existing debt issue is edited") };
    assert!(body.lines().any(|line| line.starts_with("- [ ] ")), "dispatch staffs only an issue with an unticked box: {body}");
    assert!(body.contains("<!-- after-landing-gate: aaaaaaaaaaaa code-health -->"), "the older section stays: {body}");
}
