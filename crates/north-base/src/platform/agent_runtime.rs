pub const NAMING_FILE: &str = "/etc/north/agent-runtime-link";

pub const CLAUDE_LINK: &str = "agent-claude";

pub fn claude_script() -> String {
    claude_script_naming(NAMING_FILE)
}

pub fn claude_script_naming(naming_file: &str) -> String {
    format!(
        r#"f=$(head -n 1 -- {naming_file} 2>/dev/null) && case "$f" in /*) ;; *) exit 127;; esac && [ "$(stat -L -c %u -- {naming_file})" = 0 ] && m=$(stat -L -c %a -- {naming_file}) && case "$m" in ?[2367]?|??[2367]) exit 127;; esac && exec "$(dirname -- "$f")/{CLAUDE_LINK}/bin/claude" "$@"; exit 127"#
    )
}
