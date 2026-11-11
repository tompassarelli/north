use std::collections::{BTreeMap, BTreeSet};

use crate::workspace::{Kind, Package, Target};

const TARGET_TMPDIR: &str = "/tmp";

pub struct Context<'a> {
    pub package: &'a Package,
    pub target: &'a Target,
    pub binaries: &'a BTreeMap<String, String>,
    pub unittest: bool,
}

impl Context<'_> {
    fn integration(&self) -> bool {
        self.target.kind == Kind::Test && !self.unittest
    }
}

pub fn values(context: &Context, reads: &BTreeSet<String>, crate_name: &str) -> Result<BTreeMap<String, String>, String> {
    let mut values = BTreeMap::new();
    for var in reads.iter().filter(|var| var.starts_with("CARGO") && !set_elsewhere(context, var)) {
        let value = value(context, crate_name, var).ok_or_else(|| format!("{}: target {} reads {var}, which north buck targets does not set", context.package.name, context.target.name))?;
        values.insert(var.clone(), value);
    }
    Ok(values)
}

pub fn run_values(context: &Context) -> BTreeMap<String, String> {
    if !context.integration() {
        return BTreeMap::new();
    }
    context.binaries.iter().map(|(binary, rule)| (format!("CARGO_BIN_EXE_{binary}"), format!("$(location :{rule})"))).collect()
}

fn set_elsewhere(context: &Context, var: &str) -> bool {
    var == "CARGO_MANIFEST_DIR" || (context.integration() && var.starts_with("CARGO_BIN_EXE_"))
}

fn value(context: &Context, crate_name: &str, var: &str) -> Option<String> {
    if let Some(field) = var.strip_prefix("CARGO_PKG_") {
        return package_value(context.package, field);
    }
    Some(match var {
        "CARGO" => "cargo".to_owned(),
        "CARGO_CRATE_NAME" => crate_name.to_owned(),
        "CARGO_PRIMARY_PACKAGE" => "1".to_owned(),
        "CARGO_BIN_NAME" if matches!(context.target.kind, Kind::Bin | Kind::Example) => context.target.name.clone(),
        "CARGO_TARGET_TMPDIR" if context.integration() => TARGET_TMPDIR.to_owned(),
        _ => return None,
    })
}

fn package_value(package: &Package, field: &str) -> Option<String> {
    let text = |value: &Option<String>| value.clone().unwrap_or_default();
    Some(match field {
        "NAME" => package.name.clone(),
        "VERSION" => package.version.clone(),
        "VERSION_MAJOR" => version_part(&package.version, 0)?,
        "VERSION_MINOR" => version_part(&package.version, 1)?,
        "VERSION_PATCH" => version_part(&package.version, 2)?,
        "VERSION_PRE" => package.version.split_once('-').map_or("", |(_, pre)| pre).to_owned(),
        "AUTHORS" => package.authors.join(":"),
        "DESCRIPTION" => text(&package.description),
        "LICENSE" => text(&package.license),
        "REPOSITORY" => text(&package.repository),
        "HOMEPAGE" => text(&package.homepage),
        "RUST_VERSION" => text(&package.rust_version),
        _ => return None,
    })
}

fn version_part(version: &str, index: usize) -> Option<String> {
    version.split(['.', '-', '+']).nth(index).map(str::to_owned)
}
