use super::registry::parse;
use super::tick::{Red, State, decide};
use super::verdict::{Verdict, judge};

const GOALS: &str = r#"
[[goal]]
name = "p99"
source = "AGENTS.md: p99 at most 10 ms"
oracle = "echo 9"
unit = "ms"
max = 10
class = "contract"

[[goal]]
name = "worst"
source = "AGENTS.md: worst frame at most 14 ms"
oracle = "echo 12"
unit = "ms"
max = 10
class = "debt"
ceiling = 14
owner = "makgora#168"
revisit = "a landing that raises it"

[[goal]]
name = "roster"
source = "AGENTS.md: roster holds at 29"
oracle = "echo 29"
min = 29
class = "contract"
"#;

#[test]
fn a_contract_is_red_past_its_bound_and_debt_is_red_only_past_its_ceiling() {
    let goals = parse(GOALS).expect("goals parse");
    assert_eq!(judge(&goals[0], Ok(9.0)), Verdict::Green(9.0));
    assert_eq!(judge(&goals[0], Ok(10.5)), Verdict::RedContract(10.5));
    assert_eq!(judge(&goals[1], Ok(12.0)), Verdict::RedDebt(12.0));
    assert_eq!(judge(&goals[1], Ok(14.5)), Verdict::RedOverCeiling(14.5));
    assert_eq!(judge(&goals[2], Ok(28.0)), Verdict::RedContract(28.0));
    assert!(!judge(&goals[1], Ok(12.0)).failing());
    assert!(judge(&goals[0], Err("no answer".into())).failing());
}

#[test]
fn a_goal_without_a_class_rule_is_refused() {
    let contract_with_ceiling = GOALS.replace("class = \"contract\"\n\n[[goal]]\nname = \"worst\"", "class = \"contract\"\nceiling = 12\n\n[[goal]]\nname = \"worst\"");
    assert!(parse(&contract_with_ceiling).is_err_and(|found| found[0].contains("never red")));
    let debt_without_owner = GOALS.replace("owner = \"makgora#168\"\n", "");
    assert!(parse(&debt_without_owner).is_err_and(|found| found[0].contains("owner")));
    let ceiling_not_looser = GOALS.replace("ceiling = 14", "ceiling = 9");
    assert!(parse(&ceiling_not_looser).is_err());
}

fn red(set: &str, text: &str) -> Red {
    Red { set: set.into(), text: text.into() }
}

#[test]
fn the_tick_tells_once_when_a_goal_turns_red_again_when_the_set_changes_or_hours_pass_and_not_for_a_changed_value_of_the_same_goal() {
    let (told, state) = decide(State::default(), 1_000, || red("a", "goals red (1): a"));
    assert_eq!(told.as_deref(), Some("goals red (1): a"));
    let (told, state) = decide(state, 1_100, || unreachable!("the oracles ran again within ten minutes"));
    assert_eq!(told, None);
    let (told, state) = decide(state, 2_000, || red("a", "goals red (1): a"));
    assert_eq!(told, None);
    let (told, state) = decide(state, 3_000, || red("a;b", "goals red (2): a; b"));
    assert_eq!(told.as_deref(), Some("goals red (2): a; b"));
    let (told, state) = decide(state, 3_700, Red::default);
    assert_eq!(told, None);
    let (told, _) = decide(state, 4_400, || red("a", "goals red (1): a"));
    assert_eq!(told.as_deref(), Some("goals red (1): a"));
    {
        let (_, state) = decide(State::default(), 100_000, || red("a", "goals red (1): a 13.1"));
        let (told, state) = decide(state, 100_700, || red("a", "goals red (1): a 13.4"));
        assert_eq!(told, None);
        let mut measured = false;
        let (_, _) = decide(state, 50_000, || {
            measured = true;
            red("a", "goals red (1): a 13.4")
        });
        assert!(measured, "a state written in the future must not hold the oracles off");
    }
}
