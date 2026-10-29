//! `north ops dividend`: each open issue labelled `bet` names the checks of its Dividend section;
//! a run evaluates them over the last seven days, comments PAYING or DECAYED on the issue once a
//! day, and sends the proxy a fleet message for each bet that comments DECAYED.
use crate::foundation::clock::Timestamp;
use checks::{Check, Facts, Standing, Verdict, evaluate};

mod bets;
mod checks;
mod live;
#[cfg(test)]
mod tests;

const USAGE: &str = "usage: north ops dividend [--dry-run] [CHECK]  (CHECK: cloud-lane, laptop-share, game-share, release-closed)";

/// A bet issue as the board lists it; `comments` are the bodies already posted on it.
#[derive(Clone, Debug)]
pub(crate) struct Bet {
    pub number: u64,
    pub title: String,
    pub body: String,
    pub comments: Vec<String>,
}

/// The issue tracker a run reads the bets from and comments on, and the proxy it alerts.
pub(crate) trait Board {
    fn bets(&self) -> Result<Vec<Bet>, String>;
    fn comment(&mut self, number: u64, text: &str) -> Result<(), String>;
    fn alert(&mut self, text: &str) -> Result<(), String>;
}

pub(crate) struct Report {
    pub lines: Vec<String>,
    pub failed: bool,
}

fn marker(date: &str) -> String {
    format!("north ops dividend {date}:")
}

fn overall(verdicts: &[(Check, Verdict)]) -> Standing {
    let any = |standing| {
        verdicts
            .iter()
            .any(|(_, verdict)| verdict.standing == standing)
    };
    if any(Standing::Decayed) {
        Standing::Decayed
    } else if any(Standing::Paying) {
        Standing::Paying
    } else {
        Standing::Unread
    }
}

fn detail(verdicts: &[(Check, Verdict)]) -> String {
    verdicts
        .iter()
        .map(|(check, verdict)| {
            format!("- {} {}: {}", check.name(), verdict.standing, verdict.line)
        })
        .collect::<Vec<_>>()
        .join("\n")
}

pub(crate) fn tick(board: &mut dyn Board, facts: &Facts, date: &str, dry_run: bool) -> Report {
    let bets = match board.bets() {
        Ok(bets) => bets,
        Err(error) => {
            return Report {
                lines: vec![format!("bet issues unread: {error}")],
                failed: true,
            };
        }
    };
    let mut report = Report {
        lines: Vec::new(),
        failed: false,
    };
    for bet in bets {
        let named = bets::checks(&bet.body);
        if named.is_empty() {
            report.lines.push(format!(
                "#{} {}: its Dividend section names no `north ops dividend` check",
                bet.number, bet.title
            ));
            continue;
        }
        let verdicts: Vec<(Check, Verdict)> = named
            .into_iter()
            .map(|check| (check, evaluate(check, facts)))
            .collect();
        let standing = overall(&verdicts);
        report
            .lines
            .push(format!("#{} {}: {standing}", bet.number, bet.title));
        report
            .lines
            .extend(detail(&verdicts).lines().map(|line| format!("  {line}")));
        if standing != Standing::Unread {
            post(board, &bet, (standing, &verdicts), (date, dry_run), &mut report);
        }
    }
    report
}

fn post(
    board: &mut dyn Board,
    bet: &Bet,
    (standing, verdicts): (Standing, &[(Check, Verdict)]),
    (date, dry_run): (&str, bool),
    report: &mut Report,
) {
    let marker = marker(date);
    if bet
        .comments
        .iter()
        .any(|comment| comment.starts_with(&marker))
    {
        report.lines.push(format!("  already commented {date}"));
        return;
    }
    if dry_run {
        report
            .lines
            .push(format!("  would comment {standing} on #{}", bet.number));
        return;
    }
    let text = format!("{marker} {standing}\n\n{}\n", detail(verdicts));
    match board.comment(bet.number, &text) {
        Ok(()) => report
            .lines
            .push(format!("  commented {standing} on #{}", bet.number)),
        Err(error) => {
            report.failed = true;
            report.lines.push(format!("  not commented: {error}"));
            return;
        }
    }
    if standing == Standing::Decayed {
        let message = format!(
            "north ops dividend: bet #{} {} is DECAYED on {date}\n{}",
            bet.number,
            bet.title,
            detail(verdicts)
        );
        match board.alert(&message) {
            Ok(()) => report.lines.push("  alerted the proxy".into()),
            Err(error) => {
                report.failed = true;
                report.lines.push(format!("  proxy not alerted: {error}"));
            }
        }
    }
}

pub(crate) fn run(arguments: &[String]) -> u8 {
    let (dry_run, rest): (Vec<&String>, Vec<&String>) = arguments
        .iter()
        .partition(|argument| *argument == "--dry-run");
    let dry_run = !dry_run.is_empty();
    match rest.as_slice() {
        [] => all(dry_run),
        [name] => match Check::parse(name) {
            Some(check) => one(check),
            None => {
                eprintln!("{USAGE}");
                2
            }
        },
        _ => {
            eprintln!("{USAGE}");
            2
        }
    }
}

fn all(dry_run: bool) -> u8 {
    let facts = live::facts();
    let date = Timestamp::from_unix_seconds_f64(facts.at).iso_date();
    let report = tick(&mut live::Gh, &facts, &date, dry_run);
    report.lines.iter().for_each(|line| println!("{line}"));
    u8::from(report.failed)
}

fn one(check: Check) -> u8 {
    let verdict = evaluate(check, &live::facts());
    println!("{} {}: {}", check.name(), verdict.standing, verdict.line);
    match verdict.standing {
        Standing::Paying => 0,
        Standing::Decayed => 1,
        Standing::Unread => 3,
    }
}
