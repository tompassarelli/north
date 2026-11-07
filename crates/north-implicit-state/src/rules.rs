#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Language {
    Rust,
    TypeScript,
    Tsx,
    JavaScript,
}

pub(crate) fn language(path: &str) -> Option<Language> {
    let extension = path.rsplit_once('.').map(|(_, extension)| extension)?;
    Some(match extension {
        "rs" => Language::Rust,
        "ts" | "mts" | "cts" => Language::TypeScript,
        "tsx" => Language::Tsx,
        "js" | "mjs" | "cjs" | "jsx" => Language::JavaScript,
        _ => return None,
    })
}

const PHASE_SUFFIX: &str = "staging|stage|old|partial|done|tried|pending|running|failed|inprogress|started|finished|complete|held|paused|claimed";
const UNIT_INTENT: &str = "stop|disable|mask";
const UNIT_READ: &str = "is-active|is-enabled|is-failed|.*(?:ActiveState|SubState|UnitFileState).*";

const RUST: &str = r#"id: test-scope
rule:
  any:
    - { kind: mod_item, follows: { kind: attribute_item, regex: 'cfg\s*\(\s*test\s*\)' } }
    - { kind: function_item, follows: { kind: attribute_item, regex: '^#\[\s*(tokio::)?test' } }
---
id: marker-stamp
rule:
  kind: call_expression
  any:
    - has: { field: function, kind: field_expression, has: { field: field, regex: '^(set_modified|set_times)$' } }
    - has: { field: function, kind: scoped_identifier, has: { field: name, regex: '^(set_file_mtime|set_file_times|set_file_handle_times|set_symlink_file_times)$' } }
    - all:
        - has: { field: function, regex: 'Command::new$' }
        - has: { field: arguments, regex: '^\(\s*"touch"\s*\)$' }
---
id: mtime-read
rule:
  kind: call_expression
  all:
    - has: { field: function, kind: field_expression, has: { field: field, regex: '^(modified|accessed|created)$' } }
    - has: { field: arguments, regex: '^\(\s*\)$' }
---
id: presence-check
rule:
  kind: call_expression
  any:
    - all:
        - has: { field: function, kind: field_expression, all: [ { has: { field: field, regex: '^(exists|is_file|try_exists)$' } }, { has: { field: value, pattern: $P } } ] }
        - has: { field: arguments, regex: '^\(\s*\)$' }
    - all:
        - has: { field: function, regex: '(^|::)(fs::exists|fs::try_exists)$' }
        - has: { field: arguments, has: { pattern: $P } }
---
id: presence-read
rule:
  pattern: $F($P).ok()
constraints:
  F: { regex: '(^|::)read_to_string$' }
---
id: state-write
rule:
  any:
    - pattern: $F($P, $$$)
    - pattern: $F($P)
constraints:
  F: { regex: '(^|::)(fs::write|File::create|File::create_new|fs::remove_file)$' }
---
id: state-open
rule:
  pattern: $O.open($P)
constraints:
  O: { regex: 'create(_new)?\(\s*true\s*\)' }
---
id: pid-liveness
rule:
  kind: call_expression
  any:
    - has: { field: function, kind: field_expression, all: [ { has: { field: field, regex: '^(exists|is_dir|try_exists)$' } }, { has: { field: value, regex: '"/proc' } } ] }
    - all:
        - has: { field: function, regex: '(^|::)kill$' }
        - has: { field: arguments, regex: ',\s*(0|None)\s*\)$' }
    - has: { field: function, regex: '(^|::)test_kill_process$' }
---
id: phase-suffix
rule:
  any:
    - { kind: string_literal, regex: '^"[^" ]*\.(PHASE)\.?"$' }
    - all:
        - kind: call_expression
        - has: { field: function, kind: field_expression, has: { field: field, regex: '^join$' } }
        - has: { field: arguments, regex: '^\(\s*"(PHASE)/?"\s*\)$' }
    - all:
        - kind: call_expression
        - has: { field: function, kind: field_expression, has: { field: field, regex: '^with_extension$' } }
        - has: { field: arguments, regex: '^\(\s*"(PHASE)"\s*\)$' }
---
id: unit-intent
rule: { kind: string_literal, regex: '^"(INTENT)"$', inside: { kind: arguments, stopBy: end } }
---
id: unit-read
rule: { kind: string_literal, regex: '^"(UNITREAD)"$' }
---
id: link-read
rule:
  kind: call_expression
  has: { field: function, regex: '(^|::|\.)read_link$' }
---
id: lock-flag
rule:
  kind: call_expression
  any:
    - has: { field: function, kind: field_expression, has: { field: field, regex: '^(try_lock|try_lock_shared|try_lock_exclusive)$' } }
    - has: { field: function, regex: '(^|::)flock$' }
---
id: status-compare
rule:
  kind: binary_expression
  all:
    - has: { field: operator, regex: '^(==|!=)$' }
    - has: { field: left, pattern: $L }
    - has: { field: right, pattern: $R }
  not: { inside: { kind: function_item, has: { field: name, regex: 'NAMES' }, stopBy: end } }
---
id: status-prefix
rule:
  kind: call_expression
  all:
    - has: { field: function, kind: field_expression, all: [ { has: { field: field, regex: '^(starts_with|ends_with|strip_prefix|strip_suffix)$' } }, { has: { field: value, pattern: $L } } ] }
    - has: { field: arguments, regex: '^\(\s*"[A-Za-z][A-Za-z0-9_ -]*"\s*\)$' }
  not: { inside: { kind: function_item, has: { field: name, regex: 'NAMES' }, stopBy: end } }
---
id: status-match
rule:
  kind: match_expression
  all:
    - has: { field: value, pattern: $L }
    - has: { field: body, has: { kind: match_arm, has: { field: pattern, has: { kind: string_literal, stopBy: end } } } }
  not: { inside: { kind: function_item, has: { field: name, regex: 'NAMES' }, stopBy: end } }
---
id: status-matches
rule:
  kind: macro_invocation
  has: { field: macro, regex: '^matches$' }
  not: { inside: { kind: function_item, has: { field: name, regex: 'NAMES' }, stopBy: end } }
---
id: status-sql
rule: { any: [ { kind: string_literal }, { kind: raw_string_literal } ], regex: '(?i)(create\s+table|\bwhere\b|\band\b|\bset\b)' }
---
id: state-struct
rule:
  kind: struct_item
  has: { field: name, pattern: $N }
"#;

const SCRIPT: &str = r#"id: marker-stamp
rule:
  kind: call_expression
  has: { field: function, regex: '(^|\.)(utimes|utimesSync|futimes|futimesSync|lutimes|lutimesSync)$' }
---
id: mtime-read
rule:
  kind: member_expression
  has: { field: property, regex: '^(mtime|mtimeMs|atime|atimeMs)$' }
---
id: pid-liveness
rule:
  any:
    - pattern: process.kill($P, 0)
    - { kind: template_string, regex: '^.\/proc\/\$\{[^}]*\}.$' }
---
id: phase-suffix
rule: { kind: string, regex: '^.[^ ''"`]*\.(PHASE).$' }
---
id: unit-intent
rule: { kind: string, regex: '^.(INTENT).$', inside: { kind: arguments, stopBy: end } }
---
id: unit-read
rule: { kind: string, regex: '^.(UNITREAD).$' }
"#;

const PARSERS: &str = "^(from_str|try_from|from|parse[a-z_]*|named|from_name|of_name)$";

pub(crate) fn rules(language: Language) -> String {
    let (name, body) = match language {
        Language::Rust => ("Rust", RUST),
        Language::TypeScript => ("TypeScript", SCRIPT),
        Language::Tsx => ("Tsx", SCRIPT),
        Language::JavaScript => ("JavaScript", SCRIPT),
    };
    body.replace("PHASE", PHASE_SUFFIX)
        .replace("INTENT", UNIT_INTENT)
        .replace("UNITREAD", UNIT_READ)
        .replace("NAMES", PARSERS)
        .split("\n---\n")
        .map(|rule| rule.replacen("\nrule:", &format!("\nlanguage: {name}\nrule:"), 1))
        .collect::<Vec<_>>()
        .join("\n---\n")
}
