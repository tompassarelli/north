//! `north threads replay-staffing`: the tier the old and the new rule would have picked for each first-assignment run.
use std::collections::BTreeMap;

use super::read::{Raw, escalated, raw_runs};
use super::recommend::recommend;
use super::verbs::{since_start, stat_usd};
use super::{Knobs, Pick, Run, tier_key};
use crate::threads::verb::{Args, Context, Failure, Outcome, open};

const OLD_TIERS: [&str; 6] = ["haiku", "low", "medium", "high", "xhigh", "max"];

#[derive(Default)]
struct Change {
    runs: usize,
    new: usize,
    new_ok: usize,
    old: usize,
    old_ok: usize,
    other: usize,
    pick: Option<Pick>,
}

fn remember(held: &mut BTreeMap<String, String>, cell: &str, tier: Option<&String>) {
    match tier {
        Some(tier) => held.insert(cell.to_owned(), tier.clone()),
        None => held.remove(cell),
    };
}

fn stat_text(tier: &str, pick: &Pick) -> Option<String> {
    let (_, stat) = pick.stats.iter().find(|(key, _)| key == tier)?;
    Some(format!("{tier} {}% of {}, {} (priced n={})", stat.rate, stat.n, stat_usd(stat.usd), stat.usd_n))
}

fn shown(tier: &Option<String>) -> String {
    tier.clone().unwrap_or_else(|| "None".to_owned())
}

pub(super) fn run(argv: &[String], context: &Context) -> Outcome {
    let args = Args::parse(argv, &["--since"], &[])?;
    if !args.positional.is_empty() {
        return Err(Failure::Usage("usage: threads replay-staffing [--since DATE]".into()));
    }
    let since = since_start(args.get("--since").unwrap_or("7d"), context.now);
    let knobs = Knobs::registry();
    let db = open(context.database, false)?;
    let mut rows: Vec<Raw> = raw_runs(&db, "follows IS NULL AND outcome != 'split'", &[])?.into_iter().filter(|raw| !raw.unjudged()).collect();
    rows.sort_by(|x, y| x.ended.cmp(&y.ended));
    let mut followed: BTreeMap<String, String> = BTreeMap::new();
    for (follows, ended) in db.prepare("SELECT follows, ended FROM runs WHERE role = 'worker' AND follows IS NOT NULL")?.query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)))?.collect::<Result<Vec<_>, _>>()? {
        followed.entry(follows).and_modify(|first| *first = (*first).clone().min(ended.clone())).or_insert(ended);
    }
    let escalated = escalated(&db)?;
    let (mut old_held, mut new_held) = (BTreeMap::new(), BTreeMap::new());
    let mut decisions = 0;
    let mut changes: BTreeMap<(String, Option<String>, Option<String>), Change> = BTreeMap::new();
    for x in rows.iter().filter(|raw| raw.ended >= since && raw.cells[0] != "unknown") {
        let at = x.started.clone().filter(|started| !started.is_empty()).unwrap_or_else(|| x.ended.clone());
        let ok_then = |raw: &Raw| raw.succeeded(followed.get(&raw.agent).is_some_and(|first| *first < at));
        let mut asked: [Option<String>; 5] = Default::default();
        for (index, value) in x.cells.iter().enumerate() {
            asked[index] = Some(value.clone()).filter(|value| value != "-");
        }
        let hist: Vec<&Raw> = rows.iter().filter(|raw| raw.cells[0] == x.cells[0] && raw.ended < at).collect();
        let new_runs: Vec<Run> = hist.iter().map(|raw| raw.recommend_run(ok_then(raw))).collect();
        let old_runs: Vec<Run> = hist.iter().filter(|raw| OLD_TIERS.contains(&raw.tier.as_str())).map(|raw| raw.recommend_run(ok_then(raw))).collect();
        let old = recommend(&knobs, &old_runs, &asked, &old_held, false);
        remember(&mut old_held, &old.cell, old.tier.as_ref());
        let new = recommend(&knobs, &new_runs, &asked, &new_held, true);
        remember(&mut new_held, &new.cell, new.tier.as_ref());
        decisions += 1;
        if old.tier == new.tier {
            continue;
        }
        let change = changes.entry((new.cell.clone(), old.tier.clone(), new.tier.clone())).or_default();
        change.runs += 1;
        let actual = Some(tier_key(&x.tier));
        let ok = x.succeeded(escalated.contains(&x.agent));
        if actual == new.tier {
            change.new += 1;
            change.new_ok += usize::from(ok);
        } else if actual == old.tier {
            change.old += 1;
            change.old_ok += usize::from(ok);
        } else {
            change.other += 1;
        }
        change.pick = Some(new);
    }
    Ok(summarize(&since, decisions, &changes))
}

fn summarize(since: &str, decisions: usize, changes: &BTreeMap<(String, Option<String>, Option<String>), Change>) -> String {
    let mut text = format!("replay-staffing since {}: {decisions} first-assignment runs, picks from strictly earlier runs\n", if since.is_empty() { "all" } else { since });
    let mut ordered: Vec<_> = changes.iter().collect();
    ordered.sort_by(|(xk, x), (yk, y)| y.runs.cmp(&x.runs).then_with(|| (&xk.0, shown(&xk.1), shown(&xk.2)).cmp(&(&yk.0, shown(&yk.1), shown(&yk.2)))));
    for ((cell, old, new), change) in ordered {
        let pick = change.pick.as_ref().expect("a change has a pick");
        text += &format!("[{cell}]  {} -> {}  runs {}  rule {}\n", shown(old), shown(new), change.runs, pick.rule);
        for tier in [new, old].into_iter().flatten() {
            if let Some(line) = stat_text(tier, pick) {
                text += &format!("  {line}\n");
            }
        }
        text += &format!("  actual: new pick {} ({} ok), old pick {} ({} ok), other {}\n", change.new, change.new_ok, change.old, change.old_ok, change.other);
    }
    let sum = |field: fn(&Change) -> usize| changes.values().map(field).sum::<usize>();
    let cells: std::collections::BTreeSet<&String> = changes.keys().map(|key| &key.0).collect();
    text += &format!(
        "changed {} of {decisions} decisions in {} cells; actual tier was the new pick {} ({} ok), the old pick {} ({} ok), another {}\n",
        sum(|c| c.runs), cells.len(), sum(|c| c.new), sum(|c| c.new_ok), sum(|c| c.old), sum(|c| c.old_ok), sum(|c| c.other)
    );
    text
}
