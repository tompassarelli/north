//! `north config operator check [DIR] [--only NAME,...]`: every schema-class file of the operator repository read against its typed key table.
//! A schema is one entry of `SCHEMAS`; a finding prints as `FILE: KEY: reason`.
use std::path::{Path, PathBuf};

use toml_edit::{DocumentMut, Item, TableLike, Value};

use crate::error::{NorthError, NorthResult};

pub const USAGE: &str = "usage: north config operator check [DIR] [--only NAME,...]";

#[derive(Clone, Copy)]
enum Ty {
    Text,
    Number,
    Money,
    Day,
    Nested,
    Any,
}

struct Table {
    path: &'static str,
    keys: &'static [(&'static str, Ty)],
    required: &'static [&'static str],
}

type Values = fn(&str) -> Result<(), String>;

struct Schema {
    name: &'static str,
    file: &'static str,
    tables: &'static [Table],
    values: Option<Values>,
}

use Ty::{Any, Day, Money, Nested, Number, Text};

const SCHEMAS: &[Schema] = &[
    Schema {
        name: "costs",
        file: "agents/costs.toml",
        values: None,
        tables: &[
            Table { path: "", keys: &[("service", Nested)], required: &[] },
            Table {
                path: "service",
                keys: &[("id", Text), ("name", Text), ("kind", Text), ("billing", Text), ("usd_per_week", Number), ("usd_per_month_cap", Number), ("usd_setup_once", Number), ("use", Text), ("projects", Any), ("readout", Text), ("status", Text), ("credit", Text), ("credit_source", Text), ("initial", Number), ("expires", Day)],
                required: &["kind", "usd_per_week"],
            },
        ],
    },
    Schema {
        name: "claude-accounts",
        file: "agents/claude-accounts.toml",
        values: None,
        tables: &[
            Table { path: "", keys: &[("account", Nested)], required: &[] },
            Table { path: "account", keys: &[("email", Text), ("letter", Text), ("claude_org", Text), ("plan", Text), ("renews", Day), ("read_on", Day), ("api_org", Nested)], required: &["email", "letter", "plan"] },
            Table { path: "account.api_org", keys: &[("id", Text), ("name", Text), ("linked", Day), ("monthly_credit_usd", Number), ("next_credit", Day), ("grants", Nested)], required: &["id"] },
            Table { path: "account.api_org.grants", keys: &[("usd", Number), ("granted", Day), ("expires", Day), ("kind", Text), ("kind_source", Text)], required: &["usd", "expires", "kind", "kind_source"] },
        ],
    },
    Schema {
        name: "credentials",
        file: "agents/credentials.toml",
        values: Some(credential_values),
        tables: &[
            Table { path: "", keys: &[("credential", Nested), ("browser_session", Nested)], required: &[] },
            Table { path: "credential", keys: &[("source", Text), ("use", Text), ("kind", Text), ("renewal", Text), ("expires", Text), ("reach", Text)], required: &["source", "use", "kind", "renewal"] },
            Table { path: "browser_session", keys: &[("host", Text), ("alternative", Text), ("use", Text), ("renewal", Text)], required: &["host", "use", "renewal"] },
        ],
    },
    Schema {
        name: "infra",
        file: "infra/manifest.toml",
        values: None,
        tables: &[
            Table { path: "", keys: &[("currency_usd_per_eur", Number), ("currency_source", Text), ("machine", Nested), ("service", Nested)], required: &[] },
            Table {
                path: "machine",
                keys: &[("id", Text), ("role", Text), ("provider", Text), ("plan", Text), ("location", Text), ("monthly_usd", Number), ("hourly_eur", Number), ("monthly_cap_eur", Number), ("setup_usd_once", Number), ("cores", Number), ("end_or_renewal", Text), ("runs", Text), ("orchestration_host", Text), ("host_config", Text), ("decision", Text), ("source", Text), ("read_on", Day)],
                required: &["id", "monthly_usd"],
            },
            Table {
                path: "service",
                keys: &[("id", Text), ("role", Text), ("provider", Text), ("plan", Text), ("location", Text), ("monthly_usd", Money), ("setup_usd_once", Number), ("api_credit", Text), ("end_or_renewal", Text), ("runs", Text), ("decision", Text), ("source", Text), ("read_on", Day)],
                required: &["id"],
            },
        ],
    },
    Schema {
        name: "frozen-topics",
        file: "agents/frozen-topics.toml",
        values: None,
        tables: &[
            Table { path: "", keys: &[("topic", Nested)], required: &[] },
            Table { path: "topic", keys: &[("repo", Text), ("name", Text), ("terms", Any), ("since", Text), ("reason", Text)], required: &["repo", "name", "terms", "since", "reason"] },
        ],
    },
    Schema {
        name: "operating-point",
        file: "agents/operating-point.toml",
        values: None,
        tables: &[Table {
            path: "",
            keys: &[("date", Day), ("model", Text), ("variant", Text), ("policy_sha256", Text), ("startup_tokens", Number), ("conformance", Number), ("n", Number), ("evidence", Text), ("anchor_conformance", Number), ("anchor_n", Number), ("anchor_date", Day), ("holdout_from", Day), ("weak_scenarios", Any), ("skill_lines", Number), ("provisional", Any)],
            required: &["date", "model", "variant", "policy_sha256", "startup_tokens", "conformance", "n", "evidence"],
        }],
    },
    Schema {
        name: "watch",
        file: "agents/watch.toml",
        values: None,
        tables: &[
            Table { path: "", keys: &[("source", Nested), ("recheck", Nested)], required: &[] },
            Table { path: "source", keys: &[("id", Text), ("person", Text), ("repo", Text), ("path", Text), ("license", Text), ("last_reviewed", Day), ("last_commit", Text), ("record", Text), ("took", Any), ("cadence", Text)], required: &["id", "repo", "last_reviewed", "last_commit"] },
            Table { path: "recheck", keys: &[("id", Text), ("repo", Text), ("recheck", Day), ("reason", Text)], required: &["id", "recheck"] },
        ],
    },
    Schema {
        name: "rules",
        file: "agents/rules.toml",
        values: None,
        tables: &[
            Table { path: "", keys: &[("rule", Nested), ("candidate", Nested)], required: &[] },
            Table { path: "rule", keys: &[("id", Text), ("section", Text), ("text", Text), ("placement", Text), ("fixes", Text), ("incidents", Any), ("replay", Any), ("provenance", Text)], required: &["id", "section", "text", "placement", "fixes"] },
            Table { path: "candidate", keys: &[("id", Text), ("mode", Text), ("seen", Number), ("source", Text), ("decision", Text), ("layer", Text), ("reason", Text), ("covers", Any)], required: &["id", "mode", "decision"] },
        ],
    },
    Schema {
        name: "private-kinds",
        file: "privacy/private-kinds.toml",
        values: None,
        tables: &[
            Table { path: "", keys: &[("kind", Nested)], required: &[] },
            Table { path: "kind", keys: &[("id", Text), ("description", Text)], required: &["id", "description"] },
        ],
    },
];

const GROUPS: &[(&str, &[&str])] = &[("policy", &["frozen-topics", "operating-point", "watch", "rules", "private-kinds"])];

fn credential_values(source: &str) -> Result<(), String> {
    north_ops::ops::credentials::declared::parse(source).map(drop)
}

fn matches(ty: Ty, item: &Item) -> bool {
    let value = item.as_value();
    match ty {
        Text => value.is_some_and(Value::is_str),
        Number => value.is_some_and(|value| value.is_integer() || value.is_float()),
        Money => value.is_some_and(|value| value.is_integer() || value.is_float() || value.is_str()),
        Day => value.is_some_and(|value| value.is_datetime() || value.is_str()),
        Nested | Any => true,
    }
}

fn nodes(item: &Item) -> Vec<(String, &dyn TableLike)> {
    if let Some(array) = item.as_array_of_tables() {
        return array.iter().enumerate().map(|(index, table)| (format!("[{index}]"), table as &dyn TableLike)).collect();
    }
    if let Some(array) = item.as_value().and_then(Value::as_array) {
        return array.iter().enumerate().filter_map(|(index, value)| Some((format!("[{index}]"), value.as_inline_table()? as &dyn TableLike))).collect();
    }
    item.as_table_like().map(|table| vec![(String::new(), table)]).unwrap_or_default()
}

fn walk(schema: &Schema, path: &str, label: &str, node: &dyn TableLike, found: &mut Vec<String>) {
    let Some(table) = schema.tables.iter().find(|table| table.path == path) else { return };
    let at = |key: &str| if label.is_empty() { key.to_owned() } else { format!("{label}.{key}") };
    for key in table.required.iter().filter(|key| node.get(key).is_none()) {
        found.push(format!("{}: {}: required key is missing", schema.file, at(key)));
    }
    for (key, item) in node.iter() {
        let Some((_, ty)) = table.keys.iter().find(|(name, _)| *name == key) else {
            found.push(format!("{}: {}: unknown key", schema.file, at(key)));
            continue;
        };
        if !matches(*ty, item) {
            found.push(format!("{}: {}: wrong type of value", schema.file, at(key)));
        } else if matches!(ty, Nested) {
            let child = if path.is_empty() { key.to_owned() } else { format!("{path}.{key}") };
            for (index, inner) in nodes(item) {
                walk(schema, &child, &format!("{}{index}", at(key)), inner, found);
            }
        }
    }
}

fn check_file(schema: &Schema, root: &Path) -> Vec<String> {
    let path = root.join(schema.file);
    let source = match std::fs::read_to_string(&path) {
        Ok(source) => source,
        Err(error) => return vec![format!("{}: file: cannot read {}: {error}", schema.file, path.display())],
    };
    let document: DocumentMut = match source.parse() {
        Ok(document) => document,
        Err(error) => return vec![format!("{}: file: {}", schema.file, error.to_string().replace('\n', " "))],
    };
    let mut found = Vec::new();
    walk(schema, "", "", document.as_table(), &mut found);
    if found.is_empty()
        && let Some(values) = schema.values
        && let Err(reason) = values(&source)
    {
        let reason = reason.strip_prefix(&format!("{}: ", schema.file)).unwrap_or(&reason);
        found.push(format!("{}: file: {reason}", schema.file));
    }
    found
}

fn default_root() -> PathBuf {
    crate::platform::orchestration::path().parent().and_then(Path::parent).map(Path::to_path_buf).unwrap_or_default()
}

pub fn findings(root: &Path, only: Option<&[&str]>) -> Vec<String> {
    SCHEMAS.iter().filter(|schema| only.is_none_or(|names| selects(names, schema))).flat_map(|schema| check_file(schema, root)).collect()
}

fn selects(names: &[&str], schema: &Schema) -> bool {
    names.iter().any(|name| *name == schema.name || GROUPS.iter().any(|(group, members)| group == name && members.contains(&schema.name)))
}

pub fn run(arguments: &[String]) -> NorthResult<()> {
    let usage = || NorthError::Usage(USAGE.into());
    let (mut root, mut only) = (None, None::<Vec<&str>>);
    let mut rest = arguments.iter();
    while let Some(argument) = rest.next() {
        match argument.as_str() {
            "--only" => only = Some(rest.next().ok_or_else(usage)?.split(',').collect()),
            flag if flag.starts_with("--") => return Err(usage()),
            _ if root.is_none() => root = Some(PathBuf::from(argument)),
            _ => return Err(usage()),
        }
    }
    if let Some(unknown) = only.iter().flatten().find(|name| !SCHEMAS.iter().any(|schema| schema.name == **name) && !GROUPS.iter().any(|(group, _)| *group == **name)) {
        let names: Vec<&str> = SCHEMAS.iter().map(|schema| schema.name).chain(GROUPS.iter().map(|(group, _)| *group)).collect();
        return Err(NorthError::Usage(format!("{USAGE}; NAME {unknown} is not one of {}", names.join(", "))));
    }
    let root = root.unwrap_or_else(default_root);
    let found = findings(&root, only.as_deref());
    if found.is_empty() {
        return Ok(());
    }
    for line in &found {
        println!("{line}");
    }
    Err(NorthError::Rejected(format!("{} finding(s) in {}", found.len(), root.display())))
}
