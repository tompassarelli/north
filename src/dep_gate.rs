use regex::Regex;
use serde_json::{Map, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use crate::platform::git as platform_git;
use std::process::Command;
use std::time::Duration;

use crate::foundation::clock::Timestamp;

const TRAILER_FORMAT: &str = "Dependency: <name> — <reason> (ASCII - also accepted)";
const URGENT_FORMAT: &str = "Dependency-Urgent: <name>@<version> <advisory-id>; flakes use node@locked.rev (otherwise locked.version or locked.narHash)";
const COOLDOWN: f64 = 604800.0;
type Parsed<T> = Result<T, &'static str>;
type Identity = (String, String, String);
type Urgent = BTreeSet<(String, String)>;

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
struct Dependency {
    name: String,
    source: String,
    allowed: bool,
}

#[derive(Debug)]
struct LockCandidate {
    path: String,
    ecosystem: String,
    name: String,
    version: String,
    published: Option<f64>,
}

impl LockCandidate {
    fn identity(&self) -> Identity {
        (
            self.ecosystem.clone(),
            self.name.clone(),
            self.version.clone(),
        )
    }
}

fn matches(pattern: &str, text: &str) -> bool {
    Regex::new(pattern)
        .expect("valid dependency expression")
        .is_match(text)
}

fn table(value: &Value) -> Parsed<&Map<String, Value>> {
    value.as_object().ok_or("ValueError")
}

fn string(value: &Value) -> Parsed<&str> {
    let text = value.as_str().ok_or("ValueError")?;
    if text.trim().is_empty() || text.chars().any(|c| c < ' ') {
        return Err("ValueError");
    }
    Ok(text.trim())
}

fn get<'a>(value: &'a Value, key: &str) -> &'a Value {
    value.get(key).unwrap_or(&Value::Null)
}

fn optional_table<'a>(value: &'a Value, key: &str) -> Parsed<Option<&'a Map<String, Value>>> {
    value.get(key).map(table).transpose()
}

fn required<'a>(value: &'a Value, key: &str) -> Parsed<&'a Value> {
    value.get(key).ok_or("KeyError")
}

fn unquote(text: &str) -> String {
    percent_encoding::percent_decode_str(text)
        .decode_utf8_lossy()
        .into_owned()
}

fn normalize(path: &str) -> String {
    let absolute = path.starts_with('/');
    let mut parts = Vec::new();
    for part in path.split('/') {
        match part {
            "" | "." => (),
            ".." if parts.last().is_some_and(|p| *p != "..") => {
                parts.pop();
            }
            ".." if !absolute => parts.push(part),
            ".." => (),
            _ => parts.push(part),
        }
    }
    format!("{}{}", if absolute { "/" } else { "" }, parts.join("/"))
}

fn internal_path(path: &str, manifest: &str) -> bool {
    let path = unquote(path);
    if path.starts_with(['/', '~']) || path.contains('\\') {
        return false;
    }
    let directory = manifest
        .rsplit_once('/')
        .map_or("", |(directory, _)| directory);
    let resolved = normalize(&if directory.is_empty() {
        path
    } else {
        format!("{directory}/{path}")
    });
    resolved != ".." && !resolved.starts_with("../")
}

fn owned_github(source: &str) -> Parsed<bool> {
    let source = source.strip_prefix("git+").unwrap_or(source);
    if matches(r"\A(?:github:)?tompassarelli/[^/#\s]+(?:#\S+)?\z", source) {
        return Ok(true);
    }
    let source = if let Some(path) = source.strip_prefix("git@github.com:") {
        format!("ssh://git@github.com/{path}")
    } else {
        source.to_owned()
    };
    let Some((scheme, rest)) = source.split_once("://") else {
        return Ok(false);
    };
    let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let authority = rest[..end].rsplit('@').next().unwrap_or("");
    let (host, port) = authority
        .split_once(':')
        .map_or((authority, None), |(h, p)| (h, Some(p)));
    let port = match port {
        Some("") | None => None,
        Some(p) => Some(p.parse::<u16>().map_err(|_| "ValueError")?),
    };
    let path = rest[end..].split(['?', '#']).next().unwrap_or("");
    let decoded = unquote(path);
    let parts: Vec<_> = decoded.trim_matches('/').split('/').collect();
    Ok(
        ["https", "http", "ssh", "git"].contains(&scheme.to_ascii_lowercase().as_str())
            && host.eq_ignore_ascii_case("github.com")
            && port.is_none_or(|p| [22, 80, 443].contains(&p))
            && parts.len() >= 2
            && parts[0] == "tompassarelli"
            && !parts[1].is_empty()
            && parts.iter().all(|p| *p != "." && *p != ".."),
    )
}

fn source_dependency(
    name: &Value,
    source: &Value,
    manifest: &str,
    kind: &str,
) -> Parsed<Dependency> {
    let name = string(name)?.to_owned();
    let source = string(source)?;
    let (identity, allowed) = if kind == "path" {
        (format!("path:{source}"), internal_path(source, manifest))
    } else if let Some(value) = source.strip_prefix("workspace:") {
        let allowed = if value.starts_with(['.', '/', '~']) {
            internal_path(value, manifest)
        } else {
            matches(
                r"\A(?:\*|[~^]|[~^]?\d[^\s]*|@?[^\s:/]+(?:/[^\s:/]+)?@(?:\*|[~^]|[~^]?\d[^\s]*))\z",
                value,
            )
        };
        if !allowed {
            return Err("ValueError");
        }
        (source.to_owned(), true)
    } else if let Some(path) = source.strip_prefix("file:") {
        (
            source.to_owned(),
            internal_path(path, manifest)
                && normalize(&unquote(path)).starts_with("vendor/")
                && !path.starts_with('/'),
        )
    } else if owned_github(source)? {
        (source.to_owned(), true)
    } else if kind == "url"
        || source.contains([':', '/', '\\'])
        || source.starts_with(['.', '~'])
        || [".tgz", ".tar.gz", ".tar", ".zip"]
            .iter()
            .any(|s| source.ends_with(s))
    {
        if source.starts_with("npm:") && matches(r"\Anpm:(?:@[^/\s]+/)?[^@\s]+@[^\s:/]+\z", source)
        {
            ("registry".to_owned(), true)
        } else {
            (source.to_owned(), false)
        }
    } else {
        if !matches(r"\A[A-Za-z0-9*^~<>=|+!., -]+\z", source) {
            return Err("ValueError");
        }
        ("registry".to_owned(), true)
    };
    Ok(Dependency {
        name,
        source: identity,
        allowed,
    })
}

fn python_json(value: &Value) -> String {
    match value {
        Value::Object(map) => format!(
            "{{{}}}",
            map.iter()
                .map(|(k, v)| format!(
                    "{}: {}",
                    python_json(&Value::String(k.clone())),
                    python_json(v)
                ))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        Value::Array(values) => format!(
            "[{}]",
            values
                .iter()
                .map(python_json)
                .collect::<Vec<_>>()
                .join(", ")
        ),
        Value::String(text) => {
            let encoded = serde_json::to_string(text).expect("JSON string");
            let mut output = String::new();
            for c in encoded.chars() {
                if c.is_ascii() {
                    output.push(c);
                } else {
                    for unit in c.encode_utf16(&mut [0; 2]) {
                        output.push_str(&format!("\\u{unit:04x}"));
                    }
                }
            }
            output
        }
        _ => value.to_string(),
    }
}

fn declared_dependency(
    name: &str,
    value: &Value,
    manifest: &str,
    ecosystem: &str,
) -> Parsed<Dependency> {
    if value.is_string() {
        return source_dependency(&name.into(), value, manifest, "npm");
    }
    if ecosystem == "npm" {
        return Err("ValueError");
    }
    let values = table(value)?;
    let name = string(
        values
            .get("package")
            .unwrap_or(&Value::String(name.to_owned())),
    )?
    .to_owned();
    let sources: Vec<_> = ["git", "url", "path", "file", "workspace"]
        .into_iter()
        .filter(|k| values.contains_key(*k))
        .collect();
    if sources.len() > 1 {
        return Err("ValueError");
    }
    if let Some(key) = sources.first() {
        if *key == "workspace" {
            if values[*key] != true {
                return Err("ValueError");
            }
            return Ok(Dependency {
                name,
                source: "workspace".into(),
                allowed: true,
            });
        }
        let dep = source_dependency(
            &name.into(),
            &values[*key],
            manifest,
            if *key == "path" { "path" } else { "url" },
        )?;
        let identity = [
            "git",
            "url",
            "path",
            "file",
            "rev",
            "tag",
            "branch",
            "subdirectory",
        ]
        .into_iter()
        .filter_map(|k| values.get(k).map(|v| (k.to_owned(), v.clone())))
        .collect::<Map<_, _>>();
        return Ok(Dependency {
            source: python_json(&Value::Object(identity)),
            ..dep
        });
    }
    if let Some(version) = values.get("version") {
        string(version)?;
        return Ok(Dependency {
            name,
            source: "registry".into(),
            allowed: true,
        });
    }
    Err("ValueError")
}

fn normalized_name(name: &str) -> String {
    Regex::new("[-_.]+")
        .unwrap()
        .replace_all(name, "-")
        .to_lowercase()
}

fn requirement(value: &Value, manifest: &str) -> Parsed<Dependency> {
    let value = string(value)?;
    let re = Regex::new(
        r"\A([A-Za-z0-9](?:[A-Za-z0-9._-]*[A-Za-z0-9])?)(?:\[[A-Za-z0-9_,. -]+\])?\s*(.*)\z",
    )
    .unwrap();
    let captures = re.captures(value).ok_or("ValueError")?;
    let name = normalized_name(&captures[1]);
    let tail = captures[2].split(';').next().unwrap_or("").trim();
    if let Some(source) = tail.strip_prefix('@') {
        return source_dependency(&name.into(), &source.trim().into(), manifest, "url");
    }
    if !tail.is_empty()
        && !matches(
            r"\A\(?\s*(?:===|==|!=|~=|<=|>=|<|>)\s*[A-Za-z0-9*+!._-]+(?:\s*,\s*(?:===|==|!=|~=|<=|>=|<|>)\s*[A-Za-z0-9*+!._-]+)*\s*\)?\z",
            tail,
        )
    {
        return Err("ValueError");
    }
    Ok(Dependency {
        name,
        source: "registry".into(),
        allowed: true,
    })
}

fn requirement_list(
    values: Option<&Value>,
    manifest: &str,
    result: &mut BTreeSet<Dependency>,
) -> Parsed<()> {
    let Some(values) = values else {
        return Ok(());
    };
    for value in values.as_array().ok_or("ValueError")? {
        if value
            .as_object()
            .is_some_and(|m| m.len() == 1 && m.contains_key("include-group"))
        {
            string(&value["include-group"])?;
        } else {
            result.insert(requirement(value, manifest)?);
        }
    }
    Ok(())
}

fn parse_toml(text: &str) -> Parsed<Value> {
    crate::platform::toml_json::parse(text).map_err(|_| "TOMLDecodeError")
}

fn parse_json(text: &str) -> Parsed<Value> {
    serde_json::from_str(text).map_err(|_| "JSONDecodeError")
}

fn dependencies(manifest: &str, text: &str) -> Parsed<BTreeSet<Dependency>> {
    let data = if manifest.ends_with("package.json") {
        parse_json(text)?
    } else {
        parse_toml(text)?
    };
    table(&data)?;
    let mut result = BTreeSet::new();
    if manifest.ends_with("package.json") {
        for section in [
            "dependencies",
            "devDependencies",
            "peerDependencies",
            "optionalDependencies",
        ] {
            for (name, value) in optional_table(&data, section)?.into_iter().flatten() {
                result.insert(declared_dependency(name, value, manifest, "npm")?);
            }
        }
    } else if manifest.ends_with("Cargo.toml") {
        fn walk(
            node: &Map<String, Value>,
            manifest: &str,
            result: &mut BTreeSet<Dependency>,
        ) -> Parsed<()> {
            for (key, value) in node {
                if key.ends_with("dependencies") {
                    for (name, spec) in table(value)? {
                        result.insert(declared_dependency(name, spec, manifest, "cargo")?);
                    }
                } else if let Some(child) = value.as_object() {
                    walk(child, manifest, result)?;
                }
            }
            Ok(())
        }
        walk(table(&data)?, manifest, &mut result)?;
    } else {
        for section in ["project", "build-system", "tool"] {
            optional_table(&data, section)?;
        }
        let project = get(&data, "project");
        requirement_list(project.get("dependencies"), manifest, &mut result)?;
        for values in optional_table(project, "optional-dependencies")?
            .into_iter()
            .flatten()
            .map(|(_, v)| v)
        {
            requirement_list(Some(values), manifest, &mut result)?;
        }
        requirement_list(
            get(&data, "build-system").get("requires"),
            manifest,
            &mut result,
        )?;
        for (_, values) in optional_table(&data, "dependency-groups")?
            .into_iter()
            .flatten()
        {
            requirement_list(Some(values), manifest, &mut result)?;
        }
        let tools = get(&data, "tool");
        optional_table(tools, "poetry")?;
        let poetry = get(tools, "poetry");
        let mut sections = vec![poetry.get("dependencies"), poetry.get("dev-dependencies")];
        for (_, group) in optional_table(poetry, "group")?.into_iter().flatten() {
            table(group)?;
            sections.push(group.get("dependencies"));
        }
        for section in sections.into_iter().flatten() {
            for (name, value) in table(section)? {
                if name.to_lowercase() != "python" {
                    for variant in value
                        .as_array()
                        .map_or_else(|| vec![value], |a| a.iter().collect())
                    {
                        result.insert(declared_dependency(
                            &normalized_name(name),
                            variant,
                            manifest,
                            "python",
                        )?);
                    }
                }
            }
        }
        for tool in ["pdm", "uv", "rye", "hatch"] {
            optional_table(tools, tool)?;
        }
        for (_, values) in optional_table(get(tools, "pdm"), "dev-dependencies")?
            .into_iter()
            .flatten()
        {
            requirement_list(Some(values), manifest, &mut result)?;
        }
        for tool in ["uv", "rye"] {
            requirement_list(
                get(tools, tool).get("dev-dependencies"),
                manifest,
                &mut result,
            )?;
        }
        for (_, env) in optional_table(get(tools, "hatch"), "envs")?
            .into_iter()
            .flatten()
        {
            table(env)?;
            requirement_list(env.get("dependencies"), manifest, &mut result)?;
        }
        for (name, value) in optional_table(get(tools, "uv"), "sources")?
            .into_iter()
            .flatten()
        {
            for spec in value
                .as_array()
                .map_or_else(|| vec![value], |a| a.iter().collect())
            {
                let entries = table(spec)?;
                if let Some(index) = entries.get("index") {
                    string(index)?;
                    if ["git", "url", "path", "workspace"]
                        .iter()
                        .any(|k| entries.contains_key(*k))
                    {
                        return Err("ValueError");
                    }
                    result.insert(Dependency {
                        name: normalized_name(name),
                        source: "registry".into(),
                        allowed: true,
                    });
                } else {
                    result.insert(declared_dependency(
                        &normalized_name(name),
                        spec,
                        manifest,
                        "python",
                    )?);
                }
            }
        }
    }
    Ok(result)
}

pub(crate) fn trailer_lines(messages: &[String]) -> Vec<&str> {
    let paragraphs = Regex::new(r"\n\s*\n").unwrap();
    let separators = Regex::new(r"\r\n|[\n\r\x0b\x0c\x1c-\x1e\u{85}\u{2028}\u{2029}]").unwrap();
    messages
        .iter()
        .flat_map(|message| {
            let parts: Vec<_> = paragraphs.split(message.trim()).collect();
            if parts.len() < 2 {
                return Vec::new();
            }
            let mut lines: Vec<_> = separators.split(parts.last().unwrap()).collect();
            if lines.last() == Some(&"") {
                lines.pop();
            }
            if lines
                .iter()
                .any(|line| !matches(r"\A[A-Za-z][A-Za-z0-9-]*:\s+.+\z", line))
            {
                Vec::new()
            } else {
                lines
            }
        })
        .collect()
}

fn dependency_reasons(messages: &[String]) -> BTreeSet<String> {
    let re = Regex::new(r"\ADependency:\s+(\S+)\s+(?:—|-)\s+(.+)\z").unwrap();
    trailer_lines(messages)
        .into_iter()
        .filter_map(|line| {
            re.captures(line)
                .and_then(|c| (!c[2].trim().is_empty()).then(|| c[1].to_owned()))
        })
        .collect()
}

fn urgent_dependencies(messages: &[String]) -> Urgent {
    let re = Regex::new(r"\ADependency-Urgent:\s+(\S+)@(\S+)\s+(\S+)\z").unwrap();
    trailer_lines(messages)
        .into_iter()
        .filter_map(|line| {
            re.captures(line)
                .map(|c| (c[1].to_owned(), c[2].to_owned()))
        })
        .collect()
}

type Manifests = BTreeMap<String, BTreeSet<Dependency>>;
fn violations(before: &Manifests, after: &Manifests, reasons: &BTreeSet<String>) -> Vec<String> {
    let mut result = Vec::new();
    for (manifest, deps) in after {
        let empty = BTreeSet::new();
        let old = before.get(manifest).unwrap_or(&empty);
        let old_names: BTreeSet<_> = old.iter().map(|d| &d.name).collect();
        for name in deps.iter().map(|d| &d.name).collect::<BTreeSet<_>>() {
            if !old_names.contains(name) && !reasons.contains(name) {
                result.push(format!(
                    "dep-gate: {manifest}: {name}: added dependency needs a reason"
                ));
            }
        }
        for name in deps
            .difference(old)
            .filter(|d| !d.allowed)
            .map(|d| &d.name)
            .collect::<BTreeSet<_>>()
        {
            result.push(format!("dep-gate: {manifest}: {name}: forbidden new dependency source; use github.com/tompassarelli/* or file:vendor/…"));
        }
    }
    result
}

fn jsonc(text: &str) -> Parsed<Value> {
    let re = Regex::new(r#""(?:[^"\\]|\\.)*"|//[^\n]*|/\*[\s\S]*?\*/"#).unwrap();
    let cleaned = re.replace_all(text, |c: &regex::Captures<'_>| {
        if c[0].starts_with('"') {
            c[0].to_owned()
        } else {
            " ".into()
        }
    });
    let re = Regex::new(r#""(?:[^"\\]|\\.)*"|,\s*([}\]])"#).unwrap();
    let cleaned = re.replace_all(&cleaned, |c: &regex::Captures<'_>| {
        if c[0].starts_with('"') {
            c[0].to_owned()
        } else {
            c[1].to_owned()
        }
    });
    let data = parse_json(&cleaned)?;
    table(&data)?;
    Ok(data)
}

fn locked_versions(path: &str, text: &str) -> Parsed<BTreeSet<(String, String)>> {
    let mut result = BTreeSet::new();
    if Path::new(path)
        .file_name()
        .is_some_and(|p| p == "Cargo.lock")
    {
        let data = parse_toml(text)?;
        if let Some(packages) = data.get("package") {
            for package in packages.as_array().ok_or("ValueError")? {
                table(package)?;
                if string(
                    package
                        .get("source")
                        .unwrap_or(&Value::String("local".into())),
                )?
                .starts_with("registry+")
                {
                    result.insert((
                        string(required(package, "name")?)?.to_owned(),
                        string(required(package, "version")?)?.to_owned(),
                    ));
                }
            }
        }
    } else {
        let data = jsonc(text)?;
        let re = Regex::new(r"\A((?:@[^/@\s]+/)?[^/@\s]+)@(\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?(?:\+[0-9A-Za-z.-]+)?)\z").unwrap();
        for (_, entry) in optional_table(&data, "packages")?.into_iter().flatten() {
            let entry = entry
                .as_array()
                .filter(|a| !a.is_empty())
                .ok_or("ValueError")?;
            let resolution = string(&entry[0])?;
            if let Some(c) = re.captures(resolution) {
                result.insert((c[1].to_owned(), c[2].to_owned()));
            }
        }
    }
    Ok(result)
}

fn locked_nodes(text: &str) -> Parsed<BTreeMap<String, Value>> {
    let data = parse_json(text)?;
    table(&data)?;
    let mut result = BTreeMap::new();
    for (name, node) in optional_table(&data, "nodes")?.into_iter().flatten() {
        table(node)?;
        if let Some(locked) = node.get("locked") {
            table(locked)?;
            result.insert(name.clone(), locked.clone());
        }
    }
    Ok(result)
}

fn first_party_flake(locked: &Value) -> bool {
    let kind = get(locked, "type").as_str().unwrap_or("");
    if kind == "github" {
        return get(locked, "owner")
            .as_str()
            .is_some_and(|s| s.eq_ignore_ascii_case("tompassarelli"));
    }
    let location = get(
        locked,
        if kind == "git" {
            "url"
        } else if kind == "path" {
            "path"
        } else {
            ""
        },
    )
    .as_str()
    .unwrap_or("");
    matches(
        r"(?i)\A(?:(?:git\+)?https://github\.com/tompassarelli/|(?:git\+)?ssh://git@github\.com/tompassarelli/|(?:git\+)?file:///home/[^/]+/code/|/home/[^/]+/code/)",
        location,
    )
}

fn input_node<'a>(nodes: &'a Value, root: &'a str, input: &'a Value, depth: usize) -> Option<&'a str> {
    if let Some(name) = input.as_str() {
        return Some(name);
    }
    let mut node = root;
    for step in input.as_array()? {
        let next = get(get(get(nodes, node), "inputs"), step.as_str()?);
        node = input_node(nodes, root, next, depth.checked_sub(1)?)?;
    }
    Some(node)
}

fn channel_nixpkgs(text: &str) -> Parsed<Option<String>> {
    let data = parse_json(text)?;
    let nodes = get(&data, "nodes");
    let Some(name) = get(&data, "root").as_str().and_then(|root| {
        input_node(nodes, root, get(get(get(nodes, root), "inputs"), "nixpkgs"), 16)
    }) else {
        return Ok(None);
    };
    let node = get(nodes, name);
    let official = |key: &str| {
        let source = get(node, key);
        get(source, "type").as_str() == Some("github")
            && get(source, "owner").as_str().is_some_and(|s| s.eq_ignore_ascii_case("nixos"))
            && get(source, "repo").as_str() == Some("nixpkgs")
    };
    let channel = get(get(node, "original"), "ref")
        .as_str()
        .is_some_and(|r| r.starts_with("nixos-"));
    Ok((official("locked") && official("original") && channel).then(|| name.to_owned()))
}

fn lock_candidates(
    before: &BTreeMap<String, String>,
    after: &BTreeMap<String, String>,
) -> Result<Vec<LockCandidate>, String> {
    let mut result = Vec::new();
    for (path, text) in after {
        if before.get(path) == Some(text) {
            continue;
        }
        let mut parse = || -> Parsed<()> {
            if Path::new(path)
                .file_name()
                .is_some_and(|p| p == "flake.lock")
            {
                let old = before
                    .get(path)
                    .map(|t| locked_nodes(t))
                    .transpose()?
                    .unwrap_or_default();
                let channel = channel_nixpkgs(text)?;
                for (name, locked) in locked_nodes(text)? {
                    if old.get(&name) != Some(&locked)
                        && !first_party_flake(&locked)
                        && channel.as_deref() != Some(name.as_str())
                    {
                        let version = ["rev", "version", "narHash"]
                            .into_iter()
                            .find_map(|k| locked.get(k))
                            .map(string)
                            .transpose()?
                            .unwrap_or("unknown");
                        result.push(LockCandidate {
                            path: path.clone(),
                            ecosystem: "flake".into(),
                            name: string(&name.into())?.to_owned(),
                            version: version.to_owned(),
                            published: get(&locked, "lastModified").as_i64().map(|t| t as f64),
                        });
                    }
                }
            } else {
                let old = before
                    .get(path)
                    .map(|t| locked_versions(path, t))
                    .transpose()?
                    .unwrap_or_default();
                for (name, version) in locked_versions(path, text)?.difference(&old) {
                    result.push(LockCandidate {
                        path: path.clone(),
                        ecosystem: if path.ends_with("Cargo.lock") {
                            "cargo"
                        } else {
                            "npm"
                        }
                        .into(),
                        name: name.clone(),
                        version: version.clone(),
                        published: None,
                    });
                }
            }
            Ok(())
        };
        parse().map_err(|kind| format!("cannot parse {path}: {kind}"))?;
    }
    Ok(result)
}

fn age_violations(
    candidates: &[LockCandidate],
    times: &BTreeMap<Identity, f64>,
    now: f64,
    urgent: &Urgent,
) -> Vec<String> {
    let mut result = Vec::new();
    for candidate in candidates {
        let flake = candidate.ecosystem == "flake";
        let published = if flake {
            candidate.published
        } else {
            times.get(&candidate.identity()).copied()
        };
        let urgent = urgent.contains(&(candidate.name.clone(), candidate.version.clone()));
        let identity = format!("{}@{}", candidate.name, candidate.version);
        if published.is_none() && (flake || !urgent) {
            result.push(format!(
                "dep-gate: {}: {identity}: missing {}",
                candidate.path,
                if flake {
                    "lastModified"
                } else {
                    "publish time"
                }
            ));
        } else if published.is_some_and(|t| t > now - COOLDOWN) && !urgent {
            result.push(format!(
                "dep-gate: {}: {identity}: {} less than 7 days ago",
                candidate.path,
                if flake { "lastModified" } else { "published" }
            ));
        }
    }
    result
}

fn quote(text: &str) -> String {
    percent_encoding::utf8_percent_encode(text, percent_encoding::NON_ALPHANUMERIC).to_string()
}

fn read_registry(host: &str, path: &str) -> Parsed<String> {
    let output = Command::new("curl")
        .args([
            "--silent",
            "--show-error",
            "--max-time",
            "10",
            "--user-agent",
            "dep-gate (https://github.com/tompassarelli/north)",
            "--write-out",
            "\n%{http_code}",
            &format!("https://{host}{path}"),
        ])
        .output()
        .map_err(|_| "FileNotFoundError")?;
    if !output.status.success() {
        return Err("OSError");
    }
    let output = String::from_utf8(output.stdout).map_err(|_| "UnicodeDecodeError")?;
    let (body, status) = output.rsplit_once('\n').ok_or("HTTPException")?;
    if status != "200" {
        return Err("HTTPException");
    }
    Ok(body.to_owned())
}

fn publish_times(
    candidates: &[LockCandidate],
    urgent: &Urgent,
) -> Result<BTreeMap<Identity, f64>, String> {
    let mut groups: BTreeMap<(String, String), BTreeSet<String>> = BTreeMap::new();
    for c in candidates {
        if c.ecosystem != "flake" && !urgent.contains(&(c.name.clone(), c.version.clone())) {
            groups
                .entry((c.ecosystem.clone(), c.name.clone()))
                .or_default()
                .insert(c.version.clone());
        }
    }
    let jobs: Vec<_> = groups.into_iter().collect();
    let api_read = std::sync::Mutex::new(None::<std::time::Instant>);
    std::thread::scope(|scope| {
        let mut output = BTreeMap::new();
        for batch in jobs.chunks(8) {
            let handles: Vec<_> = batch.iter().map(|((ecosystem,name),versions)| {
                let api_read = &api_read;
                scope.spawn(move || {
                    let fetch = || -> Parsed<BTreeMap<Identity,f64>> {
                        let data = if ecosystem == "npm" {
                            let data = parse_json(&read_registry("registry.npmjs.org",&format!("/{}",quote(name)))?)?;
                            required(&data,"time")?.clone()
                        } else {
                            let lower = name.to_lowercase();
                            let prefix = match lower.len() { 0..=2 => lower.len().to_string(), 3 => format!("3/{}",&lower[..1]), _ => format!("{}/{}",&lower[..2],&lower[2..4]) };
                            let body = read_registry("index.crates.io",&format!("/{prefix}/{}",quote(&lower)))?;
                            let mut rows = Map::new();
                            for line in body.lines() { let row = parse_json(line)?; rows.insert(string(required(&row,"vers")?)?.to_owned(),row); }
                            Value::Object(rows)
                        };
                        let mut result = BTreeMap::new();
                        for version in versions {
                            let row = required(&data,version)?;
                            let timestamp = if ecosystem == "npm" { row.clone() } else if !get(row,"pubtime").is_null() { row["pubtime"].clone() } else {
                                let mut last = api_read.lock().expect("registry rate lock");
                                if let Some(last) = *last { std::thread::sleep(Duration::from_secs(1).saturating_sub(last.elapsed())); }
                                *last = Some(std::time::Instant::now());
                                let response = parse_json(&read_registry("crates.io",&format!("/api/v1/crates/{}/{}",quote(name),quote(version)))?)?;
                                required(required(&response,"version")?,"created_at")?.clone()
                            };
                            let timestamp = string(&timestamp)?.replace('Z',"+00:00");
                            result.insert((ecosystem.clone(),name.clone(),version.clone()),Timestamp::parse_publish_time(&timestamp).ok_or("ValueError")?.unix_seconds_f64());
                        }
                        Ok(result)
                    };
                    fetch().map_err(|kind| format!("{ecosystem}: {name}: registry unavailable or invalid publish time; retry ({kind})"))
                })
            }).collect();
            for handle in handles {
                output.extend(handle.join().expect("registry reader thread")?);
            }
        }
        Ok(output)
    })
}

fn git(root: &Path, args: &[&str]) -> Result<String, String> {
    let stdout = platform_git::stdout(root, args, |output| {
        format!(
            "git {} failed (exit {}); check repository and revisions",
            args[0],
            output.status.code().unwrap_or(-1)
        )
    })?;
    String::from_utf8(stdout).map_err(|e| e.to_string())
}

struct Snapshot {
    manifests: BTreeMap<String, String>,
    locks: BTreeMap<String, String>,
}

const MANIFEST_NAMES: [&str; 3] = ["package.json", "Cargo.toml", "pyproject.toml"];
const LOCK_NAMES: [&str; 3] = ["bun.lock", "Cargo.lock", "flake.lock"];

fn read_snapshot(root: &Path, revision: &str) -> Result<Snapshot, String> {
    let listing = git(root, &["ls-tree", "-r", "-z", revision])?;
    let mut wanted: Vec<(&str, &str, bool)> = Vec::new();
    for entry in listing.split('\0') {
        let Some((meta, path)) = entry.split_once('\t') else { continue };
        let Some(oid) = meta.split(' ').nth(2) else { continue };
        let filename = path.rsplit('/').next().unwrap_or("");
        let manifest = MANIFEST_NAMES.contains(&filename);
        if !path.starts_with("repos/") && (manifest || LOCK_NAMES.contains(&filename)) {
            wanted.push((path, oid, manifest));
        }
    }
    let request: String = wanted.iter().map(|(_, oid, _)| format!("{oid}\n")).collect();
    let root_text = root.to_string_lossy();
    let output = crate::platform::process::run_with_input_within(
        "git",
        &["-C", root_text.as_ref(), "cat-file", "--batch"],
        Some(request.as_bytes()),
        Duration::from_secs(60),
    )
    .filter(|output| output.status.success())
    .ok_or("git cat-file failed; check repository and revisions")?;
    let bytes = output.stdout;
    let mut at = 0;
    let mut snapshot = Snapshot { manifests: BTreeMap::new(), locks: BTreeMap::new() };
    for (path, _, manifest) in wanted {
        let header_end = at + bytes[at..].iter().position(|&byte| byte == b'\n').ok_or("git cat-file output ended early")?;
        let header = String::from_utf8_lossy(&bytes[at..header_end]);
        let size: usize = header.rsplit(' ').next().and_then(|size| size.parse().ok()).ok_or_else(|| format!("git cat-file: {path}: {header}"))?;
        let body = bytes.get(header_end + 1..header_end + 1 + size).ok_or("git cat-file output ended early")?;
        let text = String::from_utf8(body.to_vec()).map_err(|e| e.to_string())?;
        at = header_end + 1 + size + 1;
        if manifest { snapshot.manifests.insert(path.to_owned(), text); } else { snapshot.locks.insert(path.to_owned(), text); }
    }
    Ok(snapshot)
}

fn parse_manifests(manifests: &BTreeMap<String, String>) -> Result<Manifests, String> {
    manifests
        .iter()
        .map(|(path, text)| {
            dependencies(path, text)
                .map_err(|kind| format!("cannot parse {path}: {kind}"))
                .map(|deps| (path.clone(), deps))
        })
        .collect()
}

fn evaluate(root: &Path, args: &[String]) -> Result<(u8, String), (u8, String)> {
    let check = || -> Result<(u8, String), (u8, String)> {
        if !git(root, &["rev-parse", "--show-prefix"])
            .map_err(|e| (1, e))?
            .trim()
            .is_empty()
        {
            return Err((1, "run dep-gate from the repository root".into()));
        }
        let revisions: Vec<_> = args
            .iter()
            .map(|rev| {
                git(
                    root,
                    &[
                        "rev-parse",
                        "--verify",
                        "--end-of-options",
                        &format!("{rev}^{{commit}}"),
                    ],
                )
                .map(|s| s.trim().to_owned())
            })
            .collect::<Result<_, _>>()
            .map_err(|e| (1, e))?;
        let (base, head) = (&revisions[0], &revisions[1]);
        let base_snapshot = read_snapshot(root, base).map_err(|e| (1, e))?;
        let head_snapshot = read_snapshot(root, head).map_err(|e| (1, e))?;
        let before = parse_manifests(&base_snapshot.manifests).map_err(|e| (1, e))?;
        let after = parse_manifests(&head_snapshot.manifests).map_err(|e| (1, e))?;
        let messages: Vec<_> = git(
            root,
            &["log", "--format=%B%x00", &format!("{base}..{head}")],
        )
        .map_err(|e| (1, e))?
        .split('\0')
        .map(str::to_owned)
        .collect();
        let failures = violations(&before, &after, &dependency_reasons(&messages));
        if !failures.is_empty() {
            return Ok((1, format!("{}\n{TRAILER_FORMAT}\n", failures.join("\n"))));
        }
        let candidates = lock_candidates(&base_snapshot.locks, &head_snapshot.locks).map_err(|e| (1, e))?;
        let urgent = urgent_dependencies(&messages);
        let times = publish_times(&candidates, &urgent).map_err(|e| (2, e))?;
        let now = Timestamp::now().unix_seconds_f64();
        let failures = age_violations(&candidates, &times, now, &urgent);
        if !failures.is_empty() {
            return Ok((1, format!("{}\n{URGENT_FORMAT}\n", failures.join("\n"))));
        }
        Ok((0, String::new()))
    };
    check()
}

pub fn run(args: &[String]) -> u8 {
    if args.len() != 2 {
        eprintln!("usage: dep-gate BASE HEAD\n{URGENT_FORMAT}");
        return 1;
    }
    let root = match std::env::current_dir() {
        Ok(root) => root,
        Err(e) => {
            eprintln!("dep-gate: {e}");
            return 1;
        }
    };
    match evaluate(&root, args) {
        Ok((code, output)) => {
            print!("{output}");
            code
        }
        Err((code, error)) => {
            eprintln!("dep-gate: {error}");
            code
        }
    }
}

#[cfg(test)]
#[path = "dep_gate_tests.rs"]
mod tests;
