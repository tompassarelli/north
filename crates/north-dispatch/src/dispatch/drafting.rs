//! The draft pass (north#1240): for each ready box in the well-specified band whose own line names a test
//! command, a draft lane on a build host and a small-model session on an owner's plan account that computes the
//! box's change there and records it through the finish call's draft road, on slots this round left over, within
//! the daily bound. It runs each round only when `draft_pass = true` in the orchestration file, and for named
//! issues when `north fleet dispatch --draft REPO#N...` asks.
use super::drafts::{self, Draft, Ground, Outcome};
use super::issues::{Issue, IssueRef};
use super::plan::{self, Capacity, Cloud, Launch, Ready};
use super::scope::Scope;
use super::slots::{Account, Pools};
use super::survey::Survey;
use super::tier::{Route, Tier};
use super::{effects, record, screen};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use toml_edit::{DocumentMut, Item};

const SWITCH: &str = "draft_pass";
const PROVIDER: &str = "claude";
const SPENT_DAYS: u32 = 2;

const DAY_S: f64 = 24.0 * 3600.0;

fn qualifies(issue: &Issue, box_text: &str, category: &str) -> Result<String, String> {
    if let Some(why) = super::band::facets_outside(issue, category) {
        return Err(why);
    }
    if box_text.trim_start().starts_with("Observe:") {
        return Err("an Observe: box reads live state, not the tree".to_owned());
    }
    super::facets::test_command(box_text).ok_or_else(|| "its box names no backticked test command".to_owned())
}

fn box_sha256(box_text: &str) -> String {
    crate::platform::decision_log::sha256_hex(box_text.trim().as_bytes())
}

fn recent<'a>(drafts: &'a [Draft], item: &str, sha: &str, now: f64) -> Option<&'a Draft> {
    let made: Vec<&Draft> = drafts.iter().filter(|draft| draft.item == item && draft.box_sha256 == sha && now - draft.at < DAY_S).collect();
    match made.as_slice() {
        [] => None,
        [only] if matches!(drafts::ground(only), Ground::Moved(_)) => None,
        [.., last] => Some(*last),
    }
}

struct Bound {
    count: usize,
    tokens: u64,
    usd: f64,
}

impl Bound {
    fn configured() -> Self {
        let number = crate::platform::harness::default_number;
        Self { count: number("draft_daily_count") as usize, tokens: number("draft_daily_tokens") as u64, usd: number("draft_daily_usd") }
    }

    fn room(&self, drafts: &[Draft], now: f64) -> Result<(), String> {
        let today: Vec<&Draft> = drafts.iter().filter(|draft| now - draft.at < DAY_S).collect();
        let tokens: u64 = today.iter().filter_map(|draft| draft.tokens).sum();
        let usd: f64 = today.iter().filter_map(|draft| draft.cost_usd).sum();
        if today.len() >= self.count {
            return Err(format!("{} drafts in the last 24 h reach draft_daily_count {}", today.len(), self.count));
        }
        if tokens >= self.tokens {
            return Err(format!("{tokens} draft tokens in the last 24 h reach draft_daily_tokens {}", self.tokens));
        }
        if usd >= self.usd {
            return Err(format!("${usd:.2} of drafts in the last 24 h reaches draft_daily_usd ${:.2}", self.usd));
        }
        Ok(())
    }
}

fn mark_path(draft: &Draft) -> Option<PathBuf> {
    let slug = draft.lane.file_name()?;
    let container = draft.lane.parent()?.parent()?;
    Some(container.join("main/.git/north/lane-drafts").join(format!("{}.json", slug.to_string_lossy())))
}

fn marked(draft: &Draft, mark: &serde_json::Value) -> Draft {
    let mut settled = draft.clone();
    if let Some(outcome) = mark.get("outcome").cloned().and_then(|outcome| serde_json::from_value::<Outcome>(outcome).ok()) {
        settled.outcome = outcome;
        settled.check_exit = matches!(outcome, Outcome::DraftPass | Outcome::PassesOnBase).then_some(0);
    }
    settled.commit = mark.get("commit").and_then(serde_json::Value::as_str).map(str::to_owned).or(settled.commit);
    if let Some(files) = mark.get("files").and_then(|files| serde_json::from_value::<Vec<String>>(files.clone()).ok()).filter(|files| !files.is_empty()) {
        settled.files = files;
    }
    settled
}

fn with_spend(draft: &Draft, spent: Option<(u64, f64)>) -> Draft {
    let mut spent_on = draft.clone();
    if let Some((tokens, usd)) = spent {
        spent_on.tokens = Some(tokens);
        spent_on.cost_usd = Some(usd);
    }
    spent_on
}

fn plan_account(account: &Account) -> bool {
    account.provider() == PROVIDER && account.grant.is_none() && !account.name.ends_with(":api")
}

fn switched_on(cfg: &DocumentMut) -> bool {
    cfg.get(SWITCH).and_then(Item::as_bool).unwrap_or(false)
}

fn box_launched(outcome: &super::Outcome, item: &IssueRef, text: &str) -> bool {
    outcome.launches.iter().any(|launch| launch.item == *item && super::facets::declared_in(&launch.brief, "Box").is_some_and(|named| named.trim() == text.trim()))
}

pub(super) fn pass(survey: &Survey, outcome: &super::Outcome, pools: &mut Pools, named: Option<&[IssueRef]>, dry_run: bool) -> Vec<String> {
    let scope = Scope::parse(&survey.cfg);
    if named.is_none() {
        if !switched_on(&survey.cfg) {
            return Vec::new();
        }
        if !scope.allows_provider(PROVIDER) {
            return vec![format!("no drafts: {}", scope.why_not_provider(PROVIDER))];
        }
        if outcome.skipped.iter().any(|(_, why)| why.starts_with("ready, not placed")) {
            return vec!["no drafts: ready work this round was not placed, so no slot is idle".to_owned()];
        }
    }
    let scope = scope.admitting(PROVIDER);
    let accounts: Vec<Account> = survey.accounts.iter().filter(|account| plan_account(account)).cloned().collect();
    let cloud = Cloud::Closed("drafts run on build hosts".to_owned());
    let capacity = Capacity { here: &survey.here, cloud: &cloud, accounts: &accounts, charge: survey.rules.charge, spend: &survey.spend, scope: &scope };
    let directory = record::directory();
    let spent = crate::ledger::session_spend(SPENT_DAYS);
    let mut lines: Vec<String> = spent.as_ref().err().map(|error| format!("draft spend unread from the runs ledger, so only the count bounds drafts: {error}")).into_iter().collect();
    let spent = spent.unwrap_or_default();
    let mut kept: Vec<Draft> = drafts::read(&directory).iter().map(|draft| with_spend(draft, spent.get(&draft.name).copied())).collect();
    let bound = Bound::configured();
    for item in named.unwrap_or_default().iter().filter(|item| !outcome.ready.contains(item)) {
        let why = outcome.skipped.iter().find(|(skipped, _)| skipped == item).map_or("not an open issue dispatch reads", |(_, why)| why.as_str());
        lines.push(format!("no draft for {item}: {why}"));
    }
    for issue in survey.issues.iter().filter(|issue| outcome.ready.contains(&issue.reference()) && named.is_none_or(|named| named.contains(&issue.reference()))) {
        let item = issue.reference();
        let none = std::collections::BTreeMap::new();
        let taken: BTreeSet<String> = survey.box_holds.get(&item).unwrap_or(&none).keys().chain(survey.box_tries.get(&item).unwrap_or(&none).keys()).cloned().collect();
        let Ok(routed) = super::tier::route(issue, &survey.history, &survey.ledger).map_err(|why| lines.push(format!("no draft for {item}: {why}"))) else { continue };
        for text in issue.free_boxes(&taken, survey.at).filter(|text| !box_launched(outcome, &item, text)) {
            let check = match qualifies(issue, text, &routed.category) {
                Ok(check) => check,
                Err(why) => {
                    lines.push(format!("no draft for {item} box `{}`: {why}", super::box_words(text)));
                    continue;
                }
            };
            let sha = box_sha256(text);
            if let Some(made) = recent(&kept, &item.to_string(), &sha, survey.at) {
                lines.push(format!("no draft for {item} box `{}`: {} drafted it within 24 h", super::box_words(text), made.name));
                continue;
            }
            if let Err(why) = bound.room(&kept, survey.at) {
                lines.push(format!("no more drafts: {why}"));
                return lines;
            }
            match place(survey, &capacity, issue, text, &routed, pools) {
                Ok(launch) => match start(survey, &launch, issue, text, &check, &sha, dry_run) {
                    Ok((line, draft)) => {
                        lines.push(line);
                        kept.extend(draft);
                    }
                    Err(why) => lines.push(format!("draft {item} box `{}` on {} failed: {why}", super::box_words(text), launch.host)),
                },
                Err(why) => lines.push(format!("no draft for {item} box `{}`: {why}", super::box_words(text))),
            }
        }
    }
    lines
}

fn place(survey: &Survey, capacity: &Capacity, issue: &Issue, text: &str, routed: &Route, pools: &mut Pools) -> Result<Launch, String> {
    let route = Route { tier: Tier::Haiku, effort: None, effort_arm: false, signal: None, cell: None, cell_role: None, why: "a draft runs on the small model on the owner's plan accounts (north#1240)".to_owned(), ..routed.clone() };
    let (route, brief) = screen::written_as(survey, issue, &format!("north fleet dispatch drafts on {}", survey.here), Some(text), route)?;
    let candidates: Vec<plan::Candidate> = super::candidates(survey, capacity.scope, &brief).into_iter().filter(|candidate| survey.laptop.as_deref() != Some(candidate.host.as_str())).collect();
    plan::place(&Ready { item: issue.reference(), brief, code_only: false, route }, capacity, &candidates, pools)
}

fn draft_name(item: &IssueRef, text: &str) -> String {
    let slug = crate::threads::claims::box_slug(text).unwrap_or_default();
    format!("draft-{}-{}-{slug}", crate::threads::catalog::checkout(&item.repo), item.number)
}

fn git(directory: &Path, arguments: &[&str]) -> Result<String, String> {
    let output = std::process::Command::new("git").arg("-C").arg(directory).args(arguments).output().map_err(|error| error.to_string())?;
    if output.status.success() { Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned()) } else { Err(String::from_utf8_lossy(&output.stderr).trim().to_owned()) }
}

fn start(survey: &Survey, launch: &Launch, issue: &Issue, text: &str, check: &str, sha: &str, dry_run: bool) -> Result<(String, Option<Draft>), String> {
    let item = issue.reference();
    let name = draft_name(&item, text);
    let line = format!("draft {item} box `{}` on {} {} as {name}, check `{check}`", super::box_words(text), launch.host, launch.account);
    if dry_run {
        return Ok((format!("would {line}"), None));
    }
    let home = std::env::var("HOME").map_err(|_| "HOME is unset".to_owned())?;
    let container = PathBuf::from(&home).join("code").join(crate::threads::catalog::checkout(&item.repo));
    let checkout = container.join("main");
    git(&checkout, &["fetch", "-q", "origin", "main"])?;
    let base = git(&checkout, &["rev-parse", "refs/remotes/origin/main"])?;
    let lane = container.join("worktrees").join(&name);
    let lane_text = lane.to_string_lossy().into_owned();
    let words = |words: &[&str]| words.iter().map(|word| (*word).to_owned()).collect::<Vec<String>>();
    effects::north_on(survey, &launch.host, &words(&["lane", "sync", &checkout.to_string_lossy()]))?;
    effects::north_on(survey, &launch.host, &words(&["lane", "new", "--issue", &item.to_string(), "--from", &base, "--draft", &name, "--check", check, &lane_text]))?;
    if let Err(why) = effects::north_on(survey, &launch.host, &words(&["lane", "seed", &lane_text])) {
        println!("draft {name}: lane seed failed, so its check compiles cold: {why}");
    }
    let mut draft = Draft {
        name: name.clone(),
        at: crate::pacing::placement::now(),
        item: item.to_string(),
        box_text: text.to_owned(),
        box_sha256: sha.to_owned(),
        checkout,
        base: base.clone(),
        commit: None,
        files: Vec::new(),
        check: check.to_owned(),
        outcome: Outcome::Running,
        check_exit: None,
        attempt: None,
        host: launch.host.clone(),
        lane,
        session: Some(name.clone()),
        tokens: None,
        cost_usd: None,
    };
    let note = format!(
        "\n\nDraft (north#1240): this launch computes a draft of the box above, not a landing. Your working directory {lane_text} is the box's draft lane on base {base}; edit there, not in a lane of your own, and end with one finish call whose check is `{check}`. The call commits in the lane, records the draft and queues nothing; report Done when it has."
    );
    let drafted = Launch { brief: format!("{}{note}", launch.brief), ..launch.clone() };
    let started = effects::fleet_launch_in(&drafted, &name, survey, lane_text);
    if started.is_err() {
        draft.outcome = Outcome::Abandoned;
    }
    drafts::append(&record::directory(), &draft)?;
    started.map(|_| (line, Some(draft)))
}

pub fn settle(cfg: &DocumentMut, here: &str, draft: &Draft, spent: Option<(u64, f64)>) -> Draft {
    let mark = (draft.outcome == Outcome::Running).then(|| read_mark(cfg, here, draft)).flatten();
    let read = mark.map_or_else(|| draft.clone(), |mark| marked(draft, &mark));
    with_spend(&read, spent)
}

fn read_mark(cfg: &DocumentMut, here: &str, draft: &Draft) -> Option<serde_json::Value> {
    let path = mark_path(draft)?;
    let text = if draft.host == here {
        std::fs::read_to_string(&path).ok()?
    } else {
        let ssh = crate::pacing::placement::gate::machines(cfg).into_iter().find(|machine| machine.name == draft.host)?.ssh;
        let output = crate::platform::process::run_within("ssh", &["-o", "BatchMode=yes", "-o", "ConnectTimeout=5", &ssh, "cat", &path.to_string_lossy()], std::time::Duration::from_secs(15))?;
        output.status.success().then(|| String::from_utf8_lossy(&output.stdout).into_owned())?
    };
    serde_json::from_str(&text).ok()
}

pub fn listed(cfg: &DocumentMut, here: &str) -> Vec<(Draft, Ground)> {
    let directory = record::directory();
    let spent = crate::ledger::session_spend(SPENT_DAYS).unwrap_or_default();
    drafts::read(&directory)
        .iter()
        .map(|draft| {
            let settled = settle(cfg, here, draft, spent.get(&draft.name).copied());
            let ground = drafts::ground(&settled);
            (settled, ground)
        })
        .collect()
}
