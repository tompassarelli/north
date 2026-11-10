//! `north glance` (layer 0): the one line that says whether anything is broken, whether money is all right and whether anything waits on Tom; the fleet screen's top segment prints the same line.
use serde::Deserialize;

use crate::error::{NorthError, NorthResult};
use crate::foundation::clock::Timestamp;
use crate::usage::outlook::{self, Runway};

const USAGE: &str = "usage: north glance";
const HOUR_S: i64 = 3_600;
const TICK: &str = "north/goals/tick.json";

pub fn run(words: &[String]) -> NorthResult<()> {
    match words {
        [] => {
            println!("{}", line());
            Ok(())
        }
        _ => Err(NorthError::Usage(USAGE.into())),
    }
}

pub fn lines() -> Vec<String> {
    vec![line()]
}

pub fn line() -> String {
    read(Timestamp::now()).glance()
}

pub struct Snapshot {
    bars: Result<Vec<String>, String>,
    goals_red: Result<usize, String>,
    accounts: Result<Vec<outlook::AccountOutlook>, String>,
    credit: Vec<north_core::credit::Standing>,
    gates: Result<usize, String>,
}

impl Snapshot {
    pub fn glance(&self) -> String {
        format!("glance: {} · {} · {}", self.health(), self.money(), self.waiting())
    }

    fn health(&self) -> String {
        let mut trouble = Vec::new();
        match &self.bars {
            Ok(missed) if missed.is_empty() => {}
            Ok(missed) => trouble.push(format!("{} bar{} missed ({})", missed.len(), plural(missed.len()), missed.join(", "))),
            Err(why) => trouble.push(format!("bars unread ({why})")),
        }
        match &self.goals_red {
            Ok(0) => {}
            Ok(red) => trouble.push(format!("{red} goal{} red", plural(*red))),
            Err(why) => trouble.push(format!("goals unread ({why})")),
        }
        if trouble.is_empty() {
            "health ok".to_owned()
        } else {
            format!("health: {}", trouble.join(", "))
        }
    }

    fn money(&self) -> String {
        let runway = match &self.accounts {
            Ok(accounts) => soonest(accounts),
            Err(why) => format!("accounts unread ({why})"),
        };
        format!("{runway}; {}", credit_left(&self.credit))
    }

    fn waiting(&self) -> String {
        match &self.gates {
            Err(why) => format!("gates unread ({why})"),
            Ok(0) => "no gate waits on Tom".to_owned(),
            Ok(1) => "1 gate waits on Tom".to_owned(),
            Ok(waits) => format!("{waits} gates wait on Tom"),
        }
    }
}

fn read(now: Timestamp) -> Snapshot {
    Snapshot { bars: bars(now), goals_red: goals_red(), accounts: accounts(now), credit: credit(now), gates: gates(now) }
}

fn bars(now: Timestamp) -> Result<Vec<String>, String> {
    let lines = crate::cli::scoreboard::ready::lines(now.unix_seconds(), HOUR_S)?;
    Ok(lines.into_iter().filter(|line| line.verdict == crate::cli::scoreboard::ready::Verdict::Missed).map(|line| format!("{}/{}", line.repo, line.bar)).collect())
}

fn goals_red() -> Result<usize, String> {
    let path = north_goals::tick::state_file(TICK);
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(0),
        Err(error) => return Err(error.to_string()),
    };
    let state: Ticked = serde_json::from_str(&text).map_err(|error| error.to_string())?;
    Ok(state.red.set.split(';').filter(|goal| !goal.is_empty()).count())
}

fn accounts(now: Timestamp) -> Result<Vec<outlook::AccountOutlook>, String> {
    let db = crate::usage::open_ledger(&crate::usage::profiles::home(), None, &|| {})?;
    outlook::claude_accounts(&db, &crate::platform::orchestration::path(), now)
}

fn credit(now: Timestamp) -> Vec<north_core::credit::Standing> {
    north_core::credit::standing(&crate::platform::orchestration::path(), &crate::watch::state_home(), now).orgs
}

fn gates(now: Timestamp) -> Result<usize, String> {
    let fleet = crate::cli::fleet::open_fleet().map_err(|error| error.user_message())?;
    let now_ms = u64::try_from(now.unix_millis()).unwrap_or_default();
    let books = fleet.digest(now_ms.saturating_sub(86_400_000)).map_err(|error| error.user_message())?;
    let local = books.open_gates().filter(|gate| gate.default_at_ms().is_none()).count();
    let elsewhere = fleet.gates_elsewhere().map_err(|error| error.user_message())?.iter().filter(|gate| gate.default_at_ms().is_none()).count();
    Ok(local + elsewhere)
}

fn soonest(accounts: &[outlook::AccountOutlook]) -> String {
    let short = accounts.iter().filter(|account| matches!(account.runway, Runway::Out | Runway::Empties { .. })).min_by(|left, right| minutes(&left.runway).total_cmp(&minutes(&right.runway)));
    match short.map(|account| (&account.account, &account.runway)) {
        None => "every account lasts to its reset".to_owned(),
        Some((account, Runway::Out)) => format!("{account} out until reset"),
        Some((account, Runway::Empties { minutes })) => format!("{account} runs out in {}", span(*minutes)),
        Some(_) => "every account lasts to its reset".to_owned(),
    }
}

fn minutes(runway: &Runway) -> f64 {
    match runway {
        Runway::Out => 0.0,
        Runway::Empties { minutes } => *minutes,
        Runway::Lasts | Runway::Unknown => f64::INFINITY,
    }
}

fn span(minutes: f64) -> String {
    let minutes = minutes.max(0.0).round() as u64;
    match minutes {
        0..=59 => format!("{minutes}m"),
        60..=1_439 => format!("{}h{:02}m", minutes / 60, minutes % 60),
        _ => format!("{}d", minutes / 1_440),
    }
}

fn credit_left(orgs: &[north_core::credit::Standing]) -> String {
    let open: Vec<&north_core::credit::Standing> = orgs.iter().filter(|org| !org.exhausted).collect();
    let left: f64 = open.iter().map(|org| org.left).sum();
    let mut text = match (orgs.len(), open.len()) {
        (0, _) => "no api credit".to_owned(),
        (_, 0) => "api exhausted".to_owned(),
        (1, _) => format!("api {}", open[0].figure()),
        _ => format!("api ${left:.2} left"),
    };
    let exhausted = orgs.len() - open.len();
    if orgs.len() > 1 && exhausted > 0 {
        text.push_str(&format!(", {exhausted} exhausted"));
    }
    text
}

fn plural(count: usize) -> &'static str {
    if count == 1 { "" } else { "s" }
}

#[derive(Deserialize)]
struct Ticked {
    #[serde(default)]
    red: Red,
}

#[derive(Default, Deserialize)]
struct Red {
    #[serde(default)]
    set: String,
}
