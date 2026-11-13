const AUTOLAND: [&str; 2] = ["makgora", "wisp"];

pub fn branch(slug: &str) -> String {
    format!("claude/land-{slug}")
}

pub fn autoland(repo: &str) -> bool {
    AUTOLAND.contains(&repo)
}
