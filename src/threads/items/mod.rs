//! `north threads release`, `need`, `unneed`, `new`, `note`, `close`, `reopen` and `block`: who
//! holds an issue, what blocks it, and the native items that never reach GitHub.
#[cfg(test)]
mod tests;

use std::io::{Read, Write};

use rusqlite::{OptionalExtension, params};

use super::verb::{Args, Context, Failure, Outcome, event, immediate, item_ref, open, report};
use crate::platform::pyjson::Value;
use crate::threads::catalog::CATALOG;

pub(crate) const VERBS: [&str; 8] = ["release", "need", "unneed", "new", "note", "close", "reopen", "block"];

/// Runs one verb, printing its result to `out` and a refusal to `err`; returns the exit code.
pub(crate) fn run(argv: &[String], context: &Context, stdin: &mut dyn Read, out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    let rest = &argv[1..];
    let result = match argv[0].as_str() {
        "release" => release(rest, context),
        "need" => dependency(rest, context, true),
        "unneed" => dependency(rest, context, false),
        "new" => new(rest, context, stdin),
        "note" => note(rest, context),
        "close" => set_state(rest, context, "closed"),
        "reopen" => set_state(rest, context, "open"),
        _ => block(rest, context),
    };
    report(&argv[0], result, out, err)
}

fn usage(text: &str) -> Failure {
    Failure::Usage(format!("usage: threads {text}"))
}

fn item(text: &str, context: &Context) -> Result<String, Failure> {
    item_ref(text, context).map(|(repo, number)| format!("{repo}#{number}"))
}

fn release(argv: &[String], context: &Context) -> Outcome {
    let args = Args::parse(argv, &["--to"], &[])?;
    let [id] = args.positional.as_slice() else { return Err(usage("release REPO#N [--to NAME]")) };
    let item = item(id, context)?;
    let by = context.by(&args);
    let mut db = open(context.database, true)?;
    let tx = immediate(&mut db)?;
    let owner: Option<String> = tx.query_row("SELECT owner FROM claims WHERE item = ?1", [&item], |row| row.get(0)).optional()?;
    let Some(owner) = owner else { return Err(Failure::Refused(format!("{item} has no owner"))) };
    let printed = match args.get("--to").filter(|to| !to.is_empty()) {
        Some(to) => {
            tx.execute("UPDATE claims SET owner = ?1, since = ?2 WHERE item = ?3", params![to, context.now.iso(), item])?;
            event(&tx, Some(&item), "handoff", &by, Value::object([("from", owner.as_str().into()), ("to", to.into())]), context.now)?;
            format!("{item} handed from {owner} to {to}\n")
        }
        None => {
            tx.execute("DELETE FROM claims WHERE item = ?1", [&item])?;
            event(&tx, Some(&item), "release", &by, Value::object([("owner", owner.as_str().into())]), context.now)?;
            format!("{item} released by {owner}\n")
        }
    };
    tx.commit()?;
    Ok(printed)
}

/// `need` records that ID is blocked by OTHER, `unneed` removes it: in the ledger when either is a
/// native item, else on GitHub.
fn dependency(argv: &[String], context: &Context, need: bool) -> Outcome {
    let verb = if need { "need" } else { "unneed" };
    let args = Args::parse(argv, &[], &[])?;
    let [id, other] = args.positional.as_slice() else { return Err(usage(&format!("{verb} REPO#N BLOCKER#N"))) };
    let ((repo, number), (blocker_repo, blocker)) = (item_ref(id, context)?, item_ref(other, context)?);
    let (item, needs) = (format!("{repo}#{number}"), format!("{blocker_repo}#{blocker}"));
    let by = context.by(&args);
    let local = CATALOG.is_native(&repo) || CATALOG.is_native(&blocker_repo);
    let mut db = open(context.database, true)?;
    if local {
        let tx = immediate(&mut db)?;
        if need {
            tx.execute("INSERT OR IGNORE INTO item_needs(item, needs) VALUES (?1, ?2)", params![item, needs])?;
        } else {
            tx.execute("DELETE FROM item_needs WHERE item = ?1 AND needs = ?2", params![item, needs])?;
        }
        event(&tx, Some(&item), verb, &by, Value::object([("needs", needs.as_str().into())]), context.now)?;
        tx.commit()?;
    } else {
        let owner = &CATALOG.owner;
        let blocker_id = context.gh(&["api", &format!("repos/{owner}/{blocker_repo}/issues/{blocker}"), "--jq", ".id"])?.trim().to_owned();
        let path = format!("repos/{owner}/{repo}/issues/{number}/dependencies/blocked_by");
        if need {
            context.gh(&["api", "-X", "POST", &path, "-F", &format!("issue_id={blocker_id}")])?;
        } else {
            context.gh(&["api", "-X", "DELETE", &format!("{path}/{blocker_id}")])?;
        }
        db.execute("DELETE FROM gh_cache WHERE repo = ?1", [&repo])?;
        let tx = immediate(&mut db)?;
        event(&tx, Some(&item), verb, &by, Value::object([("needs", needs.as_str().into())]), context.now)?;
        tx.commit()?;
    }
    Ok(match (need, local) {
        (true, true) => format!("{id} is blocked by {other} locally\n"),
        (true, false) => format!("{id} is blocked by {other} on GitHub\n"),
        (false, _) => format!("{id} is no longer blocked by {other}\n"),
    })
}

fn native_namespaces() -> String {
    CATALOG.native.join(", ")
}

/// A native item that exists, as namespace and number.
fn native(text: &str, context: &Context, db: &rusqlite::Connection) -> Result<(String, u64), Failure> {
    let (ns, number) = item_ref(text, context)?;
    if !CATALOG.is_native(&ns) {
        return Err(Failure::Refused(format!("{text} is a GitHub issue; only {} items are native", native_namespaces())));
    }
    let found: Option<i64> = db.query_row("SELECT 1 FROM items WHERE ns = ?1 AND number = ?2", params![ns, number as i64], |row| row.get(0)).optional()?;
    if found.is_none() {
        return Err(Failure::Refused(format!("no item {ns}#{number}")));
    }
    Ok((ns, number))
}

fn new(argv: &[String], context: &Context, stdin: &mut dyn Read) -> Outcome {
    let args = Args::parse(argv, &["--due", "--body"], &[])?;
    let [ns, title] = args.positional.as_slice() else { return Err(usage("new NS TITLE [--due DATE] [--body TEXT|-]")) };
    if !CATALOG.is_native(ns) {
        return Err(Failure::Refused(format!("{ns} is not a native namespace ({})", native_namespaces())));
    }
    let body = match args.get("--body") {
        Some("-") => {
            let mut text = String::new();
            stdin.read_to_string(&mut text).map_err(|error| Failure::Refused(error.to_string()))?;
            text.trim().to_owned()
        }
        body => body.unwrap_or("").to_owned(),
    };
    context.outside.scan(&format!("{title}\n{body}"))?;
    let mut db = open(context.database, true)?;
    let tx = immediate(&mut db)?;
    let number: i64 = tx.query_row("SELECT coalesce(max(number), 0) + 1 FROM items WHERE ns = ?1", [ns], |row| row.get(0))?;
    tx.execute("INSERT INTO items(ns, number, title, body, created, due) VALUES (?1, ?2, ?3, ?4, ?5, ?6)", params![ns, number, title, body, context.now.iso(), args.get("--due")])?;
    let item = format!("{ns}#{number}");
    event(&tx, Some(&item), "capture", &context.by(&args), Value::Object(Vec::new()), context.now)?;
    tx.commit()?;
    Ok(format!("{item}\n"))
}

fn note(argv: &[String], context: &Context) -> Outcome {
    let args = Args::parse(argv, &[], &[])?;
    let Some((id, words)) = args.positional.split_first().filter(|(_, words)| !words.is_empty()) else { return Err(usage("note ID TEXT...")) };
    let mut db = open(context.database, true)?;
    let (ns, number) = native(id, context, &db)?;
    let text = words.join(" ");
    context.outside.scan(&text)?;
    let item = format!("{ns}#{number}");
    let tx = immediate(&mut db)?;
    event(&tx, Some(&item), "note", &context.by(&args), Value::object([("text", text.into())]), context.now)?;
    tx.commit()?;
    Ok(format!("noted on {item}\n"))
}

fn set_state(argv: &[String], context: &Context, state: &str) -> Outcome {
    let args = Args::parse(argv, &[], &[])?;
    let [id] = args.positional.as_slice() else { return Err(usage(&format!("{} ID", if state == "closed" { "close" } else { "reopen" }))) };
    let mut db = open(context.database, true)?;
    let (ns, number) = native(id, context, &db)?;
    let item = format!("{ns}#{number}");
    let closed = (state == "closed").then(|| context.now.iso());
    let tx = immediate(&mut db)?;
    tx.execute("UPDATE items SET state = ?1, closed = ?2 WHERE ns = ?3 AND number = ?4", params![state, closed, ns, number as i64])?;
    event(&tx, Some(&item), if state == "closed" { "close" } else { "reopen" }, &context.by(&args), Value::Object(Vec::new()), context.now)?;
    tx.commit()?;
    Ok(format!("{item} {state}\n"))
}

fn block(argv: &[String], context: &Context) -> Outcome {
    let args = Args::parse(argv, &[], &["--clear"])?;
    let Some((id, words)) = args.positional.split_first() else { return Err(usage("block REPO#N REASON... | block REPO#N --clear")) };
    let item = item(id, context)?;
    let reason = words.join(" ").trim().to_owned();
    let by = context.by(&args);
    let mut db = open(context.database, true)?;
    let tx = immediate(&mut db)?;
    let printed = if args.has("--clear") {
        tx.execute("DELETE FROM blocks WHERE item = ?1", [&item])?;
        event(&tx, Some(&item), "note", &by, Value::object([("unblock", Value::Bool(true))]), context.now)?;
        format!("{item} unblocked\n")
    } else {
        if reason.is_empty() {
            return Err(Failure::Refused("block needs a reason".into()));
        }
        tx.execute("INSERT OR REPLACE INTO blocks(item, reason, since, by) VALUES (?1, ?2, ?3, ?4)", params![item, reason, context.now.iso(), by])?;
        event(&tx, Some(&item), "note", &by, Value::object([("block", reason.as_str().into())]), context.now)?;
        format!("{item} blocked: {reason}\n")
    };
    tx.commit()?;
    Ok(printed)
}
