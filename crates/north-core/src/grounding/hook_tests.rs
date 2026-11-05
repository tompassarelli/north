use super::*;
use compose::{HOOK_LIMIT_CHARS, assemble};
use store::{Doc, Hit};

fn hit(reference: &str, cover: f64, text: &str) -> Hit {
    Hit {
        doc: Doc {
            source: "policy".into(),
            id: reference.into(),
            reference: reference.into(),
            title: format!("skill {reference}"),
            text: text.into(),
            at: 0.0,
            decays: false,
        },
        cover,
        score: cover,
        matched: vec!["gauge".into()],
    }
}

fn words() -> Vec<String> {
    vec!["river".into(), "gauge".into()]
}

const NOTIFICATION: &str = "<task-notification>\n<task-id>a82ad1133476ed437</task-id>\n<status>completed</status>\n<summary>Agent \"Rolling after-landing gate issue\" finished; did we already build the skill before? remember the hook</summary>\n<result>Not done: Stop: landing. toolu tmp task notification</result>\n</task-notification>";

#[test]
fn system_events_inject_nothing_and_never_the_tier_b_instruction() {
    for prompt in [
        NOTIFICATION,
        "<system-reminder>\n<tools-used-before-summary>bash</tools-used-before-summary>\n</system-reminder>",
        "<local-command-caveat>The command below was run directly</local-command-caveat>\n<command-name>/model</command-name>\n<command-message>model</command-message>\n<command-args></command-args>",
        "<local-command-stdout>Set model to Opus</local-command-stdout>",
        "<task-notification>\n<task-id>x</task-id>\n<summary>unterminated, the hook we built before",
    ] {
        assert_eq!(typed_text(prompt), "", "{prompt}");
        let mut session = Session::default();
        let text = render(prompt, 0.0, "a prior reply about the river gauge skill we built before", &mut session).unwrap();
        assert_eq!(text, "", "{prompt}");
        assert!(!text.contains("tier B"));
        assert!(session.refs.is_empty() && session.live.is_none());
    }
    assert_eq!(typed_text(&format!("{NOTIFICATION}\nwhat did the gauge skill decide?")), "what did the gauge skill decide?");
}

#[test]
fn a_best_hit_below_the_floor_injects_nothing_or_only_a_changed_usage_block() {
    let hits = vec![hit("harness/skills/gauge/SKILL.md", 0.4, "river gauge")];
    let shape = Shape::hook(HashSet::new(), None);
    assert_eq!(assemble("river gauge", &hits, &words(), vec![], &[], &shape).text, "");
    let usage = vec!["Usage, source of truth x:".to_owned(), "- claude-a: 5h 40% (resets Thu 2026-11-05 09:20Z, in 23m, as of 08:57Z); allowed_workers 3".to_owned()];
    let first = assemble("usage river gauge", &hits, &words(), usage.clone(), &[], &shape);
    assert!(first.text.contains("claude-a: 5h 40%"));
    assert!(!first.text.contains("SKILL.md"));
    let later = vec![usage[0].clone(), usage[1].replace("in 23m, as of 08:57Z", "in 9m, as of 09:11Z")];
    let again = assemble("usage river gauge", &hits, &words(), later, &[], &Shape::hook(HashSet::new(), first.live.clone()));
    assert_eq!(again.text, "");
    let moved = vec![usage[0].clone(), usage[1].replace("40%", "55%")];
    assert!(assemble("usage river gauge", &hits, &words(), moved, &[], &Shape::hook(HashSet::new(), first.live)).text.contains("55%"));
}

#[test]
fn an_item_injected_earlier_in_the_session_is_not_injected_again() {
    let hits = vec![
        hit("harness/skills/gauge/SKILL.md", 1.0, "river gauge"),
        hit("harness/skills/tide/SKILL.md", 0.6, "river gauge tide"),
    ];
    let first = assemble("river gauge", &hits, &words(), vec![], &[], &Shape::hook(HashSet::new(), None));
    assert!(first.text.contains("harness/skills/gauge/SKILL.md"));
    assert!(first.refs.contains(&"harness/skills/gauge/SKILL.md".to_owned()));
    let seen: HashSet<String> = ["harness/skills/gauge/SKILL.md".to_owned()].into();
    let second = assemble("river gauge", &hits, &words(), vec![], &[], &Shape::hook(seen, None));
    assert!(!second.text.contains("harness/skills/gauge/SKILL.md"), "{}", second.text);
    assert!(second.text.contains("harness/skills/tide/SKILL.md"));
    let all: HashSet<String> = first.refs.into_iter().collect();
    assert_eq!(assemble("river gauge", &hits, &words(), vec![], &[], &Shape::hook(all, None)).text, "");
}

#[test]
fn the_whole_injection_stays_under_the_hook_cap() {
    let long = "river gauge kayak cache detail ".repeat(20);
    let hits: Vec<Hit> = (0..60).map(|i| hit(&format!("harness/skills/gauge-{i}/SKILL.md"), 1.0, &long)).collect();
    let usage = vec!["Usage:".to_owned(), format!("- claude-a: {}", "5h 40% ".repeat(40))];
    let extra = vec![format!("No confident tier-A hit: run `north grounding query --tier B --query '{}'`", "x".repeat(300))];
    let out = assemble("river gauge", &hits, &words(), usage, &extra, &Shape::hook(HashSet::new(), None));
    assert!(!out.text.is_empty());
    assert!(ulen(&out.text) < HOOK_LIMIT_CHARS, "{}", ulen(&out.text));
    assert!(out.refs.iter().all(|r| out.text.contains(r.as_str())));
}

#[test]
fn session_ids_cannot_escape_the_grounding_state_dir() {
    assert!(session_path("../escape").is_none());
    assert!(session_path("").is_none());
    assert!(session_path("895929a7-6218").unwrap().ends_with("sessions/895929a7-6218.json"));
}

#[test]
fn a_tier_b_instruction_comes_first_and_names_the_weaker_candidates() {
    let hits = vec![hit("harness/skills/gauge/SKILL.md", 0.3, "river gauge")];
    let extra = vec!["No confident tier-A hit: run tier B".to_owned()];
    let out = assemble("river gauge", &hits, &words(), vec![], &extra, &Shape::hook(HashSet::new(), None));
    assert_eq!(out.text.lines().nth(1), Some("No confident tier-A hit: run tier B"), "{}", out.text);
    assert!(out.text.contains("harness/skills/gauge/SKILL.md"), "{}", out.text);
}
