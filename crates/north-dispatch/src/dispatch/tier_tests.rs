use super::plan::Cloud;
use super::tests::{issue, survey};
use super::{decide, slots};

fn uncategorised(repo: &str, number: u64, body: &str, labels: &[&str]) -> super::issues::Issue {
    super::issues::Issue { body: body.into(), ..issue(repo, number, "", labels) }
}


#[test]
fn a_rolling_alarm_issue_is_ready_but_never_placed_because_its_body_declares_no_category_while_the_same_body_with_one_launches() {
    let rise = crate::tune::rises::Rise { tier: "sonnet".into(), category: "tooling".into(), before: (5.2, 3), after: (10.4, 3), boxes: Vec::new() };
    let body = crate::tune::rises::issue_body(&[rise]).unwrap();
    let hosts = vec![slots::Host { name: "gex45".into(), cap: Some(24), running: Ok(0), measure: None }];
    let accounts = vec![slots::Account { name: "claude:b".into(), hosts: vec!["gex45".into()], plan: slots::PlanSize { five_hour: 4.0, week: 2.0 }, room: slots::Room { five_hour: 90.0, week: 90.0, week_reset_h: 100.0 }, spending: false, refusal: None, burn: None, readings: Vec::new() }];
    let alarm = decide(&survey(vec![uncategorised("north", 608, &body, &[])], Cloud::Closed(String::new()), hosts.clone(), accounts.clone()), "test");
    assert!(alarm.launches.is_empty(), "{:?}", alarm.skipped);
    assert_eq!(alarm.skipped, [(super::issues::IssueRef::parse("north#608").unwrap(), format!("ready, not placed: {}", super::facets::UNCATEGORISED))]);

    let headed = decide(&survey(vec![uncategorised("north", 608, &format!("Category: tooling\n{body}"), &[])], Cloud::Closed(String::new()), hosts, accounts), "test");
    assert_eq!(headed.launches.len(), 1, "the header alone is what staffed north#608: {:?}", headed.skipped);
}
