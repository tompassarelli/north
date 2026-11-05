use toml_edit::{Array, DocumentMut, Item, Table};

pub const CLASSES: [&str; 4] = ["moderate", "heavy", "gpu", "exclusive"];

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Step {
    pub dir: String,
    pub command: Vec<String>,
    pub class: String,
    pub timeout_seconds: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Check {
    pub name: String,
    pub repo: String,
    pub checkout: String,
    pub context: String,
    pub paths: Vec<String>,
    pub steps: Vec<Step>,
}

fn text(table: &Table, owner: &str, key: &str) -> Result<String, String> {
    table.get(key).and_then(Item::as_str).filter(|value| !value.is_empty()).map(str::to_owned).ok_or_else(|| format!("{owner}: {key} is missing"))
}

fn strings(array: Option<&Array>, owner: &str, key: &str) -> Result<Vec<String>, String> {
    let array = array.ok_or_else(|| format!("{owner}: {key} is missing"))?;
    let values = array.iter().map(|value| value.as_str().filter(|text| !text.is_empty()).map(str::to_owned)).collect::<Option<Vec<_>>>();
    values.filter(|values| !values.is_empty()).ok_or_else(|| format!("{owner}: {key} must be a non-empty list of non-empty strings"))
}

fn step(table: &Table, owner: &str) -> Result<Step, String> {
    let command = strings(table.get("command").and_then(Item::as_array), owner, "command")?;
    let owner = format!("{owner} step {}", command.join(" "));
    let class = text(table, &owner, "class")?;
    if !CLASSES.contains(&class.as_str()) {
        return Err(format!("{owner}: class {class} is not one of {}", CLASSES.join(", ")));
    }
    let timeout_seconds = table.get("timeout_seconds").and_then(Item::as_integer).filter(|seconds| (1..=3600).contains(seconds));
    let timeout_seconds = timeout_seconds.ok_or_else(|| format!("{owner}: timeout_seconds must be 1 to 3600"))?;
    let dir = table.get("dir").and_then(Item::as_str).unwrap_or(".").to_owned();
    if dir.starts_with('/') || dir.split('/').any(|part| part == "..") {
        return Err(format!("{owner}: dir {dir} must stay inside the checkout"));
    }
    Ok(Step { dir, command, class, timeout_seconds: timeout_seconds.unsigned_abs() })
}

fn slug(value: &str) -> bool {
    !value.is_empty() && value.chars().all(|character| character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.'))
}

fn check(table: &Table) -> Result<Check, String> {
    let name = text(table, "a check", "name")?;
    if !slug(&name) {
        return Err(format!("check {name}: name must be letters, digits, '-', '_' or '.'"));
    }
    let owner = format!("check {name}");
    let repo = text(table, &owner, "repo")?;
    if repo.split('/').count() != 2 || !repo.split('/').all(slug) {
        return Err(format!("{owner}: repo {repo} is not OWNER/NAME"));
    }
    let checkout = text(table, &owner, "checkout")?;
    if !slug(&checkout) {
        return Err(format!("{owner}: checkout {checkout} is not a directory name under ~/code"));
    }
    let steps = table.get("step").and_then(Item::as_array_of_tables).ok_or_else(|| format!("{owner}: no [[check.step]] tables"))?;
    Ok(Check {
        context: text(table, &owner, "context")?,
        paths: strings(table.get("paths").and_then(Item::as_array), &owner, "paths")?,
        steps: steps.iter().map(|one| step(one, &owner)).collect::<Result<_, _>>()?,
        name,
        repo,
        checkout,
    })
}

pub fn parse(source: &str) -> Result<Vec<Check>, String> {
    let document: DocumentMut = source.parse().map_err(|error| format!("private checks: {error}"))?;
    let Some(tables) = document.get("check") else { return Ok(Vec::new()) };
    let tables = tables.as_array_of_tables().ok_or("private checks: check must be [[check]] tables")?;
    let checks = tables.iter().map(check).collect::<Result<Vec<_>, _>>()?;
    for (index, one) in checks.iter().enumerate() {
        if checks[..index].iter().any(|earlier| earlier.name == one.name) {
            return Err(format!("private checks: {} is listed twice", one.name));
        }
    }
    Ok(checks)
}
