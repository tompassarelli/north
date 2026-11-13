use super::source::source_dependency;
use super::{Dependency, Parsed, get, matches, optional_table, parse_json, parse_toml, string, table};
use regex::Regex;
use serde_json::{Map, Value};
use std::collections::{BTreeMap, BTreeSet};

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

pub(super) fn dependencies(manifest: &str, text: &str) -> Parsed<BTreeSet<Dependency>> {
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

type Manifests = BTreeMap<String, BTreeSet<Dependency>>;
pub(super) fn violations(before: &Manifests, after: &Manifests, reasons: &BTreeSet<String>) -> Vec<String> {
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

pub(super) fn parse_manifests(manifests: &BTreeMap<String, String>) -> Result<Manifests, String> {
    manifests
        .iter()
        .map(|(path, text)| {
            dependencies(path, text)
                .map_err(|kind| format!("cannot parse {path}: {kind}"))
                .map(|deps| (path.clone(), deps))
        })
        .collect()
}
