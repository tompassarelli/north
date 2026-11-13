//! The board's facts, read once per snapshot: Work from the fleet store and books, Accounts from the usage table and
//! the one credit producer, Infra from `north infra`'s read, and Health from the readiness bars and the habit hook.
use crate::cli::infra::report::{self, Price};
use crate::fleet::board::{Board, Panel};
use crate::fleet::books::Landing;
use crate::fleet::gates::Gate;
use crate::fleet::registry::Fleet;
use crate::foundation::clock::Timestamp;
use crate::usage::outlook::{self, Runway};

use super::escalations::Escalations;
use super::scoreboard::ready::{self, Verdict};
use super::spend::Spend;
use super::Screen;

const NAMED_FAILURES: usize = 3;
const DETAIL_ROWS: usize = 5;
const WHOLE_FROM_COMMIT: &str = "291d551ce";
const WHOLE_FROM_UNIX_SECONDS: i64 = 1_794_592_920;

#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Work {
    pub sessions: Vec<SessionLine>,
    pub gates: Vec<Gate>,
    pub landed: Vec<Landing>,
    pub held: usize,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct SessionLine {
    pub name: String,
    pub status: &'static str,
    pub task: Option<String>,
}

pub(super) fn work(fleet: &Fleet, since_ms: u64) -> Result<Work, String> {
    let digest = fleet.digest(since_ms).map_err(|error| error.user_message())?;
    let sessions = fleet.sessions().map_err(|error| error.user_message())?;
    Ok(Work {
        sessions: sessions.into_iter().map(|record| SessionLine { status: fleet.status(&record).label(), name: record.name, task: record.task_ref }).collect(),
        gates: digest.gates.into_iter().filter(Gate::is_open).collect(),
        landed: digest.landed,
        held: fleet.held_since(since_ms).len(),
    })
}

pub(super) fn of(screen: &Screen) -> Board {
    let window = window_words(&screen.window);
    let hours = ((screen.now - screen.since).max(60) as f64) / 3_600.0;
    let landings = screen.work.as_ref().ok().map(|work| work.landed.len());
    let closed = screen.runs.as_ref().ok().map(|(_, report)| report.closed);
    let whole = whole_from_note(screen.since);
    let first_line = match (landings, closed) {
        (Some(landings), Some(closed)) => format!("{landings} landings ({:.1}/h) · {closed} boxes closed ({:.1}/h{whole})", landings as f64 / hours, closed as f64 / hours),
        (Some(landings), None) => format!("{landings} landings ({:.1}/h) · boxes unread", landings as f64 / hours),
        (None, Some(closed)) => format!("landings unread · {closed} boxes closed ({:.1}/h{whole})", closed as f64 / hours),
        (None, None) => "landings and boxes unread".to_owned(),
    };
    Board {
        first_line,
        panels: vec![work_panel(screen, &window), accounts_panel(screen, &window), infra_panel(screen), health_panel(screen, &window)],
    }
}

fn work_panel(screen: &Screen, window: &str) -> Panel {
    let mut panel = Panel::new("Work", "the fleet store could not be read");
    let Ok(work) = &screen.work else {
        panel.unread.push(screen.work.as_ref().err().cloned().unwrap_or_default());
        return panel;
    };
    let count = |status: &str| work.sessions.iter().filter(|session| session.status == status).count();
    let landings = work.landed.len();
    panel.headline = format!("{} working, {} idle, {} open gates, {} held, {landings} landings in the last {window}", count("working"), count("idle"), work.gates.len(), work.held);
    for gate in work.gates.iter().take(DETAIL_ROWS) {
        let recommended = gate.request.recommend.checked_sub(1).and_then(|index| gate.request.options.get(index)).cloned().unwrap_or_else(|| "none".to_owned());
        let when = gate.default_at_ms().map_or_else(|| "waits for Tom".to_owned(), |at| format!("default {}", Timestamp::from_unix_millis(i64::try_from(at).unwrap_or(0)).local_twelve_hour()));
        panel.lines.push(format!("gate {} {}: {} ({}; {when})", gate.id, gate.request.session, gate.request.question, recommended));
    }
    for session in work.sessions.iter().filter(|session| session.status != "exited").take(DETAIL_ROWS) {
        panel.lines.push(format!("session {} {} {}", session.name, session.status, session.task.as_deref().unwrap_or("no task")));
    }
    for landing in work.landed.iter().rev().take(DETAIL_ROWS) {
        let sha: String = landing.sha.chars().take(9).collect();
        let wall = landing.wall_ms.map_or_else(|| "wall unmeasured".to_owned(), |wall| format!("wall {}", span_ms(wall)));
        panel.lines.push(format!("landed {} {sha} {wall}", landing.task_ref));
    }
    match &screen.attempts {
        Ok(attempts) => {
            let landed = attempts.iter().filter(|attempt| attempt.succeeded).count();
            if !attempts.is_empty() {
                panel.lines.push(format!("first tries: {} of {} landed", landed, attempts.len()));
            }
        }
        Err(why) => panel.unread.push(format!("first tries unread: {why}")),
    }
    if let Ok((runs, _)) = &screen.runs {
        let escalations = Escalations::of(&screen.launches, runs, screen.since as f64, screen.now as f64);
        if !escalations.rows.is_empty() {
            panel.lines.push(escalations.summary(window));
            for row in escalations.rows.iter().take(DETAIL_ROWS) {
                let lift = row.lift_delta().map_or_else(|| "-".to_owned(), |delta| format!("{:+.0} pts", delta * 100.0));
                panel.lines.push(format!("escalation {}: {} climbed, {} landed, lift {lift}", row.rung, row.escalations, first_try(row.landed, row.finished)));
            }
        }
    } else if let Err(why) = &screen.runs {
        panel.unread.push(format!("runs unread: {why}"));
    }
    panel
}

fn accounts_panel(screen: &Screen, window: &str) -> Panel {
    let mut panel = Panel::new("Accounts", "the accounts could not be read");
    let accounts = match &screen.accounts {
        Ok(accounts) => accounts,
        Err(why) => {
            panel.unread.push(why.clone());
            return panel;
        }
    };
    let short = accounts.iter().filter(|account| matches!(account.runway, Runway::Out | Runway::Empties { .. })).count();
    let api_left: f64 = screen.credit.iter().filter(|org| !org.exhausted).map(|org| org.left).sum();
    let spent = Spend::of(screen);
    let spent_words = spent.as_ref().map_or_else(|_| "spend unread".to_owned(), |spent| format!("{} in the last {window}", money(spent.classes.iter().map(|class| class.usd).sum())));
    panel.headline = format!(
        "{}; ${api_left:.2} left on {} api {}; {spent_words}",
        if short == 0 { format!("every account lasts to its reset ({} accounts)", accounts.len()) } else { format!("{short} of {} accounts run out before reset", accounts.len()) },
        screen.credit.iter().filter(|org| !org.exhausted).count(),
        if screen.credit.len() == 1 { "organization" } else { "organizations" },
    );
    let mut rows = vec![vec!["account".to_owned(), "plan".to_owned(), "week left".to_owned(), "use/h".to_owned(), "outlook".to_owned(), "resets".to_owned()]];
    rows.extend(accounts.iter().take(6).map(|account| {
        let plan = screen.billing.as_ref().ok().and_then(|billing| billing.plans.get(&account.account));
        vec![
            account.account.clone(),
            plan.map_or_else(|| "-".to_owned(), |plan| format!("{} {}/month", plan.plan, money(plan.monthly_usd))),
            account.week_used.map_or_else(|| "-".to_owned(), |used| format!("{:.0}%", (100.0 - used).max(0.0))),
            account.burn_per_hour.map_or_else(|| "-".to_owned(), |rate| format!("{rate:.1}%/h")),
            runway(account.runway),
            account.week_resets.map_or_else(|| "-".to_owned(), outlook::weekday_clock),
        ]
    }));
    panel.lines.extend(grid(&rows, &[Align::Left, Align::Left, Align::Right, Align::Right, Align::Left, Align::Left]));
    for org in &screen.credit {
        panel.lines.push(org.line());
    }
    match &spent {
        Ok(spent) => {
            let mut rows = vec![vec!["model and effort".to_owned(), "runs".to_owned(), "spend".to_owned(), "week".to_owned(), "api".to_owned()]];
            rows.extend(spent.classes.iter().take(3).map(|class| vec![class.mix.clone(), class.runs.to_string(), money(class.usd), class.all_pct.map_or_else(|| "-".to_owned(), percent), "-".to_owned()]));
            rows.push(vec!["all runs".to_owned(), spent.classes.iter().map(|class| class.runs).sum::<usize>().to_string(), money(spent.classes.iter().map(|class| class.usd).sum()), "-".to_owned(), money(spent.api.usd)]);
            panel.lines.extend(grid(&rows, &[Align::Left, Align::Right, Align::Right, Align::Right, Align::Right]));
        }
        Err(why) => panel.unread.push(format!("spend unread: {why}")),
    }
    panel
}

fn infra_panel(screen: &Screen) -> Panel {
    let mut panel = Panel::new("Infra", "the infrastructure could not be read");
    let (resources, unread) = match &screen.infra {
        Ok((resources, unread)) => (resources, unread),
        Err(why) => {
            panel.unread.push(why.clone());
            return panel;
        }
    };
    panel.unread.extend(unread.iter().cloned());
    if resources.is_empty() {
        return panel;
    }
    let report = report::Report::of(Timestamp::from_unix_seconds(screen.now).iso(), resources.clone(), unread.clone());
    let machines = resources.iter().filter(|resource| resource.is_machine()).count();
    let idle = resources.iter().filter(|resource| resource.idle()).count();
    panel.headline = format!("{machines} machines, {}/month, {idle} idle", report.total_line());
    let mut rows = vec![vec!["resource".to_owned(), "kind".to_owned(), "per month".to_owned(), "status".to_owned(), "load".to_owned()]];
    rows.extend(resources.iter().take(DETAIL_ROWS * 2).map(|resource| {
        let load = resource.usage.map_or_else(|| "-".to_owned(), |usage| if usage.jobs == 0 { "idle".to_owned() } else { format!("{} jobs", usage.jobs) });
        vec![resource.name.clone(), resource.kind.clone(), resource.price.map_or_else(|| "-".to_owned(), Price::line), resource.status.clone(), load]
    }));
    panel.lines.extend(grid(&rows, &[Align::Left, Align::Left, Align::Right, Align::Left, Align::Left]));
    panel
}

fn health_panel(screen: &Screen, window: &str) -> Panel {
    let mut panel = Panel::new("Health", "the targets could not be read");
    match &screen.bars {
        Ok(lines) => {
            let missed: Vec<&ready::Line> = lines.iter().filter(|line| line.verdict == Verdict::Missed).collect();
            let slipping = lines.iter().filter(|line| line.verdict == Verdict::Slipping).count();
            panel.headline = if missed.is_empty() {
                format!("every one of {} targets is met right now; {slipping} slipping over the last {window}", lines.len())
            } else {
                let named: Vec<&str> = missed.iter().take(NAMED_FAILURES).map(|line| line.plain).collect();
                format!("{} of {} targets missed right now, first {}{}", missed.len(), lines.len(), named.join(", "), if missed.len() > NAMED_FAILURES { format!(" and {} more", missed.len() - NAMED_FAILURES) } else { String::new() })
            };
            let mut rows = vec![vec!["".to_owned(), "measure".to_owned(), "target".to_owned(), "last 5 min".to_owned(), "last 15 min".to_owned(), "last 1 h".to_owned(), format!("last {window}")]];
            rows.extend(missed.iter().take(DETAIL_ROWS * 2).map(|line| vec!["FAIL".to_owned(), line.plain.to_owned(), line.target.clone(), line.five_text.clone(), line.fifteen_text.clone(), line.hour_text.clone(), line.window_text.clone()]));
            let met = lines.iter().filter(|line| line.verdict == Verdict::Met).count();
            if met > 0 {
                rows.push(vec!["pass".to_owned(), format!("{met} more met now and over the last {window}"), String::new(), String::new(), String::new(), String::new(), String::new()]);
            }
            panel.lines.extend(grid(&rows, &[Align::Left, Align::Left, Align::Left, Align::Right, Align::Right, Align::Right, Align::Right]));
        }
        Err(why) => panel.unread.push(why.clone()),
    }
    match &screen.habits {
        Ok(rows) => {
            let rewrites: usize = rows.iter().map(|row| row.rewrites).sum();
            let warnings: usize = rows.iter().map(|row| row.warnings).sum();
            if !rows.is_empty() {
                panel.lines.push(format!("habits: {} and {} on {} {}", count(rewrites, "rewrite"), count(warnings, "warning"), rows.len(), if rows.len() == 1 { "rule" } else { "rules" }));
                for row in rows.iter().take(DETAIL_ROWS) {
                    panel.lines.push(format!("habit {}: {} and {}", row.rule, count(row.rewrites, "rewrite"), count(row.warnings, "warning")));
                }
            }
        }
        Err(why) => panel.unread.push(format!("habits unread: {why}")),
    }
    panel
}

fn runway(runway: Runway) -> String {
    match runway {
        Runway::Out => "out until reset".to_owned(),
        Runway::Empties { minutes } => format!("runs out in {}", span_minutes(minutes)),
        Runway::Lasts => "lasts to reset".to_owned(),
        Runway::Unknown => "no burn rate yet".to_owned(),
    }
}

fn span_minutes(minutes: f64) -> String {
    let minutes = minutes.round() as i64;
    match minutes {
        minutes if minutes < 60 => format!("{minutes} min"),
        minutes if minutes % 60 == 0 => format!("{} h", minutes / 60),
        minutes => format!("{} h {} min", minutes / 60, minutes % 60),
    }
}

fn span_ms(millis: u64) -> String {
    span_minutes(millis as f64 / 60_000.0)
}

pub(super) fn window_words(window: &str) -> String {
    let split = window.find(|character: char| !character.is_ascii_digit()).unwrap_or(window.len());
    let (count, unit) = window.split_at(split);
    match unit {
        "d" if count == "1" => "1 day".to_owned(),
        "d" => format!("{count} days"),
        _ => format!("{count} {unit}"),
    }
}

fn whole_from_note(since: i64) -> String {
    Some(since).filter(|since| *since < WHOLE_FROM_UNIX_SECONDS).map_or_else(String::new, |_| format!("; whole from {WHOLE_FROM_COMMIT} at {}", Timestamp::from_unix_seconds(WHOLE_FROM_UNIX_SECONDS).local_twelve_hour()))
}

fn money(usd: f64) -> String {
    format!("${usd:.2}")
}

fn count(number: usize, word: &str) -> String {
    format!("{number} {word}{}", if number == 1 { "" } else { "s" })
}

fn percent(share: f64) -> String {
    if share > 0.0 && share < 0.5 { "<1%".to_owned() } else { format!("{share:.0}%") }
}

fn first_try(landed: usize, tries: usize) -> String {
    match tries {
        0 => "-".to_owned(),
        tries => format!("{landed} of {tries} ({:.0}%)", landed as f64 / tries as f64 * 100.0),
    }
}

#[derive(Clone, Copy)]
enum Align {
    Left,
    Right,
}

fn grid(rows: &[Vec<String>], align: &[Align]) -> Vec<String> {
    let widths: Vec<usize> = (0..align.len()).map(|column| rows.iter().map(|row| row.get(column).map_or(0, |cell| cell.chars().count())).max().unwrap_or(0)).collect();
    rows.iter()
        .map(|row| {
            let cells: Vec<String> = row.iter().zip(&widths).zip(align).map(|((cell, width), align)| match align {
                Align::Left => format!("{cell:<width$}"),
                Align::Right => format!("{cell:>width$}"),
            }).collect();
            cells.join("  ").trim_end().to_owned()
        })
        .collect()
}
