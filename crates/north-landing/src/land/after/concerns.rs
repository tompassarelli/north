use std::collections::{BTreeMap, HashMap};
use std::path::Path;

use north_concerns::attribute::{self, Answer, first_red};
use north_concerns::oracle::Fault;
use north_concerns::registry::{self, Concern, Reads};
use north_base::foundation::pair::{self, Compared, Pair, Reading};

use super::super::candidate::{self, git};
use super::super::lander::{Lander, state_home};
use super::super::sheriff;
use crate::landing::after_landing::Failure;

const GATE: &str = "concern-revert";
const GATES: &str = "harness/gates.toml";

pub struct Landed<'a> {
    pub project: &'a str,
    pub main: &'a Path,
    pub tree: &'a Path,
    pub scratch: &'a Path,
    pub base: &'a str,
    pub head: &'a str,
    pub north: &'a str,
}

/// A red contract concern read at the first commit of the landed range where it reads red and at that commit's parent, by one north in one run.
pub struct Weighed {
    pub concern: String,
    pub commit: String,
    pub parent: String,
    pub compared: Compared,
}

impl Weighed {
    fn verdicts(&self) -> String {
        format!("{} reads {} at the parent {} and {} at {}", self.concern, twice(&self.compared.base), self.parent, twice(&self.compared.candidate), self.commit)
    }

    fn outputs(&self) -> String {
        let runs = self.compared.base.iter().map(|reading| (&self.parent, reading)).chain(self.compared.candidate.iter().map(|reading| (&self.commit, reading)));
        runs.filter_map(|(commit, reading)| match reading {
            Reading::NoVerdict(said) => Some(format!("\n\nthe oracle's output at {commit}:\n{said}")),
            _ => None,
        })
        .collect()
    }
}

fn twice(runs: &[Reading; 2]) -> String {
    if runs[0] == runs[1] { format!("{} twice", runs[0].word()) } else { format!("{} then {}", runs[0].word(), runs[1].word()) }
}

fn outcome(pair: Pair) -> &'static str {
    match pair {
        Pair::Regressed => "a regression",
        Pair::Fixed => "a fix",
        Pair::SameRed => "red at both",
        Pair::RedChanged => "red at both, another way at the landing",
        Pair::SameGreen => "green at both",
        Pair::NoVerdict => "no verdict",
        Pair::TwoAnswers => "two answers in one tree, so its oracle is nondeterministic and blames no commit",
    }
}

fn reading(answer: Answer) -> Reading {
    match answer {
        Answer::Green(value) => Reading::Green(value.to_string()),
        Answer::Red(value) => Reading::Red(value.to_string()),
        Answer::NoVerdict(fault) if fault.output.is_empty() => Reading::NoVerdict(fault.why),
        Answer::NoVerdict(fault) => Reading::NoVerdict(format!("{}\n{}", fault.why, fault.output)),
    }
}

/// The north running this after-landing run, which reads every verdict it acts on.
pub fn this_north() -> String {
    std::env::current_exe().and_then(std::fs::canonicalize).map_or_else(|_| "north".to_owned(), |path| path.display().to_string())
}

struct Probe<'a> {
    scratch: &'a Path,
    concerns: &'a [Concern],
    seen: HashMap<String, Vec<Answer>>,
}

impl Probe<'_> {
    fn answer(&mut self, commit: &str, index: usize) -> Answer {
        let (scratch, concerns) = (self.scratch, self.concerns);
        let answers = self.seen.entry(commit.to_owned()).or_insert_with(|| match git(scratch, &["checkout", "-q", "--detach", commit]) {
            Ok(_) => attribute::answers(scratch, concerns),
            Err(why) => concerns.iter().map(|_| Answer::NoVerdict(Fault { why: format!("{commit} does not check out"), output: why.clone() })).collect(),
        });
        answers[index].clone()
    }
}

fn settled(tree: &Path, contracts: &[Concern]) -> Vec<Concern> {
    let first = attribute::answers(tree, contracts);
    for (concern, answer) in contracts.iter().zip(&first) {
        if let Answer::NoVerdict(fault) = answer {
            println!("after-landing: contract concern {} gave no verdict, so it reverts nothing: {}\n{}", concern.name, fault.why, fault.output);
        }
    }
    let red: Vec<Concern> = contracts.iter().zip(&first).filter(|(_, answer)| answer.red()).map(|(concern, _)| concern.clone()).collect();
    if red.is_empty() {
        return red;
    }
    let again = attribute::answers(tree, &red);
    red.into_iter().zip(again).filter(|(_, answer)| answer.red()).map(|(concern, _)| concern).collect()
}

/// Reverts, through the landing queue, each landed commit that turned a contract concern red: red in the landed tree twice, and read
/// green twice at its parent and red twice the same way at itself by this north in this run (`pair::compare`). A concern whose
/// oracle reads host state, or that gives no verdict at either commit, reverts nothing; a refused revert is reported with both
/// verdicts. The concern list and the `concern-revert` gate's mode are read from the landing's base, so no landing can exempt
/// itself by editing them; while that gate is in shadow it logs the reverts it would make.
pub fn gate(landed: &Landed, link: &dyn Fn(&Path) -> Result<(), String>) -> Vec<Failure> {
    if !north_concerns::PROJECTS.contains(&landed.project) {
        return Vec::new();
    }
    let failure = |output: String| vec![Failure { gate: "concerns".to_owned(), output, check: format!("north check concerns {}", landed.project) }];
    let contracts = match at_base(landed, registry::PATH).map_or(Ok(Vec::new()), |text| attribute::contracts(&text)) {
        Ok(contracts) => contracts,
        Err(why) => return failure(format!("concerns.toml at {} does not read: {why}", landed.base)),
    };
    let red = attributable(settled(landed.tree, &contracts), landed.head);
    if red.is_empty() {
        return Vec::new();
    }
    let candidate = landed.scratch.with_file_name(format!("{}-candidate", landed.scratch.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default()));
    let trees = [landed.scratch, candidate.as_path()];
    let added = trees.iter().try_for_each(|tree| git(landed.main, &["worktree", "add", "--quiet", "--detach", &tree.to_string_lossy(), landed.base]).and_then(|_| link(tree)));
    let found = added.and_then(|()| culprits(landed, &red, &candidate));
    for tree in trees {
        let _ = git(landed.main, &["worktree", "remove", "--force", &tree.to_string_lossy()]);
    }
    let lander = Lander::new(landed.main, Vec::new());
    match found.and_then(|found| Ok((found, lander?))) {
        Ok((found, lander)) => {
            let (flips, refused) = flips(found);
            let refusals = refused.iter().map(|weighed| refusal(landed, weighed));
            if !reverts(landed) {
                flips.iter().for_each(|(commit, weighed)| would_revert(landed, commit, weighed));
                return refusals.collect();
            }
            flips.iter().map(|(commit, weighed)| revert(landed, &lander, commit, weighed)).chain(refusals).collect()
        }
        Err(why) => failure(format!("cannot attribute the red contract concerns of {}: {why}", landed.head)),
    }
}

/// The text of `path` at the landing's base, or none when the base has no such file.
fn at_base(landed: &Landed, path: &str) -> Option<String> {
    git(landed.main, &["show", &format!("{}:{path}", landed.base)]).ok()
}

/// Whether the `concern-revert` gate of the base's harness/gates.toml enforces; a landing that edits the file changes the mode only for the landings after it.
fn reverts(landed: &Landed) -> bool {
    let document = at_base(landed, GATES).and_then(|text| text.parse::<toml_edit::DocumentMut>().ok());
    document.is_some_and(|document| document["gate"][GATE]["mode"].as_str() == Some("enforce"))
}

fn would_revert(landed: &Landed, commit: &str, weighed: &[Weighed]) {
    let verdicts: Vec<String> = weighed.iter().map(Weighed::verdicts).collect();
    println!("north land: contract concern {}, read by {}; the concern-revert gate is in shadow, so {commit} is not reverted", verdicts.join("; "), landed.north);
    let at = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |elapsed| elapsed.as_secs());
    let concerns: Vec<&str> = weighed.iter().map(|weighed| weighed.concern.as_str()).collect();
    let line = serde_json::json!({"gate": GATE, "at": at, "project": landed.project, "base": landed.base, "head": landed.head, "would_revert": commit, "concerns": concerns, "verdicts": verdicts, "north": landed.north});
    let gates = state_home().join("north/gates");
    let written = std::fs::create_dir_all(&gates).and_then(|()| std::fs::OpenOptions::new().create(true).append(true).open(gates.join(format!("{GATE}.jsonl")))).and_then(|mut file| std::io::Write::write_all(&mut file, format!("{line}\n").as_bytes()));
    if let Err(error) = written {
        eprintln!("north land: could not log the concern revert of {commit}: {error}");
    }
}

fn attributable(red: Vec<Concern>, head: &str) -> Vec<Concern> {
    let (commit, host): (Vec<Concern>, Vec<Concern>) = red.into_iter().partition(|concern| concern.reads == Reads::Commit);
    for concern in &host {
        println!("after-landing: contract concern {} is red at {head}, but its oracle reads this host's state rather than the commit, so it reverts no landing; the fleet tick pages the proxy", concern.name);
    }
    commit
}

fn culprits(landed: &Landed, red: &[Concern], candidate: &Path) -> Result<Vec<Weighed>, String> {
    let mut probe = Probe { scratch: landed.scratch, concerns: red, seen: HashMap::new() };
    let range = format!("{}..{}", landed.base, landed.head);
    let commits: Vec<String> = git(landed.main, &["rev-list", "--reverse", "--first-parent", &range])?.lines().map(str::to_owned).collect();
    let mut weighed = Vec::new();
    for (index, concern) in red.iter().enumerate() {
        if probe.answer(landed.base, index).red() {
            continue;
        }
        let Some(at) = first_red(&commits, |commit| probe.answer(commit, index).red()) else { continue };
        let parent = if at == 0 { landed.base.to_owned() } else { commits[at - 1].clone() };
        git(landed.scratch, &["checkout", "-q", "--detach", &parent])?;
        git(candidate, &["checkout", "-q", "--detach", &commits[at]])?;
        let check = |tree: &Path| attribute::answers(tree, std::slice::from_ref(concern)).into_iter().next().map_or_else(|| Reading::NoVerdict("no answer".to_owned()), reading);
        let compared = pair::compare(&check, landed.scratch, candidate);
        weighed.push(Weighed { concern: concern.name.clone(), commit: commits[at].clone(), parent, compared });
    }
    Ok(weighed)
}

fn flips(weighed: Vec<Weighed>) -> (BTreeMap<String, Vec<Weighed>>, Vec<Weighed>) {
    let (flipped, refused): (Vec<Weighed>, Vec<Weighed>) = weighed.into_iter().partition(|weighed| weighed.compared.regressed());
    let mut by_commit: BTreeMap<String, Vec<Weighed>> = BTreeMap::new();
    for weighed in flipped {
        by_commit.entry(weighed.commit.clone()).or_default().push(weighed);
    }
    (by_commit, refused)
}

fn refusal(landed: &Landed, weighed: &Weighed) -> Failure {
    Failure {
        gate: format!("concerns: {} not reverted", weighed.concern),
        output: format!("contract concern {}, read by {} in one after-landing run: {}; a concern reverts only a landing whose parent reads green twice and which reads red twice the same way, so {} stays on main{}", weighed.verdicts(), landed.north, outcome(weighed.compared.pair), weighed.commit, weighed.outputs()),
        check: format!("north check concerns {}", landed.project),
    }
}

fn revert(landed: &Landed, lander: &Lander, commit: &str, weighed: &[Weighed]) -> Failure {
    let names = weighed.iter().map(|weighed| weighed.concern.as_str()).collect::<Vec<_>>().join(", ");
    let verdicts = weighed.iter().map(Weighed::verdicts).collect::<Vec<_>>().join("; ");
    let outcome = queue(landed, lander, commit, &names, &verdicts);
    Failure { gate: format!("concerns: {names}"), output: format!("contract concerns {verdicts}, read by {} in one after-landing run; {}", landed.north, outcome.unwrap_or_else(|why| format!("{commit} was not reverted: {why}"))), check: format!("north check concerns {}", landed.project) }
}

fn queue(landed: &Landed, lander: &Lander, commit: &str, names: &str, verdicts: &str) -> Result<String, String> {
    let message = git(landed.main, &["log", "-1", "--format=%B", commit])?;
    if sheriff::is_revert(&message) {
        return Err("it is itself a revert".to_owned());
    }
    let marker = lander.queue.join("concern-reverts").join(commit);
    if marker.exists() {
        return Ok(format!("the revert of {commit} is already queued"));
    }
    let worktree = candidate::lander_worktree(landed.main, "lander-concerns")?;
    candidate::ensure(landed.main, &worktree)?;
    if !sheriff::without(&worktree, landed.head, commit)? {
        return Err(format!("it no longer reverts cleanly onto {}", landed.head));
    }
    let branch = format!("concerns-revert-{}", &commit[..commit.len().min(12)]);
    git(&worktree, &["checkout", "-q", "-B", &branch])?;
    let subject = message.lines().next().unwrap_or_default();
    let text = format!("Revert {subject}\n\nContract concern {verdicts}, both read by {} in one after-landing run: green twice at this landing's parent and red twice the same way at it, so the after-landing run reverts it through the landing queue.\n\nReverts: {commit}\nConcern-red: {names}", landed.north);
    git(&worktree, &["-c", "core.hooksPath=/dev/null", "commit", "-q", "--no-verify", "-m", &text])?;
    sheriff::queue_revert(lander, &worktree, &branch)?;
    let _ = std::fs::create_dir_all(marker.parent().unwrap_or(&lander.queue)).and_then(|()| std::fs::write(&marker, &branch));
    Ok(format!("{commit} is reverted as {branch} through the landing queue"))
}

#[cfg(test)]
#[path = "concerns_tests.rs"]
mod tests;
