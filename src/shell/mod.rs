mod heredoc;
mod lexer;
mod scan;
mod segments;
mod substitution;
#[cfg(test)]
mod tests;

#[allow(unused_imports)]
pub use heredoc::{blank, heredoc_bodies, strip_heredoc_bodies, strip_heredocs};
#[allow(unused_imports)]
pub use lexer::{
    SEPARATORS, ShellToken, argv_spans, basename, operator_tokens, shlex_split, tokenize,
    whitespace, words,
};
#[allow(unused_imports)]
pub use segments::{
    blank_quotes, command, command_index, is_separator, segments, shell_segments_sep, strip_prefixes,
    strip_quotes, tripwire_tokens, tripwire_word,
};
#[allow(unused_imports)]
pub use substitution::{shell_substitutions, unquoted_mask};

use serde_json::{Value, json};

#[allow(dead_code)] // exercised by tests and the shellcmd_compare example, not the binary
pub fn rewrite_input(data: &Value, field: &str, value: &str, context: &str) -> Value {
    let mut input = data["tool_input"].clone();
    input[field] = json!(value);
    json!({"hookSpecificOutput": {"hookEventName": "PreToolUse", "permissionDecision": "allow", "updatedInput": input, "additionalContext": context}})
}
