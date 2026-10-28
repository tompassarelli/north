//! One worker-ledger row into the ledger: a running worker holds its Item, a finished one releases
//! it and becomes a run, a stale one releases it; a recorded run takes only what a backfill adds.
use std::collections::BTreeSet;

use rusqlite::types::Value as Sql;
use rusqlite::{Connection, OptionalExtension, params, params_from_iter};

use super::brief::{CATEGORIES, Fields};
use super::row::{Row, bind, python_repr, text, truthy, value};
use crate::platform::pyjson::Value;
use crate::threads::catalog::CATALOG;
use crate::threads::verb::{Context, Failure, event_as};

const ROLES: [&str; 5] = ["worker", "parent", "proxy", "routine", "probe"];
const STAGES: [&str; 3] = ["staffed", "review_requested", "landed_at"];
const CONTEXT: [&str; 8] = ["role", "peak_ctx", "compactions", "handoff_fires", "handoff_ignored", "input_tokens", "cache_read_tokens", "idle_gap_misses"];
const CREATED: [&str; 2] = ["codex.PostToolUse.spawn_agent", "claude.PostToolUse.Agent"];
const FINISHED: &str = "INSERT INTO runs(agent, item, follows, session, tier, category, started, ended, minutes, eta_min, tokens, peak_ctx, outcome, report, note, landed, arm, spec, scope, surface, verify, lens, flaws, expects_landing, staffed, review_requested, landed_at, role, compactions, handoff_fires, handoff_ignored, input_tokens, cache_read_tokens, idle_gap_misses) VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)";
const BACKFILL: &str = "UPDATE runs SET spec = coalesce(spec, ?), scope = coalesce(scope, ?), surface = coalesce(surface, ?), verify = coalesce(verify, ?), lens = coalesce(lens, ?), flaws = coalesce(flaws, ?), expects_landing = coalesce(?, expects_landing), landed = CASE WHEN coalesce(?, landed_sha) IS NOT NULL THEN 1 ELSE coalesce(?, landed) END, landed_sha = coalesce(?, landed_sha), staffed = coalesce(?, staffed), review_requested = coalesce(?, review_requested), landed_at = coalesce(?, landed_at), item = coalesce(?, item, ?), eta_min = coalesce(eta_min, ?), category = CASE category WHEN 'unknown' THEN coalesce(?, category) ELSE category END WHERE agent = ?";

struct Recorded {
    category: Sql,
    outcome: Sql,
    model: Sql,
    effort: Sql,
    brief: [Sql; 4],
    started: Sql,
}

/// A row with what it says about itself: its brief's fields, its role and whether it is recorded.
struct Run {
    r: Row,
    f: Fields,
    id: Value,
    role: String,
    status: Value,
    original: bool,
}

impl Run {
    fn worker(&self) -> bool {
        self.role == "worker"
    }

    fn agent(&self) -> Result<Sql, Failure> {
        bind(Some(&self.id))
    }

    /// The brief's Item, else the first issue the row's commits and issue commands name (encrypted Codex briefs).
    fn item(&self) -> Option<String> {
        issue_ref(&self.either(self.f.item.as_deref(), "item")).or_else(|| self.r.get("refs").and_then(Value::as_array).and_then(|refs| refs.iter().find_map(issue_ref)))
    }

    /// `brief or r.get(key)`.
    fn either(&self, brief: Option<&str>, key: &str) -> Value {
        brief.filter(|text| !text.is_empty()).map_or_else(|| self.r.get(key).cloned().unwrap_or(Value::Null), Value::from)
    }

    fn facets(&self) -> impl Iterator<Item = Sql> + '_ {
        self.f.facets.iter().map(|facet| text(facet.as_deref())).chain([text(self.f.lens.as_deref())])
    }
}

/// Canonical repo#N, or None when the value is not an issue ref.
fn issue_ref(item: &Value) -> Option<String> {
    let text = item.as_str().filter(|_| truthy(item))?;
    CATALOG.parse_ref(text).ok().map(|(repo, number)| format!("{repo}#{number}"))
}

/// What one ingest recorded, and the runs whose category waits on their issue's labels.
#[derive(Default)]
pub(super) struct Tally {
    pub added: usize,
    pub backfilled: usize,
    pub unlabeled: Vec<(Value, String)>,
    pub unpriced: BTreeSet<String>,
}

pub(super) struct Ingest<'a> {
    pub db: &'a Connection,
    pub context: &'a Context<'a>,
    pub tally: &'a mut Tally,
}

impl Ingest<'_> {
    pub(super) fn row(&mut self, r: Row) -> Result<(), Failure> {
        self.telemetry(&r)?;
        if r.truthy("telemetry_only") {
            return Ok(());
        }
        let (run, recorded) = self.read(r)?;
        let recorded = match recorded {
            Some(prior) if prior.outcome == Sql::Text("running".into()) && run.status == "finished" => {
                self.db.execute("DELETE FROM runs WHERE agent = ?", [run.agent()?])?;
                None
            }
            other => other,
        };
        match recorded {
            Some(prior) if run.r.truthy("backfill") || !run.worker() => self.backfill(&run, &prior),
            Some(_) => Ok(()),
            None => self.record(&run),
        }
    }

    fn telemetry(&mut self, r: &Row) -> Result<(), Failure> {
        match r.get("unpriced_models") {
            Some(Value::Array(models)) => self.tally.unpriced.extend(models.iter().map(|model| model.as_string().map_or_else(|| crate::platform::pyjson::dumps(model), |text| text.lossy().into_owned()))),
            Some(Value::String(models)) => self.tally.unpriced.extend(models.lossy().chars().map(String::from)),
            _ => {}
        }
        if !r.truthy("account") {
            return Ok(());
        }
        for sample in r.get("usage_samples").and_then(Value::as_array).unwrap_or_default() {
            let field = |key: &str| bind(Some(sample.get(key).ok_or_else(|| Failure::Refused(format!("a usage sample has no {key}")))?));
            self.db.execute(
                "INSERT INTO run_usage(agent, request, ts, account, weight) VALUES (?,?,?,?,?) ON CONFLICT(agent, request) DO UPDATE SET ts=excluded.ts, account=excluded.account, weight=excluded.weight",
                params![r.required("id")?, field("request")?, field("ts")?, r.required("account")?, field("weight")?],
            )?;
        }
        self.db.execute(
            "UPDATE runs SET account=?, model=coalesce(?, model), effort=coalesce(?, effort), fast=? WHERE agent=?",
            params![r.required("account")?, r.sql("model")?, r.sql("effort")?, r.fast()?, r.required("id")?],
        )?;
        Ok(())
    }

    /// The row as a run, with what the ledger already holds for it: a recorded brief replaces the
    /// row's, and the observed model and effort carry over when the row names none.
    fn read(&self, mut r: Row) -> Result<(Run, Option<Recorded>), Failure> {
        let status = r.get_or("status", Value::from("finished"));
        let role = r.or("role", Value::from("worker"));
        let Some(role) = role.as_str().filter(|role| ROLES.contains(role)).map(str::to_owned) else {
            return Err(Failure::Refused(format!("unknown run role {}", python_repr(&role))));
        };
        let brief = r.or("brief", Value::from(""));
        let mut f = Fields::read(brief.as_str().ok_or_else(|| Failure::Refused("an ingest row's brief is not text".into()))?);
        let id = r.require("id")?.clone();
        let recorded = self
            .db
            .query_row("SELECT category, outcome, model, effort, brief_text, brief_origin, brief_path, brief_sha256, started FROM runs WHERE agent = ?", [bind(Some(&id))?], |row| {
                Ok(Recorded { category: row.get(0)?, outcome: row.get(1)?, model: row.get(2)?, effort: row.get(3)?, brief: [row.get(4)?, row.get(5)?, row.get(6)?, row.get(7)?], started: row.get(8)? })
            })
            .optional()?;
        let mut original = false;
        if let Some(prior) = &recorded {
            for (key, stored) in [("model", &prior.model), ("effort", &prior.effort)] {
                if r.get(key).is_none() {
                    r.set(key, value(stored));
                }
            }
            original = !matches!(prior.brief[0], Sql::Null);
            if original {
                let Sql::Text(brief) = &prior.brief[0] else { return Err(Failure::Refused("a recorded brief is not text".into())) };
                f = Fields::read(brief);
                for (key, stored) in ["brief_text", "brief_origin", "brief_path", "brief_sha256"].into_iter().zip(&prior.brief) {
                    r.set(key, value(stored));
                }
                r.set("category", f.category.as_deref().map_or(Value::Null, Value::from));
                if prior.brief[1] == Sql::Text(CREATED[0].into()) && !matches!(prior.started, Sql::Null) {
                    r.set("started", value(&prior.started));
                }
            }
        }
        Ok((Run { r, f, id, role, status, original }, recorded))
    }

    fn event(&self, item: &str, kind: &str, run: &Run, data: Value) -> Result<(), Failure> {
        event_as(self.db, Some(item), kind, &run.agent()?, &run.r.required("tier")?, data, self.context.now)?;
        Ok(())
    }

    /// A run already recorded: a backfill or a non-worker report fills what the ledger lacks.
    fn backfill(&mut self, run: &Run, prior: &Recorded) -> Result<(), Failure> {
        let (r, agent) = (&run.r, run.agent()?);
        self.db.execute(
            "UPDATE runs SET model=coalesce(?, model), effort=coalesce(?, effort), tier=coalesce(?, tier), outcome=CASE WHEN outcome='unclear' THEN coalesce(?, outcome) ELSE outcome END WHERE agent=?",
            params![r.sql("model")?, r.sql("effort")?, r.sql("tier")?, r.sql("outcome")?, agent],
        )?;
        let category = run.either(run.f.category.as_deref(), "category");
        let known = category.as_str().filter(|category| CATEGORIES.contains(category));
        if !run.worker() {
            self.db.execute(
                "UPDATE runs SET ended = ?, category = coalesce(?, category), report = coalesce(?, report), outcome = coalesce(?, outcome) WHERE agent = ?",
                params![r.required("ended")?, known, r.sql("report")?, r.sql("outcome")?, agent],
            )?;
        }
        let sets: Vec<String> = CONTEXT.iter().map(|key| if *key == "handoff_ignored" && r.has(key) { format!("{key} = ?") } else { format!("{key} = coalesce(?, {key})") }).collect();
        let mut values = vec![Sql::Text(run.role.clone())];
        values.extend(CONTEXT[1..].iter().map(|key| r.sql(key)).collect::<Result<Vec<_>, _>>()?);
        values.push(agent.clone());
        self.db.execute(&format!("UPDATE runs SET {} WHERE agent = ?", sets.join(", ")), params_from_iter(values))?;
        let item = if run.worker() { run.item() } else { None };
        if let Some(item) = item.as_ref().filter(|_| run.worker() && !run.original && known.is_none() && prior.category == Sql::Text("unknown".into())) {
            self.tally.unlabeled.push((run.id.clone(), item.clone()));
        }
        if run.original {
            self.db.execute(
                "UPDATE runs SET category=?,spec=?,scope=?,surface=?,verify=? WHERE agent=?",
                params![run.f.known_category().unwrap_or("unknown"), run.f.facets[0], run.f.facets[1], run.f.facets[2], run.f.facets[3], agent],
            )?;
        }
        let mut values: Vec<Sql> = run.facets().collect();
        for key in ["flaws", "expects_landing", "landed_sha", "landed", "landed_sha"].into_iter().chain(STAGES) {
            values.push(r.sql(key)?);
        }
        let claimed = if run.worker() { issue_ref(&run.either(run.f.item.as_deref(), "item")) } else { None };
        values.extend([text(claimed.as_deref()), text(item.as_deref()), r.rounded("eta_min")?, text(known), agent]);
        self.tally.backfilled += self.db.execute(BACKFILL, params_from_iter(values))?;
        Ok(())
    }

    /// A run the ledger has not recorded: a running worker holds its issue, anything else ends.
    fn record(&mut self, run: &Run) -> Result<(), Failure> {
        let item = if run.worker() { run.item() } else { None };
        let follows = run.either(run.f.follows.as_deref(), "follows");
        let claim: Option<String> = match &item {
            Some(item) => self.db.query_row("SELECT owner FROM claims WHERE item = ?", [item], |row| row.get(0)).optional()?,
            None => None,
        };
        let mine = claim.as_deref().is_some_and(|owner| run.id == *owner);
        if run.status == "running" && run.worker() {
            if let Some(item) = item.as_deref().filter(|_| !mine) {
                self.hold(item, run, claim.as_deref(), &follows)?;
            }
            return self.created(run, item.as_deref(), &follows);
        }
        if let Some(item) = item.as_deref().filter(|_| mine) {
            self.db.execute("DELETE FROM claims WHERE item = ?", [item])?;
            self.event(item, "release", run, Value::object([("why", run.status.clone())]))?;
        }
        if run.status != "finished" && run.worker() {
            return Ok(());
        }
        self.finished(run, item.as_deref(), follows)
    }

    /// A running worker holds its brief's issue; taking it from someone else is a handoff.
    fn hold(&self, item: &str, run: &Run, claim: Option<&str>, follows: &Value) -> Result<(), Failure> {
        let r = &run.r;
        let prev = claim.map_or_else(|| follows.clone(), Value::from);
        let since = bind(Some(&r.or("started", Value::from(self.context.now.iso()))))?;
        if claim.is_some() {
            self.db.execute("UPDATE claims SET owner = ?, since = ?, eta_min = ? WHERE item = ?", params![run.agent()?, since, r.rounded("eta_min")?, item])?;
        } else {
            self.db.execute("INSERT INTO claims(item, owner, since, eta_min) VALUES (?,?,?,?)", params![item, run.agent()?, since, r.rounded("eta_min")?])?;
            self.event(item, "claim", run, Value::object([("eta_min", r.get_or("eta_min", Value::Null))]))?;
        }
        if truthy(&prev) && prev != run.id {
            self.event(item, "handoff", run, Value::object([("from", prev), ("to", run.id.clone())]))?;
        }
        Ok(())
    }

    /// The running row a native spawn hook writes, kept with its original brief.
    fn created(&mut self, run: &Run, item: Option<&str>, follows: &Value) -> Result<(), Failure> {
        let r = &run.r;
        let created = r.get("brief_origin").and_then(Value::as_str).is_some_and(|origin| CREATED.contains(&origin));
        if !created || r.get("brief_text").and_then(Value::as_str).is_none() {
            return Ok(());
        }
        let category = run.f.category.as_deref().unwrap_or("unknown");
        let f = &run.f.facets;
        self.db.execute(
            "INSERT INTO runs(agent,item,follows,session,tier,category,started,ended,minutes,eta_min,outcome,spec,scope,surface,verify,staffed,role,model,effort,brief_text,brief_origin,brief_path,brief_sha256) VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)",
            params![
                run.agent()?, item, bind(Some(follows))?, r.sql("session")?, r.required("tier")?, category, r.sql("started")?, "", 0, r.sql("eta_min")?, "running", f[0], f[1], f[2], f[3],
                r.sql("staffed")?, run.role, r.sql("model")?, r.sql("effort")?, r.required("brief_text")?, r.required("brief_origin")?, r.sql("brief_path")?, r.sql("brief_sha256")?,
            ],
        )?;
        self.tally.added += 1;
        Ok(())
    }

    /// A finished run, handed over from the item's last run when no handoff named it.
    fn finished(&mut self, run: &Run, item: Option<&str>, follows: Value) -> Result<(), Failure> {
        let raw = run.either(run.f.category.as_deref(), "category");
        let raw = if truthy(&raw) { raw } else { Value::from("unknown") };
        if let Some(item) = item.filter(|_| raw == "unknown" && !run.original) {
            self.tally.unlabeled.push((run.id.clone(), item.to_owned()));
        }
        let category = raw.as_str().filter(|raw| CATEGORIES.contains(raw)).unwrap_or("unknown");
        let mut prev = Value::Null;
        if let Some(item) = item {
            let handed = self.db.query_row("SELECT 1 FROM events WHERE item = ? AND kind = 'handoff' AND json_extract(data, '$.to') = ?", params![item, run.agent()?], |_| Ok(())).optional()?;
            if handed.is_none() {
                let last: Option<Sql> = self.db.query_row("SELECT agent FROM runs WHERE item = ? ORDER BY ended DESC LIMIT 1", [item], |row| row.get(0)).optional()?;
                prev = if truthy(&follows) { follows.clone() } else { last.as_ref().map_or(Value::Null, value) };
            }
        }
        self.db.execute(FINISHED, params_from_iter(finished_values(run, item, &follows, category)?))?;
        let r = &run.r;
        self.db.execute("UPDATE runs SET account=?, model=?, effort=?, fast=?, landed_sha=? WHERE agent=?", params![r.sql("account")?, r.sql("model")?, r.sql("effort")?, r.fast()?, r.sql("landed_sha")?, run.agent()?])?;
        self.db.execute(
            "UPDATE runs SET brief_text=?, brief_origin=?, brief_path=?, brief_sha256=? WHERE agent=?",
            params![r.sql("brief_text")?, r.sql("brief_origin")?, r.sql("brief_path")?, r.sql("brief_sha256")?, run.agent()?],
        )?;
        if let Some(item) = item.filter(|_| truthy(&prev) && prev != run.id) {
            self.event(item, "handoff", run, Value::object([("from", prev), ("to", run.id.clone())]))?;
        }
        self.tally.added += 1;
        Ok(())
    }
}

fn finished_values(run: &Run, item: Option<&str>, follows: &Value, category: &str) -> Result<Vec<Sql>, Failure> {
    let r = &run.r;
    let arm = run.f.arm.as_deref().unwrap_or_default().to_lowercase();
    let mut values = vec![run.agent()?, text(item), bind(Some(follows))?, r.sql("session")?, r.required("tier")?, text(Some(category)), r.sql("started")?, r.required("ended")?, r.required("actual_min")?, r.round("eta_min")?];
    values.extend([r.sql("tokens")?, r.sql("peak_ctx")?, r.required("outcome")?, bind(Some(&r.or("report", Value::from(""))))?, r.sql("note")?, r.sql("landed")?, text(Some(&arm).filter(|arm| !arm.is_empty()).map(String::as_str))]);
    values.extend(run.facets());
    for key in ["flaws", "expects_landing"].into_iter().chain(STAGES) {
        values.push(r.sql(key)?);
    }
    values.push(Sql::Text(run.role.clone()));
    for key in &CONTEXT[2..] {
        values.push(r.sql(key)?);
    }
    Ok(values)
}
