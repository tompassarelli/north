use super::*;
use crate::ops::pacing::placement::gate::Verdict;

fn verdict(account: &str, headroom: Option<f64>) -> Verdict {
    let line = match headroom {
        Some(room) => format!("{account} available: 5h {:.0}% ({room:.0}% headroom)", 95.0 - room),
        None => format!("{account} unavailable: runaway: 5h 42% + 86.0%/h x 2.0h = 214% > 100% before reset"),
    };
    Verdict { account: account.into(), available: headroom.is_some(), headroom: headroom.unwrap_or(0.0), line }
}

fn decision(target: Target) -> Decision {
    Decision { target, code_only: true, surface: "code-only box on public north".into(), verdicts: vec![verdict("claude:a", Some(60.0)), verdict("claude:c", None)] }
}

#[test]
fn an_override_reason_admits_an_off_placement_launch_and_survives_revive() {
    let cloud = decision(Target::Cloud("cloud credit: unread".into()));
    let local = decision(Target::Account("claude:a".into()));
    let launch = |account| Launch { name: "box", account, brief: None };
    let given = |local: Option<&str>, pin: Option<&str>| Overrides { local: local.map(Into::into), pin: pin.map(Into::into) };

    assert_eq!(admit(&local, &launch("a"), "", &Overrides::default()), Ok(None));
    let refused = admit(&local, &launch("c"), "", &Overrides::default()).unwrap_err();
    assert!(refused.starts_with("claude:c unavailable: runaway") && refused.contains("--account a"), "{refused}");
    assert_eq!(admit(&local, &launch("c"), "", &given(None, Some("drain c"))), Ok(Some("pin: drain c".into())));
    assert!(admit(&cloud, &launch("a"), "", &given(None, Some("drain c"))).unwrap_err().contains("claude --cloud"));
    assert!(admit(&cloud, &launch("c"), "", &given(Some("x"), None)).unwrap_err().contains("--pin"));
    let both = admit(&cloud, &launch("c"), "", &given(Some("x"), Some("y"))).unwrap();
    assert_eq!(both.as_deref(), Some("local: x; pin: y"));

    assert_eq!(Overrides::default().recorded(both.as_deref()), given(Some("x"), Some("y")));
    assert_eq!(given(Some("new"), None).recorded(both.as_deref()), given(Some("new"), Some("y")));
    assert_eq!(Overrides::default().recorded(None), Overrides::default());
}
