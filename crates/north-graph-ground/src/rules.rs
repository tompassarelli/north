//! The Datalog queries behind the ready set, written in a compact notation: `?name` is a variable,
//! `:name` a keyword, `#N` an integer, and any other word a string or `false`.
use serde_json::{Value, json};

fn arg(text: &str) -> Value {
    match (text.as_bytes().first(), text) {
        (Some(b'?'), _) => json!({"var": &text[1..]}),
        (Some(b':'), _) => json!(["keyword", &text[1..]]),
        (Some(b'#'), _) => text[1..].parse::<i64>().map_or_else(|_| json!(text), |number| json!(number)),
        (_, "false") => json!(false),
        _ => json!(text),
    }
}

fn atom(relation: &str, args: &[&str]) -> Value {
    json!({"rel": relation, "args": args.iter().map(|text| arg(text)).collect::<Vec<_>>()})
}

fn absent(relation: &str, args: &[&str]) -> Value {
    json!({"rel": relation, "args": args.iter().map(|text| arg(text)).collect::<Vec<_>>(), "neg": true})
}

fn fact(subject: &str, predicate: &str, object: &str) -> Value {
    atom("triple", &[subject, &format!(":{predicate}"), object])
}

fn rule(head: (&str, &[&str]), body: Vec<Value>) -> Value {
    json!({"head": {"rel": head.0, "args": head.1.iter().map(|text| arg(text)).collect::<Vec<_>>()}, "body": body})
}

fn first_stratum() -> Vec<Value> {
    let mut rules = vec![
        rule(("live", &["?issue", "?box", "?text"]), vec![fact("?issue", "state", ":open"), fact("?issue", "has-box", "?box"), fact("?box", "ticked", "false"), fact("?box", "text", "?text")]),
        rule(("pending", &["?target", "?text"]), vec![fact("?target", "ticked", "false"), fact("?target", "text", "?text")]),
        rule(("pending", &["?target", "?text"]), vec![fact("?target", "state", ":open"), fact("?target", "title", "?text")]),
        rule(("ranked", &["?issue"]), vec![fact("?issue", "rank", "?rank")]),
    ];
    for predicate in ["waits-on", "doc-waits-on"] {
        rules.push(rule(("edge", &["?box", "?target"]), vec![fact("?box", predicate, "?target")]));
    }
    for predicate in ["blocked-by", "follows", "doc-waits-on"] {
        rules.push(rule(("edge", &["?box", "?target"]), vec![fact("?other", "has-box", "?box"), fact("?other", predicate, "?target")]));
    }
    rules
}

fn second_stratum() -> Vec<Value> {
    vec![
        rule(("held", &["?box", "?target", "?why"]), vec![atom("edge", &["?box", "?target"]), atom("pending", &["?target", "?why"])]),
        rule(("blocked", &["?box"]), vec![atom("held", &["?box", "?target", "?why"])]),
    ]
}

fn ready_rules() -> Vec<Value> {
    let unblocked = |extra: Value| vec![atom("live", &["?issue", "?box", "?text"]), absent("blocked", &["?box"]), extra];
    vec![
        rule(("ready", &["?rank", "?issue", "?box", "?text"]), unblocked(fact("?issue", "rank", "?rank"))),
        rule(("ready", &["#1000000", "?issue", "?box", "?text"]), unblocked(absent("ranked", &["?issue"]))),
    ]
}

fn held_report_rules() -> Vec<Value> {
    vec![rule(("held_by", &["?issue", "?box", "?text", "?target", "?why"]), vec![atom("live", &["?issue", "?box", "?text"]), atom("held", &["?box", "?target", "?why"])])]
}

fn query(find: &str, last: Vec<Value>, order: &[(usize, &str)]) -> Value {
    let order: Vec<Value> = order.iter().map(|(column, direction)| json!({"column": column, "direction": direction})).collect();
    json!({"find": find, "strata": [first_stratum(), second_stratum(), last], "orderBy": order, "limit": 100_000})
}

pub fn ready_query() -> Value {
    query("ready", ready_rules(), &[(0, "asc"), (1, "asc"), (3, "asc")])
}

pub fn held_query() -> Value {
    query("held_by", held_report_rules(), &[(0, "asc"), (2, "asc")])
}

pub fn rank_query() -> Value {
    json!({"find": "rank", "rules": [rule(("rank", &["?issue", "?rank"]), vec![fact("?issue", "rank", "?rank")])]})
}
