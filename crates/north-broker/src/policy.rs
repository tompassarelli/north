use std::collections::BTreeMap;
use std::path::Path;

use toml_edit::{DocumentMut, Item, Table};

pub type Permissions = BTreeMap<String, String>;

const LEVELS: [&str; 3] = ["read", "write", "admin"];

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Ceiling {
    pub repositories: Option<Vec<String>>,
    pub permissions: Option<Permissions>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Caller {
    pub uid: u32,
    pub github: BTreeMap<String, Ceiling>,
    pub proxy: BTreeMap<String, Vec<String>>,
    pub threads: Vec<String>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Policy {
    callers: Vec<Caller>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Scope {
    pub repositories: Vec<String>,
    pub permissions: Permissions,
}

impl Policy {
    pub fn load(path: &Path) -> Result<Self, String> {
        let text = std::fs::read_to_string(path).map_err(|error| format!("cannot read the policy {}: {error}", path.display()))?;
        Self::parse(&text).map_err(|why| format!("policy {}: {why}", path.display()))
    }

    pub fn parse(text: &str) -> Result<Self, String> {
        let document: DocumentMut = text.parse().map_err(|error| format!("not TOML: {error}"))?;
        let mut callers = Vec::new();
        for table in document.get("caller").and_then(Item::as_array_of_tables).into_iter().flatten() {
            callers.push(caller(table)?);
        }
        Ok(Self { callers })
    }

    pub fn for_user(uid: u32) -> Self {
        let lander = Ceiling { repositories: None, permissions: Some(Permissions::from([("contents".to_owned(), "write".to_owned()), ("workflows".to_owned(), "write".to_owned()), ("metadata".to_owned(), "read".to_owned())])) };
        let github = BTreeMap::from([("agent".to_owned(), Ceiling::default()), ("lander".to_owned(), lander)]);
        let proxy = BTreeMap::from([
            ("digitalocean".to_owned(), vec!["GET /v2/customers/my/balance".to_owned()]),
            ("hcloud".to_owned(), ["GET /v1/server_types", "GET /v1/server_types?*", "GET /v1/servers", "GET /v1/servers?*"].map(str::to_owned).to_vec()),
        ]);
        Self { callers: vec![Caller { uid, github, proxy, threads: Vec::new() }] }
    }

    pub fn caller(&self, uid: u32) -> Option<&Caller> {
        self.callers.iter().find(|caller| caller.uid == uid)
    }
}

fn caller(table: &Table) -> Result<Caller, String> {
    let uid = table.get("uid").and_then(Item::as_integer).and_then(|uid| u32::try_from(uid).ok()).ok_or("a [[caller]] has no uid")?;
    let mut github = BTreeMap::new();
    for (app, ceiling) in table.get("github").and_then(Item::as_table).into_iter().flat_map(Table::iter) {
        github.insert(app.to_owned(), ceiling_of(ceiling.as_table().ok_or_else(|| format!("caller {uid}: github.{app} is not a table"))?, uid, app)?);
    }
    let mut proxy = BTreeMap::new();
    for (provider, rules) in table.get("proxy").and_then(Item::as_table).into_iter().flat_map(Table::iter) {
        proxy.insert(provider.to_owned(), strings(rules).ok_or_else(|| format!("caller {uid}: proxy.{provider} is not a list of \"METHOD /path\" rules"))?);
    }
    let threads = match table.get("threads") {
        None => Vec::new(),
        Some(verbs) => strings(verbs).filter(|verbs| verbs.iter().all(|verb| name_ok(verb))).ok_or_else(|| format!("caller {uid}: threads is not a list of verb names"))?,
    };
    Ok(Caller { uid, github, proxy, threads })
}

fn ceiling_of(table: &Table, uid: u32, app: &str) -> Result<Ceiling, String> {
    let repositories = table.get("repositories").map(|list| strings(list).ok_or_else(|| format!("caller {uid}: github.{app}.repositories is not a list of names"))).transpose()?;
    let permissions = match table.get("permissions").map(|levels| levels.as_table_like().ok_or_else(|| format!("caller {uid}: github.{app}.permissions is not a table"))) {
        None => None,
        Some(levels) => {
            let mut granted = Permissions::new();
            for (name, level) in levels?.iter() {
                let level = level.as_str().filter(|level| LEVELS.contains(level)).ok_or_else(|| format!("caller {uid}: github.{app}.permissions.{name} is not read, write or admin"))?;
                granted.insert(name.to_owned(), level.to_owned());
            }
            Some(granted)
        }
    };
    if app == "lander" && permissions.is_none() {
        return Err(format!("caller {uid}: github.lander has no permissions list; the lander App is never granted its whole installation by asking for nothing"));
    }
    Ok(Ceiling { repositories, permissions })
}

fn strings(item: &Item) -> Option<Vec<String>> {
    item.as_array()?.iter().map(|value| value.as_str().map(str::to_owned)).collect()
}

pub fn rank(level: &str) -> usize {
    LEVELS.iter().position(|known| *known == level).unwrap_or(usize::MAX)
}

pub fn name_ok(name: &str) -> bool {
    !name.is_empty() && name.len() <= 100 && name.bytes().all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
}

impl Ceiling {
    pub fn scope(&self, app: &str, repositories: &[String], permissions: &Permissions) -> Result<Scope, String> {
        if let Some(name) = repositories.iter().find(|name| !name_ok(name)) {
            return Err(format!("{name:?} is not a repository name"));
        }
        if let Some(allowed) = &self.repositories {
            if repositories.is_empty() {
                return Err(format!("a {app} token must name its repositories; this caller may receive only {}", allowed.join(", ")));
            }
            if let Some(outside) = repositories.iter().find(|name| !allowed.contains(name)) {
                return Err(format!("repository {outside} is outside this caller's {app} ceiling ({})", allowed.join(", ")));
            }
        }
        Ok(Scope { repositories: repositories.to_vec(), permissions: self.permissions_for(app, permissions)? })
    }

    fn permissions_for(&self, app: &str, requested: &Permissions) -> Result<Permissions, String> {
        let mut granted = Permissions::new();
        for (name, level) in requested {
            if !name_ok(name) || !LEVELS.contains(&level.as_str()) {
                return Err(format!("{name}:{level} is not a permission"));
            }
            match &self.permissions {
                Some(ceiling) => match ceiling.get(name) {
                    None => return Err(format!("permission {name} is outside this caller's {app} ceiling")),
                    Some(most) if rank(level) > rank(most) => return Err(format!("permission {name}:{level} exceeds this caller's {app} ceiling ({name}:{most})")),
                    Some(_) => {}
                },
                None if name == "workflows" => return Err("permission workflows is never granted per call".to_owned()),
                None => {}
            }
            granted.insert(name.clone(), level.clone());
        }
        if granted.is_empty() {
            match &self.permissions {
                Some(ceiling) => granted.clone_from(ceiling),
                None => return Ok(granted),
            }
        }
        if granted.get("metadata").is_some_and(|level| level != "read") {
            return Err("permission metadata is read only".to_owned());
        }
        granted.insert("metadata".to_owned(), "read".to_owned());
        Ok(granted)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Rules(Vec<Rule>);

#[derive(Clone, Debug, Eq, PartialEq)]
struct Rule {
    method: String,
    path: Vec<String>,
    query: Option<String>,
}

impl Rules {
    pub fn new(rules: &[String]) -> Result<Self, String> {
        let parsed = rules.iter().map(|rule| match rule.split_once(' ') {
            Some((method, target)) if target.starts_with('/') && !method.is_empty() => {
                let (path, query) = target.split_once('?').map_or((target, None), |(path, query)| (path, Some(query.to_owned())));
                Ok(Rule { method: method.to_ascii_uppercase(), path: path.split('/').skip(1).map(str::to_owned).collect(), query })
            }
            _ => Err(format!("rule {rule:?} is not \"METHOD /path[?query]\"")),
        });
        parsed.collect::<Result<_, _>>().map(Self)
    }

    pub fn allows(&self, method: &str, path: &str, query: &str) -> bool {
        let Some(segments) = clean(path) else { return false };
        if !query_clean(query) {
            return false;
        }
        self.0.iter().any(|rule| rule.method == method && matches(&rule.path, &segments) && rule.query.as_deref().is_none_or(|allowed| allowed == "*" || allowed == query) && (rule.query.is_some() || query.is_empty()))
    }
}

fn pct_encoded_ok(text: &str) -> bool {
    let bytes = text.as_bytes();
    bytes.iter().enumerate().all(|(index, byte)| *byte != b'%' || (bytes.len() > index + 2 && bytes[index + 1].is_ascii_hexdigit() && bytes[index + 2].is_ascii_hexdigit()))
}

fn pchar(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || b"-._~%!$&'()*+,;=:@".contains(&byte)
}

fn query_clean(query: &str) -> bool {
    query.bytes().all(|byte| pchar(byte) || byte == b'/' || byte == b'?') && pct_encoded_ok(query)
}

fn clean(path: &str) -> Option<Vec<&str>> {
    let rest = path.strip_prefix('/')?;
    let lowered = path.to_ascii_lowercase();
    if !path.bytes().all(|byte| pchar(byte) || byte == b'/') || !pct_encoded_ok(path) || ["%2e", "%2f", "%5c", "%00"].iter().any(|bad| lowered.contains(bad)) {
        return None;
    }
    let segments: Vec<&str> = rest.split('/').collect();
    let trailing = segments.last().is_some_and(|last| last.is_empty());
    let bare = if trailing { &segments[..segments.len() - 1] } else { &segments[..] };
    let dots = |segment: &str| matches!(segment.split(';').next(), Some("." | ".."));
    (!bare.iter().any(|segment| segment.is_empty() || dots(segment))).then(|| bare.to_vec())
}

fn matches(pattern: &[String], segments: &[&str]) -> bool {
    match pattern.split_last() {
        Some((last, head)) if last == "**" => segments.len() > head.len() && head.iter().zip(segments).all(|(want, got)| want == "*" || want == got),
        _ => pattern.len() == segments.len() && pattern.iter().zip(segments).all(|(want, got)| want == "*" || want == got),
    }
}

#[cfg(test)]
#[path = "policy_tests.rs"]
mod tests;
