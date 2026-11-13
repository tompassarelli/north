//! `north config operator paths [--class CLASS]`: the paths north reads from the operator repository, one `PATH<TAB>CLASS` line each,
//! in the order of the inventory in `docs/rationale/operator-repository.md` (north#1281).
pub(in crate::cli) mod check;
pub(in crate::cli) mod init;

use crate::error::{NorthError, NorthResult};

const USAGE: &str = "usage: north config operator paths [--class CLASS] | check [DIR] [--only NAME,...] | init DIR";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Class {
    Schema,
    GenericMethod,
    PrivateValue,
    Secret,
}

use Class::{GenericMethod, PrivateValue, Schema, Secret};

const INVENTORY: &[(&str, Class)] = &[
    ("agents/orchestration.toml", Schema),
    ("agents/costs.toml", Schema),
    ("agents/claude-accounts.toml", Schema),
    ("agents/credentials.toml", Schema),
    ("agents/frozen-topics.toml", Schema),
    ("agents/operating-point.toml", Schema),
    ("agents/watch.toml", Schema),
    ("agents/rules.toml", Schema),
    ("agents/incidents.jsonl", Schema),
    ("agents/catalog-config.json", Schema),
    ("agents/catalog-config.json", GenericMethod),
    ("agents/routines/*.md", Schema),
    ("agents/replay/", Schema),
    ("privacy/public-denylist.toml", Schema),
    ("privacy/private-kinds.toml", Schema),
    ("infra/manifest.toml", Schema),
    ("agents/AGENTS.md", PrivateValue),
    ("agents/skills/", PrivateValue),
    ("agents/decisions/", PrivateValue),
    ("agents/", PrivateValue),
    (".", PrivateValue),
    ("hosts/", PrivateValue),
    ("modules/", PrivateValue),
    ("bin/", PrivateValue),
    (".git", PrivateValue),
    ("secrets/github-app.yaml", Secret),
    ("secrets/north-lander.yaml", Secret),
    ("secrets/digitalocean.yaml", Secret),
    ("secrets/hetzner-cloud.yaml", Secret),
    ("secrets/cloudflare.yaml", Secret),
    ("secrets/github-user.yaml", Secret),
    ("secrets/", Secret),
];

impl Class {
    const ALL: [Self; 4] = [Schema, GenericMethod, PrivateValue, Secret];

    fn name(self) -> &'static str {
        match self {
            Self::Schema => "schema",
            Self::GenericMethod => "generic method",
            Self::PrivateValue => "private value",
            Self::Secret => "secret",
        }
    }

    fn named(text: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|class| class.name() == text)
    }
}

pub fn run(arguments: &[String]) -> NorthResult<()> {
    match arguments {
        [verb, rest @ ..] if verb == "paths" => paths(rest),
        [verb, rest @ ..] if verb == "check" => check::run(rest),
        [verb, rest @ ..] if verb == "init" => init::run(rest),
        _ => Err(NorthError::Usage(USAGE.into())),
    }
}

fn paths(arguments: &[String]) -> NorthResult<()> {
    let wanted = match arguments {
        [] => None,
        [flag, value] if flag == "--class" => Some(Class::named(value).ok_or_else(|| NorthError::Usage(format!("{USAGE}; CLASS is one of schema, generic method, private value, secret")))?),
        _ => return Err(NorthError::Usage(USAGE.into())),
    };
    for (path, class) in INVENTORY.iter().filter(|(_, class)| wanted.is_none_or(|wanted| *class == wanted)) {
        println!("{path}\t{}", class.name());
    }
    Ok(())
}
