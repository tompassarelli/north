use super::*;
use crate::ops::pacing::placement::gate::{self as placement, Verdict};

fn verdict(account: &str, headroom: Option<f64>) -> Verdict {
    let line = match headroom {
        Some(room) => format!("{account} available: 5h {:.0}% ({room:.0}% headroom)", 95.0 - room),
        None => format!("{account} unavailable: runaway: 5h 42% + 86.0%/h x 2.0h = 214% > 100% before reset"),
    };
    Verdict { account: account.into(), available: headroom.is_some(), headroom: headroom.unwrap_or(0.0), line, owners: Vec::new() }
}

fn decision(target: Target) -> Decision {
    Decision {
        target,
        code_only: true,
        surface: "code-only box on public north".into(),
        verdicts: vec![verdict("claude:a", Some(60.0)), verdict("claude:c", None)],
        machine: None,
        here: "here".into(),
        here_lacks: None,
        machine_lines: Vec::new(),
    }
}

fn noted(note: Option<&str>) -> Result<Placed, String> {
    Ok(Placed { note: note.map(Into::into), machine: None, need: None })
}

#[test]
fn an_override_reason_admits_an_off_placement_launch_and_survives_revive() {
    let cloud = decision(Target::Cloud("cloud credit: unread".into()));
    let local = decision(Target::Account("claude:a".into()));
    let launch = |account| Launch { name: "box", account, brief: None, cwd: None, machine: None };
    let given = |local: Option<&str>, pin: Option<&str>| Overrides { local: local.map(Into::into), pin: pin.map(Into::into) };

    assert_eq!(admit(&local, &launch("a"), "", &Overrides::default()), noted(None));
    let refused = admit(&local, &launch("c"), "", &Overrides::default()).unwrap_err();
    assert!(refused.starts_with("claude:c unavailable: runaway") && refused.contains("--account a"), "{refused}");
    assert_eq!(admit(&local, &launch("c"), "", &given(None, Some("drain c"))), noted(Some("pin: drain c")));
    assert!(admit(&cloud, &launch("a"), "", &given(None, Some("drain c"))).unwrap_err().contains("claude --cloud"));
    assert!(admit(&cloud, &launch("c"), "", &given(Some("x"), None)).unwrap_err().contains("--pin"));
    let both = admit(&cloud, &launch("c"), "", &given(Some("x"), Some("y"))).unwrap().note;
    assert_eq!(both.as_deref(), Some("local: x; pin: y"));

    assert_eq!(Overrides::default().recorded(both.as_deref()), given(Some("x"), Some("y")));
    assert_eq!(given(Some("new"), None).recorded(both.as_deref()), given(Some("new"), Some("y")));
    assert_eq!(Overrides::default().recorded(None), Overrides::default());

    let mut remote = decision(Target::Account("claude:a".into()));
    remote.machine = Some(placement::Machine { name: "far".into(), ssh: "tom@far".into(), serves: Vec::new(), cap: 4, live: Some(1), accounts: vec!["a".into(), "c".into()] });
    let launch = Launch { name: "box", account: "a", brief: None, cwd: None, machine: None };
    let placed = admit(&remote, &launch, "", &Overrides::default()).unwrap();
    assert_eq!(placed, Placed { note: None, machine: Some(Machine { name: "far".into(), ssh: "tom@far".into() }), need: None });
    let kept = admit(&remote, &launch, "", &Overrides { local: Some("needs my desktop".into()), pin: None }).unwrap();
    assert_eq!(kept, Placed { note: Some("local: needs my desktop".into()), machine: None, need: None });
    let here = admit(&decision(Target::Account("claude:a".into())), &launch, "", &Overrides { local: Some("x".into()), pin: None }).unwrap();
    assert_eq!(here, noted(None).unwrap(), "--local on a box placed here records nothing");

    let unsigned = admit(&remote, &Launch { name: "box", account: "b", brief: None, cwd: None, machine: None }, "", &Overrides::default()).unwrap_err();
    assert!(unsigned.starts_with("far is not signed in to claude:b (signed in there: a, c); launch with --account a, or "), "{unsigned}");
    let pinned = admit(&remote, &Launch { name: "box", account: "b", brief: None, cwd: None, machine: None }, "", &Overrides { local: Some("x".into()), pin: Some("y".into()) });
    assert_eq!(pinned.unwrap().machine, None);
}
