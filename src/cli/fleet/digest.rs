//! `north digest [--since 24h]`: one digest of the fleet store for Tom, on request and after each usage reset.
use crate::error::{NorthError, NorthResult};
use crate::fleet::digest_report::{Facts, GateLine, render};
use crate::fleet::gates::Gate;
use crate::fleet::registry::Fleet;
use crate::fleet::runtime::now_ms;
use crate::router::moment::Moment;

const USAGE: &str = "Usage: north digest [--since DURATION] (30m, 4h, 1d; default 24h)";

pub(super) fn run(fleet: &Fleet, words: &[&str]) -> NorthResult<()> {
    let label = match words {
        [] => "24h",
        ["--since", window] => window,
        _ => return Err(NorthError::Usage(USAGE.into())),
    };
    let window_ms = crate::cli::gate::duration_ms(label).ok_or_else(|| NorthError::Usage(USAGE.into()))?;
    let now = now_ms();
    let books = fleet.digest(now.saturating_sub(window_ms))?;
    let mut gates: Vec<GateLine> = books.open_gates().map(|gate| line(&format!("{}:{}", fleet.host(), gate.id), gate)).collect();
    gates.extend(fleet.gates_elsewhere()?.iter().map(|row| GateLine {
        label: row.label(),
        session: row.request.session.clone(),
        question: row.request.question.clone(),
        recommended: recommended(&row.request.options, row.request.recommend),
        default_at_ms: row.default_at_ms(),
    }));
    let sessions = fleet.cached_sessions()?.unwrap_or_default();
    print!("{}", render(&Facts { now: Moment::now(), now_ms: now, since_label: label, landed: &books.landed, sessions: &sessions, gates: &gates }));
    Ok(())
}

fn line(label: &str, gate: &Gate) -> GateLine {
    GateLine {
        label: label.to_owned(),
        session: gate.request.session.clone(),
        question: gate.request.question.clone(),
        recommended: recommended(&gate.request.options, gate.request.recommend),
        default_at_ms: gate.default_at_ms(),
    }
}

fn recommended(options: &[String], number: usize) -> String {
    number.checked_sub(1).and_then(|index| options.get(index)).map_or_else(|| "none".to_owned(), |option| format!("{number}. {option}"))
}
