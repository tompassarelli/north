use std::path::PathBuf;
use std::process::Command;

use north_base::platform::private_text;
use toml_edit::DocumentMut;

const SOUTH_ENV: &str = "SAFE_PUSH_SOUTH";
const KINDS_IN_SOUTH: &str = "origin/main:privacy/private-kinds.toml";

pub struct Kind {
    pub id: String,
    pub description: String,
}

pub struct Kinds {
    pub source: String,
    pub list: Vec<Kind>,
}

/// The kinds list: the file named by `--kinds` or the environment, else south's `origin/main` copy, so the list lives only in south.
pub fn load(named: Option<&str>) -> Result<Kinds, String> {
    let explicit = named.map(PathBuf::from).or_else(|| std::env::var_os(private_text::KINDS_ENV).filter(|value| !value.is_empty()).map(PathBuf::from));
    let source = match explicit {
        Some(file) => std::fs::read_to_string(&file).map_err(|error| format!("the private-kinds list {} cannot be read: {error}", file.display()))?,
        None => from_south()?,
    };
    parse(source)
}

fn from_south() -> Result<String, String> {
    let home = PathBuf::from(std::env::var_os("HOME").unwrap_or_default());
    let south = std::env::var_os(SOUTH_ENV).filter(|value| !value.is_empty()).map_or_else(|| home.join("code/south"), PathBuf::from);
    let output = Command::new("git").arg("-C").arg(south.join("main")).args(["show", KINDS_IN_SOUTH]).output().map_err(|error| format!("cannot run git: {error}"))?;
    if !output.status.success() {
        return Err(format!("the private-kinds list is missing from {} {KINDS_IN_SOUTH}", south.join("main").display()));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

pub fn parse(source: String) -> Result<Kinds, String> {
    let document: DocumentMut = source.parse().map_err(|error| format!("the private-kinds list is not valid TOML: {error}"))?;
    let tables = document.get("kind").and_then(|item| item.as_array_of_tables()).ok_or("the private-kinds list has no [[kind]] entries")?;
    let list: Vec<Kind> = tables
        .iter()
        .map(|table| {
            let field = |name: &str| table.get(name).and_then(|item| item.as_str()).filter(|value| !value.trim().is_empty()).map(str::to_owned);
            field("id").zip(field("description")).map(|(id, description)| Kind { id, description }).ok_or("a [[kind]] entry lacks an id or a description")
        })
        .collect::<Result<_, _>>()?;
    if list.is_empty() {
        return Err("the private-kinds list has no [[kind]] entries".to_owned());
    }
    Ok(Kinds { source, list })
}
