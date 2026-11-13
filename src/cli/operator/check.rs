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
    Flag,
    List,
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

use Ty::{Any, Day, Flag, List, Money, Nested, Number, Text};

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
    Schema {
        name: "orchestration-accounts",
        file: "agents/orchestration.toml",
        values: None,
        tables: &[
            Table { path: "", keys: &[("accounts", Nested), ("tiers", Nested), ("on_request", Nested), ("mode", Any), ("dispatch_providers", Any), ("dispatch_hosts_excluded", Any), ("hosts", Any), ("dispatch", Any), ("placement", Any), ("projects", Any)], required: &[] },
            Table { path: "accounts", keys: &[("codex", Nested), ("claude", Nested)], required: &[] },
            Table { path: "accounts.codex", keys: &[("profiles", Any), ("target_left", Number), ("band", Any), ("thursday", Any), ("source", Text)], required: &[] },
            Table { path: "accounts.claude", keys: &[("labels", Any), ("plans", Any), ("target_left", Number), ("reserve", Any), ("api_hold", Any), ("api_beside_plans", Any), ("source", Text), ("profiles", Nested), ("api_keys", Nested), ("api_orgs", Nested)], required: &[] },
            Table { path: "accounts.claude.profiles", keys: &[("*", Any)], required: &[] },
            Table { path: "accounts.claude.api_keys", keys: &[("*", Any)], required: &[] },
            Table { path: "accounts.claude.api_orgs", keys: &[("*", Nested)], required: &[] },
            Table { path: "accounts.claude.api_orgs.*", keys: &[("id", Text), ("emails", Any)], required: &["id"] },
            Table { path: "tiers", keys: &[("*", Nested)], required: &[] },
            Table { path: "tiers.*", keys: &[("model", Text), ("effort", Text), ("agent", Text), ("role", Text), ("start", Number), ("detail", Text)], required: &["model", "effort"] },
            Table { path: "on_request", keys: &[("*", Nested)], required: &[] },
            Table { path: "on_request.*", keys: &[("model", Text), ("effort", Text), ("detail", Text)], required: &["model", "effort"] },
        ],
    },
    Schema {
        name: "orchestration",
        file: "agents/orchestration.toml",
        values: None,
        tables: &[
            Table { path: "", keys: &[("mode", Text), ("dispatch_providers", List), ("dispatch_hosts_excluded", List), ("hosts", Nested), ("dispatch", Nested), ("placement", Nested), ("projects", Nested), ("accounts", Any), ("tiers", Any), ("on_request", Any)], required: &[] },
            Table { path: "hosts", keys: &[("*", Nested)], required: &[] },
            Table { path: "hosts.*", keys: &[("resident_cap", Number), ("deploy_ssh", Text), ("builds_runtime", Flag), ("publishes_host_facts", Flag), ("fleet_reports", Flag), ("pulls_spool", Flag), ("warcraft_clients", Number), ("lease_builds", Flag), ("offload_builds", Flag), ("runs_no_checks", Flag), ("runtime_source", Flag), ("ssh", Text), ("agent_ssh", Text), ("launches", Flag), ("serves", List), ("disk_free_gib", Number), ("buck2_lint", Flag), ("buck2_red_reverts", Flag), ("buck2_test_crates", List), ("buck2_caseless_crates", List), ("pushes", List), ("reaches", List), ("settles_fleet", Flag), ("buck2_host_cases", Flag), ("buck2_tree_build", Flag), ("buck2_after_run", Flag), ("disk_budget_gib", Nested)], required: &[] },
            Table { path: "hosts.*.disk_budget_gib", keys: &[("*", Number)], required: &[] },
            Table { path: "dispatch", keys: &[("triage_lead", Text)], required: &[] },
            Table { path: "placement", keys: &[("order", List), ("cloud_credit", Text), ("cloud_host", Text), ("dispatch_host", Text), ("codex_cloud", Flag), ("claude_session_target", Number), ("claude_session_ceiling", Number), ("private_repos", List)], required: &[] },
            Table { path: "projects", keys: &[("*", Nested)], required: &[] },
            Table { path: "projects.*", keys: &[("priority", Number), ("paused", Flag), ("repos", List), ("dispatch_repos", List), ("target", Day), ("goal", List), ("drain", Any), ("status_command", List)], required: &["priority", "repos"] },
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
        Flag => value.is_some_and(Value::is_bool),
        List => value.is_some_and(Value::is_array),
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
        let Some((name, ty)) = table.keys.iter().find(|(name, _)| *name == key).or_else(|| table.keys.iter().find(|(name, _)| *name == "*")) else {
            found.push(format!("{}: {}: unknown key", schema.file, at(key)));
            continue;
        };
        if !matches(*ty, item) {
            found.push(format!("{}: {}: wrong type of value", schema.file, at(key)));
        } else if matches!(ty, Nested) {
            let child = if path.is_empty() { (*name).to_owned() } else { format!("{path}.{name}") };
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
