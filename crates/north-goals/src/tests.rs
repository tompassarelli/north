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
fn a_contract_is_red_past_its_bound_debt_only_past_its_ceiling_and_a_debt_over_it_moves_only_landings_touching_its_paying_paths_to_the_front() {
    let goals = parse(GOALS).expect("goals parse");
    assert_eq!(judge(&goals[0], Ok(9.0)), Verdict::Green(9.0));
    assert_eq!(judge(&goals[0], Ok(10.5)), Verdict::RedContract(10.5));
    assert_eq!(judge(&goals[1], Ok(12.0)), Verdict::RedDebt(12.0));
    assert_eq!(judge(&goals[1], Ok(14.5)), Verdict::RedOverCeiling(14.5));
    assert_eq!(judge(&goals[2], Ok(28.0)), Verdict::RedContract(28.0));
    assert!(!judge(&goals[1], Ok(12.0)).failing());
    assert!(judge(&goals[0], Err("no answer".into())).failing());
    let root = std::path::Path::new(".");
    let elsewhere = vec!["README.md".to_owned()];
    let paying = vec!["README.md".to_owned(), "crates/north-landing/src/lib.rs".to_owned()];
    let found = super::ceiling::paid(root, OWED, &paying).expect("goals parse");
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(found[0].starts_with("slow reads 90 s, over its ceiling 60 s"));
    assert!(super::ceiling::paid(root, OWED, &elsewhere).expect("goals parse").is_empty(), "a landing that pays nothing is not moved and not refused");
    assert!(super::ceiling::paid(root, &OWED.replace("echo 90", "echo 50"), &paying).expect("goals parse").is_empty(), "a debt within its ceiling moves no landing");
    the_landing_debt_goal_reads_the_projected_wait_of_the_last_few_landings_and_admits_any_lane_once_it_is_under_its_ceiling();
}

#[test]
fn a_goal_without_a_class_rule_is_refused() {
    let contract_with_ceiling = GOALS.replace("class = \"contract\"\n\n[[goal]]\nname = \"worst\"", "class = \"contract\"\nceiling = 12\n\n[[goal]]\nname = \"worst\"");
    assert!(parse(&contract_with_ceiling).is_err_and(|found| found[0].contains("never red")));
    let debt_without_owner = GOALS.replace("owner = \"makgora#168\"\n", "");
    assert!(parse(&debt_without_owner).is_err_and(|found| found[0].contains("owner")));
    let bar = "[[goal]]\nname = \"ready-landing-p90\"\nsource = \"north#641: p90 at most 60 s\"\nunit = \"s\"\nmax = 60\nclass = \"ready\"\n";
    assert_eq!(parse(bar).expect("a ready bar needs no oracle")[0].class, super::registry::Class::Ready);
    assert!(parse(&bar.replace("class = \"ready\"", "class = \"ready\"\noracle = \"echo 1\"")).is_err_and(|found| found[0].contains("ready bar")));
    let ceiling_not_looser = GOALS.replace("ceiling = 14", "ceiling = 9");
    assert!(parse(&ceiling_not_looser).is_err());
    let text = GOALS.replace("class = \"contract\"\n\n[[goal]]\nname = \"worst\"", "class = \"contract\"\npays = [\"src/\"]\n\n[[goal]]\nname = \"worst\"");
    assert!(parse(&text).is_err_and(|found| found[0].contains("paying paths")));
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

const OWED: &str = r#"
[[goal]]
name = "slow"
source = "README.md: lands fast"
oracle = "echo 90"
unit = "s"
max = 20
class = "debt"
ceiling = 60
owner = "north#641"
revisit = "a week over"
pays = ["crates/north-landing/"]

[[goal]]
name = "load"
source = "AGENTS.md: admission"
oracle = "echo 99"
max = 12
class = "debt"
ceiling = 24
owner = "north#641"
revisit = "a day over"
"#;


fn the_landing_debt_goal_reads_the_projected_wait_of_the_last_few_landings_and_admits_any_lane_once_it_is_under_its_ceiling() {
    let goals = parse(include_str!("../../../goals.toml")).expect("the north goals parse");
    let landing = goals.iter().find(|goal| goal.name == "landing-projected").expect("a landing projected-wait goal");
    assert!(landing.oracle.contains("--metric landing-projected"), "{}", landing.oracle);
    let last: usize = landing.oracle.split("--last ").nth(1).and_then(|rest| rest.split_whitespace().next()).and_then(|count| count.parse().ok()).expect("the oracle reads a fixed number of landings");
    assert!(last <= 5, "a trailing window of {last} landings cannot recover while the gate holds landings back");
    let debt = landing.debt.as_ref().expect("a debt goal has a ceiling");
    let reading = |value: f64| format!("[[goal]]\nname = \"{}\"\nsource = \"{}\"\noracle = \"echo {value}\"\nunit = \"{}\"\nmax = {}\nclass = \"debt\"\nceiling = {}\nowner = \"{}\"\nrevisit = \"{}\"\npays = {:?}\n", landing.name, landing.source, landing.unit, landing.bound.limit(), debt.ceiling, debt.owner, debt.revisit, debt.pays);
    let root = std::path::Path::new(".");
    let elsewhere = vec!["README.md".to_owned()];
    let paying = vec![format!("{}lib.rs", debt.pays[0])];
    assert_eq!(super::ceiling::paid(root, &reading(debt.ceiling + 1.0), &paying).expect("goals parse").len(), 1, "a projected wait over the ceiling moves a lane that pays it to the front");
    assert!(super::ceiling::paid(root, &reading(debt.ceiling + 1.0), &elsewhere).expect("goals parse").is_empty(), "any other lane keeps its place and is never refused");
    assert!(super::ceiling::paid(root, &reading(debt.ceiling), &paying).expect("goals parse").is_empty(), "a projected wait at the ceiling moves no lane");
}
