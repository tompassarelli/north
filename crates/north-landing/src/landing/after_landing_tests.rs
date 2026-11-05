use super::{Failure, Filed, Filing, filing};

#[test]
fn the_debt_issue_keeps_the_class_line_and_unticked_box_dispatch_needs_when_it_rewrites_a_body_that_lacks_them() {
    let existing = Filed { number: 572, body: "Gates that run after landing and failed.\n\n<!-- after-landing-gate: aaaaaaaaaaaa code-health -->\n### aaaaaaaaaaaa (2026-11-05) gates: code-health\n".into() };
    let failures = [Failure { gate: "code-health".into(), output: "hotspot".into() }];
    let Filing::Edit { body, .. } = filing("bbbbbbbbbbbbbbbb", "2026-11-06", Some(&existing), &failures) else { panic!("an existing debt issue is edited") };
    let header: Vec<&str> = body.lines().take_while(|line| !line.trim().is_empty()).collect();
    assert!(header.iter().any(|line| line.starts_with("Category: ")) && header.iter().any(|line| line.starts_with("Class: ")), "dispatch refuses a bug issue with no Class: line in its header: {body}");
    assert!(body.lines().any(|line| line.starts_with("- [ ] ")), "dispatch staffs only an issue with an unticked box: {body}");
    assert!(body.contains("<!-- after-landing-gate: aaaaaaaaaaaa code-health -->"), "the older section stays: {body}");
}
