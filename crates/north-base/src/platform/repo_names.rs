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
