pub fn repo_alias(name: &str) -> &str {
    match name {
        "nixos-config" => "firn",
        other => other,
    }
}

pub fn checkout_name(repo: &str) -> &str {
    match repo {
        "firn" => "nixos-config",
        other => other,
    }
}

pub const OWNER: &str = "tompassarelli";
pub const REPOS: [&str; 11] = ["makgora", "makgora-client", "padbridge", "wisp", "firn", "north", "south", "gjoa", "fram", "wake", "battlecraft"];
pub const ALIASES: [(&str, &str); 4] = [("nixos-config", "firn"), ("smashcraft", "makgora"), ("smashcraft-client", "makgora-client"), ("smashcraft-dev", "makgora")];
pub const NATIVE: [&str; 4] = ["life", "accounts", "travel", "ops"];

pub fn canonical(name: &str) -> &str {
    let name = name.strip_prefix(OWNER).and_then(|rest| rest.strip_prefix('/')).unwrap_or(name);
    let known = REPOS.iter().chain(&NATIVE).copied().chain(ALIASES.iter().map(|(alias, _)| *alias)).find(|known| known.eq_ignore_ascii_case(name));
    let name = known.unwrap_or(name);
    ALIASES.iter().find(|(alias, _)| *alias == name).map_or(name, |(_, repo)| repo)
}
