use super::{Detector, Finding};

/// One machine whose code a landed commit changed this week while narrating an incident, as
/// `north check machines --fixes` reports it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Fixed {
    pub machine: String,
    pub modelled: bool,
    pub week: String,
    pub fixes: Vec<String>,
}

#[derive(Default)]
pub struct Facts {
    pub owner: Option<String>,
    pub fixed: Vec<Fixed>,
}

pub fn census(report: &str) -> Vec<Fixed> {
    report
        .lines()
        .filter_map(|line| {
            let words: Vec<&str> = line.split_whitespace().collect();
            match words.as_slice() {
                ["machine-fix:", machine, model, "week", week, "fixes", fixes] => Some(Fixed { machine: (*machine).to_owned(), modelled: *model == "modelled", week: (*week).to_owned(), fixes: fixes.split(',').map(str::to_owned).collect() }),
                _ => None,
            }
        })
        .collect()
}

/// One box per machine and week: a modelled machine's fix is a bug its model let through, so the box extends the
/// model, and an unmodelled machine's fix is the signal to model it.
pub fn fixed(facts: &Facts) -> Vec<Finding> {
    facts
        .fixed
        .iter()
        .map(|fixed| {
            let machine = &fixed.machine;
            let landed = format!("{} landed commit{} narrate an incident in {machine}'s code this week ({})", fixed.fixes.len(), if fixed.fixes.len() == 1 { "" } else { "s" }, fixed.fixes.join(", "));
            let (title, detail, done_when) = if fixed.modelled {
                (
                    format!("Extend the {machine} model to the fixes its check let through"),
                    format!(
                        "{landed} since its model last changed, while `north check machines` passed, so its promises covered none of them. Read each fix and state the promise it restores; add it to the machine's promises in src/cli/machines.rs or to the scope in crates/north-machine-model so the check fails with the fix reverted, and where no promise over one instance can state it, record the fix and the promise it needs under the machine in docs/rationale/state-machines.md."
                    ),
                    "`north check machines` fails with each fix reverted and passes on main, or docs/rationale/state-machines.md records why no promise of the model covers it.".to_owned(),
                )
            } else {
                (
                    format!("Model {machine}"),
                    format!("{landed} and `north check machines` does not explore it. Make its machine type reachable from src/cli/machines.rs, name its promises there and generate its chart with `north check machines --write`."),
                    format!("`north check machines` prints a {machine} line with 0 broken and docs/rationale/state-machines/{machine}.md exists."),
                )
            };
            Finding { detector: Detector::MachineFix, key: format!("machine-fix:{machine}:{}", fixed.week), owner: facts.owner.clone(), title, detail, done_when, instances: fixed.fixes.clone() }
        })
        .collect()
}
