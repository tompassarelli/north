use Ty::{Any, Day, Flag, List, Money, Nested, Number, Text};

#[derive(Clone, Copy)]
pub(super) enum Ty {
    Text,
    Number,
    Money,
    Day,
    Flag,
    List,
    Nested,
    Any,
}

pub(super) struct Table {
    pub(super) path: &'static str,
    pub(super) keys: &'static [(&'static str, Ty)],
    pub(super) required: &'static [&'static str],
}

pub(super) type Values = fn(&str) -> Result<(), String>;

pub(super) struct Schema {
    pub(super) name: &'static str,
    pub(super) file: &'static str,
    pub(super) tables: &'static [Table],
    pub(super) values: Option<Values>,
}

pub(super) fn credential_values(source: &str) -> Result<(), String> {
    north_ops::ops::credentials::declared::parse(source).map(drop)
}

pub(super) const SCHEMAS: &[Schema] = &[
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
            Table { path: "hosts.*", keys: &[("resident_cap", Number), ("deploy_ssh", Text), ("builds_runtime", Flag), ("publishes_host_facts", Flag), ("fleet_reports", Flag), ("pulls_spool", Flag), ("warcraft_clients", Number), ("lease_builds", Flag), ("session_builds", Flag), ("offload_builds", Flag), ("runs_no_checks", Flag), ("disk_keep", Flag), ("build_ahead", Flag), ("runtime_source", Flag), ("ssh", Text), ("agent_ssh", Text), ("launches", Flag), ("serves", List), ("disk_free_gib", Number), ("buck2_lint", Flag), ("buck2_red_reverts", Flag), ("buck2_test_crates", List), ("buck2_caseless_crates", List), ("pushes", List), ("reaches", List), ("settles_fleet", Flag), ("buck2_host_cases", Flag), ("buck2_tree_build", Flag), ("buck2_after_run", Flag), ("disk_budget_gib", Nested)], required: &[] },
            Table { path: "hosts.*.disk_budget_gib", keys: &[("*", Number)], required: &[] },
            Table { path: "dispatch", keys: &[("triage_lead", Text)], required: &[] },
            Table { path: "placement", keys: &[("order", List), ("cloud_credit", Text), ("cloud_host", Text), ("dispatch_host", Text), ("codex_cloud", Flag), ("claude_session_target", Number), ("claude_session_ceiling", Number), ("private_repos", List)], required: &[] },
            Table { path: "projects", keys: &[("*", Nested)], required: &[] },
            Table { path: "projects.*", keys: &[("priority", Number), ("paused", Flag), ("repos", List), ("dispatch_repos", List), ("target", Day), ("goal", List), ("drain", Any), ("status_command", List)], required: &["priority", "repos"] },
        ],
    },
];
