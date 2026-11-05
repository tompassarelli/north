use std::collections::BTreeSet;
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::sync::LazyLock;

use regex::Regex;
use serde_json::Value;

const CATALOG_KEYS: [&str; 6] = ["$schema", "assets", "contracts", "package", "schema", "units"];
const ACTIVATION_FIELDS: [&str; 6] = ["rootOrder", "distributions", "targets", "supports", "owner", "providerSupport"];
const EXCLUDED_TEXT: [&str; 5] = ["PROVENANCE.md", "NOTICE", "LICENSE", "LICENSE-MIT", "LICENSE-APACHE"];
const FORBIDDEN_DIRS: [&str; 4] = ["adapters", "hooks", "providers", "secrets"];
const PROVIDER_INTERFACE_METADATA: &str = "agents/openai.yaml";
const LARGEST_SCANNED_FILE: u64 = 1_000_000;

static FORBIDDEN_TEXT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)/home/|~/|mcp__|\b(?:North|Firn|NixOS|Beagle)\b").expect("forbidden text pattern"));
static SCHEMA_ID: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^urn:agent-machinery:schema:[a-z][a-z0-9-]*:v[1-9][0-9]*$").expect("schema id pattern"));
static FRONTMATTER: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\A---\n([\s\S]*?)\n---").expect("frontmatter pattern"));
static SKILL_NAME: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?m)^name:\s*([^\n]+)$").expect("skill name pattern"));

pub(crate) fn check_package(root: &Path) -> Result<usize, String> {
    let catalog: Value = serde_json::from_str(&read(root, "catalog.json")?).map_err(|error| format!("catalog.json: {error}"))?;
    let keys: BTreeSet<&str> = catalog.as_object().map(|object| object.keys().map(String::as_str).collect()).unwrap_or_default();
    ensure(keys == BTreeSet::from(CATALOG_KEYS), "export catalog must retain the closed six-key package shape")?;
    ensure(catalog["schema"] == "agent-machinery.catalog/v1", "unexpected catalog schema")?;
    ensure(catalog["package"]["license"] == "MIT OR Apache-2.0", "unexpected package license")?;
    let serialized = catalog.to_string();
    for field in ACTIVATION_FIELDS {
        ensure(!serialized.contains(&format!("\"{field}\"")), &format!("export catalog contains activation field {field}"))?;
    }
    let units = check_units(root, list(&catalog, "units"))?;
    let (schema_paths, schema_ids) = check_assets_and_schemas(root, &catalog)?;
    ensure(schema_ids.contains(text(&catalog["$schema"])), "catalog $schema is absent from declared schema assets")?;
    check_contracts(root, list(&catalog, "contracts"), &schema_paths)?;
    check_boundary(root)?;
    Ok(units)
}

fn check_units(root: &Path, units: &[Value]) -> Result<usize, String> {
    let mut ids = BTreeSet::new();
    for unit in units {
        let id = text(&unit["id"]);
        ensure(ids.insert(id), &format!("duplicate unit id {id}"))?;
        let kind = text(&unit["kind"]);
        ensure(["module", "skill"].contains(&kind), &format!("invalid unit kind for {id}"))?;
        let source = contained(root, &unit["source"], &format!("unit {id} source"))?;
        ensure(source.exists(), &format!("missing unit source {}", text(&unit["source"])))?;
        if kind == "module" {
            ensure(unit["members"].as_array().is_some_and(|members| !members.is_empty()), &format!("module {id} has no members"))?;
        } else {
            ensure(unit.get("members").is_none(), &format!("skill {id} must not declare members"))?;
            ensure(skill_name(root, &source)? == id, &format!("unit {id} source name mismatch"))?;
        }
    }
    for unit in units.iter().filter(|unit| unit["kind"] == "module") {
        for member in list(unit, "members") {
            ensure(ids.contains(text(member)), &format!("module {} has unknown member {}", text(&unit["id"]), text(member)))?;
        }
    }
    Ok(units.len())
}

fn check_assets_and_schemas<'a>(root: &Path, catalog: &'a Value) -> Result<(Vec<&'a str>, BTreeSet<String>), String> {
    let mut asset_ids = BTreeSet::new();
    for asset in list(catalog, "assets") {
        let id = text(&asset["id"]);
        ensure(asset_ids.insert(id), &format!("duplicate asset id {id}"))?;
        ensure(["instructions", "catalog"].contains(&text(&asset["type"])), &format!("invalid asset type for {id}"))?;
        let path = contained(root, &asset["path"], &format!("asset {id}"))?;
        ensure(path.exists(), &format!("missing asset {}", text(&asset["path"])))?;
    }
    let mut schema_paths: Vec<&str> = Vec::new();
    let declared = list(catalog, "assets").iter().map(|asset| text(&asset["path"])).filter(|path| path.ends_with(".schema.json"));
    for path in declared.chain(list(catalog, "contracts").iter().map(|contract| text(&contract["schema"]))) {
        if !schema_paths.contains(&path) {
            schema_paths.push(path);
        }
    }
    let mut schema_ids = BTreeSet::new();
    for schema_path in &schema_paths {
        let path = contained(root, &Value::from(*schema_path), &format!("schema {schema_path}"))?;
        ensure(path.exists(), &format!("missing schema {schema_path}"))?;
        let schema: Value = serde_json::from_str(&fs::read_to_string(&path).map_err(|error| format!("{schema_path}: {error}"))?)
            .map_err(|error| format!("{schema_path}: {error}"))?;
        let schema_id = text(&schema["$id"]).to_owned();
        ensure(SCHEMA_ID.is_match(&schema_id), &format!("schema {schema_path} has no stable versioned package ID"))?;
        ensure(!schema_ids.contains(&schema_id), &format!("duplicate schema id {schema_id}"))?;
        schema_ids.insert(schema_id);
    }
    Ok((schema_paths, schema_ids))
}

fn check_contracts(root: &Path, contracts: &[Value], schema_paths: &[&str]) -> Result<(), String> {
    for contract in contracts {
        let id = text(&contract["id"]);
        ensure(schema_paths.contains(&text(&contract["schema"])), &format!("contract {id} schema is absent from catalog"))?;
        ensure(contract["schemaScope"] == "structural", &format!("contract {id} must classify its raw schema as structural"))?;
        ensure(contract["validator"] == "validateContract", &format!("contract {id} must use the composed validator"))?;
        let fixtures = contained(root, &contract["fixtures"], &format!("contract {id} fixtures"))?;
        ensure(fixtures.exists(), &format!("missing fixtures for {id}"))?;
    }
    Ok(())
}

fn check_boundary(root: &Path) -> Result<(), String> {
    for path in walk(root).map_err(|error| format!("{}: {error}", root.display()))? {
        let relative = path.strip_prefix(root).unwrap_or(&path).to_string_lossy().into_owned();
        let segments: Vec<&str> = relative.split('/').collect();
        for segment in &segments[..segments.len() - 1] {
            ensure(!FORBIDDEN_DIRS.contains(segment), &format!("forbidden source boundary directory: {relative}"))?;
        }
        ensure(
            relative != PROVIDER_INTERFACE_METADATA && !relative.ends_with(&format!("/{PROVIDER_INTERFACE_METADATA}")),
            &format!("provider interface metadata is outside package authority: {relative}"),
        )?;
        let size = fs::metadata(&path).map_err(|error| format!("{relative}: {error}"))?.len();
        if EXCLUDED_TEXT.contains(&relative.as_str()) || size > LARGEST_SCANNED_FILE {
            continue;
        }
        let bytes = fs::read(&path).map_err(|error| format!("{relative}: {error}"))?;
        ensure(!FORBIDDEN_TEXT.is_match(&String::from_utf8_lossy(&bytes)), &format!("non-portable source marker in {relative}"))?;
    }
    Ok(())
}

fn walk(directory: &Path) -> std::io::Result<Vec<PathBuf>> {
    let mut entries: Vec<_> = fs::read_dir(directory)?.collect::<Result<_, _>>()?;
    entries.sort_by_key(fs::DirEntry::file_name);
    let mut files = Vec::new();
    for entry in entries {
        if matches!(entry.file_name().to_str(), Some(".git" | "node_modules")) {
            continue;
        }
        if entry.file_type()?.is_dir() {
            files.extend(walk(&entry.path())?);
        } else {
            files.push(entry.path());
        }
    }
    Ok(files)
}

fn contained(root: &Path, value: &Value, label: &str) -> Result<PathBuf, String> {
    let relative = value.as_str().filter(|path| !path.is_empty() && !path.starts_with('/'));
    let relative = relative.ok_or_else(|| format!("{label} must be package-relative"))?;
    let mut depth = 0usize;
    let mut path = root.to_path_buf();
    for component in Path::new(relative).components() {
        match component {
            Component::Normal(part) => {
                depth += 1;
                path.push(part);
            }
            Component::ParentDir if depth > 0 => {
                depth -= 1;
                path.pop();
            }
            Component::CurDir => {}
            _ => return Err(format!("{label} escapes package")),
        }
    }
    ensure(depth > 0, &format!("{label} escapes package"))?;
    Ok(path)
}

fn skill_name(root: &Path, path: &Path) -> Result<String, String> {
    let shown = path.strip_prefix(root).unwrap_or(path).display().to_string();
    let source = fs::read_to_string(path).map_err(|error| format!("{shown}: {error}"))?;
    let frontmatter = FRONTMATTER.captures(&source).ok_or_else(|| format!("{shown} has no frontmatter"))?;
    let name = SKILL_NAME.captures(&frontmatter[1]).map(|name| name[1].trim().to_owned()).filter(|name| !name.is_empty());
    name.ok_or_else(|| format!("{shown} has no name"))
}

fn read(root: &Path, name: &str) -> Result<String, String> {
    fs::read_to_string(root.join(name)).map_err(|error| format!("{}: {error}", root.join(name).display()))
}

fn list<'a>(value: &'a Value, key: &str) -> &'a [Value] {
    value[key].as_array().map_or(&[], Vec::as_slice)
}

fn text(value: &Value) -> &str {
    value.as_str().unwrap_or("")
}

fn ensure(condition: bool, message: &str) -> Result<(), String> {
    if condition { Ok(()) } else { Err(message.to_owned()) }
}
