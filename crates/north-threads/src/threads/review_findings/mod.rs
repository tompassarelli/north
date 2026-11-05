
use std::io::{Read, Write};

use rusqlite::{Connection, params};

use super::verb::{Args, Context, Failure, Outcome, immediate, open, report};

pub const VERBS: [&str; 1] = ["review-finding"];

pub const KINDS: [&str; 3] = ["fact", "scenario", "architecture"];
pub const SEVERITIES: [&str; 3] = ["must-fix", "should-fix", "nit"];
pub const VERDICTS: [&str; 2] = ["accepted", "rejected"];

pub(crate) const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS review_findings(
  id INTEGER PRIMARY KEY,
  review TEXT NOT NULL CHECK(length(review) > 0),
  plan TEXT NOT NULL DEFAULT '',
  line INTEGER NOT NULL DEFAULT 0 CHECK(line >= 0),
  kind TEXT NOT NULL CHECK(kind IN ('fact','scenario','architecture')),
  lens TEXT NOT NULL DEFAULT '',
  tier TEXT NOT NULL CHECK(length(tier) > 0),
  severity TEXT NOT NULL CHECK(severity IN ('must-fix','should-fix','nit')),
  verdict TEXT NOT NULL CHECK(verdict IN ('accepted','rejected')),
  finding TEXT NOT NULL DEFAULT '',
  recorded_at TEXT NOT NULL,
  UNIQUE(review, kind, line, finding)
) STRICT;
CREATE INDEX IF NOT EXISTS review_findings_recorded ON review_findings(recorded_at);";

const USAGE: &str = "usage: threads review-finding record --review ID --kind fact|scenario|architecture --tier TIER --severity must-fix|should-fix|nit --verdict accepted|rejected [--lens LENS] [--plan PATH] [--line N] [--finding TEXT]";

pub fn run(argv: &[String], context: &Context, _: &mut dyn Read, out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    let result = match argv.get(1).map(String::as_str) {
        Some("record") => record(&argv[2..], context),
        _ => Err(Failure::Usage(USAGE.into())),
    };
    report(&argv[0], result, out, err)
}

fn required(args: &Args, flag: &str) -> Result<String, Failure> {
    args.get(flag).filter(|value| !value.is_empty()).map(str::to_owned).ok_or_else(|| Failure::Usage(format!("{flag} is required")))
}

fn one_of(args: &Args, flag: &str, allowed: &[&str]) -> Result<String, Failure> {
    let value = required(args, flag)?;
    if allowed.contains(&value.as_str()) { Ok(value) } else { Err(Failure::Usage(format!("{flag} {value} is not one of {}", allowed.join(", ")))) }
}

fn record(argv: &[String], context: &Context) -> Outcome {
    let args = Args::parse(argv, &["--review", "--plan", "--line", "--kind", "--lens", "--tier", "--severity", "--verdict", "--finding"], &[])?;
    let review = required(&args, "--review")?;
    let kind = one_of(&args, "--kind", &KINDS)?;
    let tier = required(&args, "--tier")?;
    let severity = one_of(&args, "--severity", &SEVERITIES)?;
    let verdict = one_of(&args, "--verdict", &VERDICTS)?;
    let line: i64 = match args.get("--line") {
        None => 0,
        Some(text) => text.parse().ok().filter(|line| *line >= 0).ok_or_else(|| Failure::Usage(format!("--line {text} is not a line number")))?,
    };
    if context.replica {
        return Err(Failure::Refused("this ledger is a replica; review findings are written on the ledger host".into()));
    }
    let mut db = open(context.database, true)?;
    let tx = immediate(&mut db)?;
    tx.execute_batch(SCHEMA)?;
    let added = tx.execute(
        "INSERT OR IGNORE INTO review_findings(review, plan, line, kind, lens, tier, severity, verdict, finding, recorded_at) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
        params![review, args.get("--plan").unwrap_or(""), line, kind, args.get("--lens").unwrap_or(""), tier, severity, verdict, args.get("--finding").unwrap_or(""), context.now.iso()],
    )?;
    tx.commit()?;
    Ok(if added == 0 { format!("review {review}: {kind} finding at L{line} already recorded\n") } else { format!("review {review}: recorded {kind} {severity} {verdict} from {tier}\n") })
}

#[derive(Debug, Default, Eq, PartialEq)]
pub struct Tally {
    pub kind: String,
    pub tier: String,
    pub lens: String,
    pub reviews: i64,
    pub findings: i64,
    pub accepted: i64,
    pub rejected: i64,
    pub must_fix_accepted: i64,
}

fn has_table(db: &Connection) -> rusqlite::Result<bool> {
    db.query_row("SELECT count(*) FROM sqlite_master WHERE type = 'table' AND name = 'review_findings'", [], |row| row.get::<_, i64>(0)).map(|count| count > 0)
}

pub fn tallies(db: &Connection, since: &str) -> rusqlite::Result<Vec<Tally>> {
    if !has_table(db)? {
        return Ok(Vec::new());
    }
    let mut statement = db.prepare(
        "SELECT kind, tier, lens, count(DISTINCT review), count(*), sum(verdict = 'accepted'), sum(verdict = 'rejected'), sum(verdict = 'accepted' AND severity = 'must-fix')
         FROM review_findings WHERE recorded_at >= ?1 GROUP BY kind, tier, lens
         ORDER BY CASE kind WHEN 'fact' THEN 0 WHEN 'scenario' THEN 1 ELSE 2 END, tier, lens",
    )?;
    statement
        .query_map(params![since], |row| {
            Ok(Tally { kind: row.get(0)?, tier: row.get(1)?, lens: row.get(2)?, reviews: row.get(3)?, findings: row.get(4)?, accepted: row.get(5)?, rejected: row.get(6)?, must_fix_accepted: row.get(7)? })
        })?
        .collect()
}

pub fn render(tallies: &[Tally]) -> String {
    if tallies.is_empty() {
        return "review findings: none recorded; record each with north threads review-finding record\n".to_owned();
    }
    let mut text = String::from("review findings by kind (kind, reviewer tier and lens: reviews, findings, accepted, rejected, accepted must-fix)\n");
    for kind in KINDS {
        let rows: Vec<&Tally> = tallies.iter().filter(|tally| tally.kind == kind).collect();
        let total = |field: fn(&Tally) -> i64| rows.iter().map(|tally| field(tally)).sum::<i64>();
        text += &format!("{kind}: findings {}  accepted {}  rejected {}  must-fix accepted {}\n", total(|t| t.findings), total(|t| t.accepted), total(|t| t.rejected), total(|t| t.must_fix_accepted));
        for tally in rows {
            let handler = if tally.lens.is_empty() { tally.tier.clone() } else { format!("{} lens {}", tally.tier, tally.lens) };
            text += &format!(
                "  {handler:<36} reviews {}  findings {}  accepted {}  rejected {}  must-fix accepted {}\n",
                tally.reviews, tally.findings, tally.accepted, tally.rejected, tally.must_fix_accepted
            );
        }
    }
    text
}

pub fn recommend(context: &Context, since: &str) -> Outcome {
    let db = open(context.database, false)?;
    Ok(render(&tallies(&db, since)?))
}
