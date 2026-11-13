//! `north config operator init DIR`: writes the operator repository's catalog, one registration per `skill.*` module of north's `structure.toml`.
use std::fs;
use std::path::Path;

use north_core::agent_catalog::initial_catalog;

use crate::error::{NorthError, NorthResult};
use crate::platform::harness;

pub const USAGE: &str = "usage: north config operator init DIR";

pub fn run(arguments: &[String]) -> NorthResult<()> {
    let [directory] = arguments else {
        return Err(NorthError::Usage(USAGE.into()));
    };
    let directory = Path::new(directory);
    if directory.exists() && (!directory.is_dir() || fs::read_dir(directory)?.next().is_some()) {
        return Err(NorthError::Configuration(format!("{} exists and is not empty", directory.display())));
    }
    let north = harness::harness_root().map_err(NorthError::Configuration)?;
    let structure = fs::read_to_string(north.join("structure.toml"))?;
    let catalog = initial_catalog(&structure)?;
    let agents = directory.join("agents");
    fs::create_dir_all(&agents)?;
    fs::write(agents.join("catalog-config.json"), serde_json::to_string_pretty(&catalog)? + "\n")?;
    println!("wrote {}", agents.join("catalog-config.json").display());
    Ok(())
}
