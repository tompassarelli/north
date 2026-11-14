use super::policy::Defaults;
use regex::Regex;
use std::collections::BTreeMap;
use std::sync::LazyLock;

pub use north_strategy_types::brief::{CATEGORIES, FACETS, SPEC_EXACT};

pub const ART: [&str; 3] = ["anim", "animation", "art"];

/// Refuses a category outside the brief vocabulary and the art words, naming the valid list.
pub fn known_category(category: &str) -> Result<(), String> {
    if CATEGORIES.contains(&category) || ART.contains(&category) {
        return Ok(());
    }
    Err(format!("`Category: {category}` is no category; use one of {}, or {}", CATEGORIES.join(", "), ART.join(", ")))
}

static FIELD: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^([A-Za-z][A-Za-z ]*?):[ \t]*(.*)$").unwrap());
static WORD: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^([a-z0-9-]+)(?:\s+\(([^()]*)\))?$").unwrap());
static TOKEN: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[a-z0-9-]+").unwrap());
static MINUTES: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^(?:(\d+)\s*-\s*)?(\d+)\s*(?:m|mins?|minutes?)?(?:\s+\([^()]*\))?$").unwrap()
});
static COMMAND: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"`[^`\n]+`").unwrap());
static PATH: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[A-Za-z0-9_./-]+$").unwrap());
static BULLET: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[-*]\s+").unwrap());

pub fn brief_header(text: &str) -> BTreeMap<String, String> {
    let mut fields = BTreeMap::new();
    let mut last = None;
    for line in text.lines() {
        if line.trim().is_empty() {
            break;
        }
        if let Some(captures) = FIELD.captures(line) {
            let name = captures[1].to_lowercase();
            fields
                .entry(name.clone())
                .or_insert_with(|| captures[2].to_owned());
            last = Some(name);
        } else if (line.starts_with(char::is_whitespace)
            || line.trim_start().starts_with("- ")
            || line.trim_start().starts_with("* "))
            && let Some(name) = &last
            && let Some(value) = fields.get_mut(name)
        {
            value.push('\n');
            value.push_str(line);
        }
    }
    fields
}

pub fn header_word(
    fields: &BTreeMap<String, String>,
    name: &str,
    defaults: &Defaults,
) -> Option<String> {
    let raw = fields.get(name)?.trim().to_lowercase();
    let captures = WORD.captures(&raw)?;
    let facets: Vec<&str> = match FACETS.iter().find(|(facet, _)| facet.eq_ignore_ascii_case(name)) {
        Some((_, values)) => values.to_vec(),
        None => defaults.codex_route.keys().map(String::as_str).collect(),
    };
    if captures.get(2).is_some_and(|note| {
        TOKEN
            .find_iter(note.as_str())
            .any(|token| facets.contains(&token.as_str()))
    }) {
        return None;
    }
    Some(captures[1].to_owned())
}

pub fn header_minutes(value: &str) -> Option<u64> {
    let raw = value.trim().to_lowercase();
    let captures = MINUTES.captures(&raw)?;
    let end = captures[2].parse().ok()?;
    if let Some(start) = captures.get(1)
        && start.as_str().parse::<u64>().ok()? > end
    {
        return None;
    }
    Some(end)
}

pub fn header_files(value: &str) -> Vec<String> {
    value
        .split([',', '\n'])
        .filter_map(|name| {
            let name = name.trim();
            let name = BULLET.replace(name, "");
            let name = name.trim();
            let name = name.trim_matches(['`', '"', '\'']);
            (!name.is_empty()).then(|| name.to_owned())
        })
        .collect()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodexClass {
    pub route: String,
    pub tiers: Vec<String>,
}

pub fn codex_class(category: Option<&str>, defaults: &Defaults) -> CodexClass {
    CodexClass {
        route: category
            .and_then(|c| defaults.codex_route.get(c))
            .cloned()
            .unwrap_or_else(|| "claude".into()),
        tiers: category
            .and_then(|c| defaults.codex_tier.get(c))
            .cloned()
            .unwrap_or_default(),
    }
}

pub fn codex_qualifies(
    text: &str,
    shape_only: bool,
    defaults: &Defaults,
) -> (bool, Option<String>) {
    qualifies(text, shape_only, false, defaults)
}

pub fn codex_explore_qualifies(text: &str, defaults: &Defaults) -> (bool, Option<String>) {
    qualifies(text, false, true, defaults)
}

fn qualifies(
    text: &str,
    shape_only: bool,
    explore: bool,
    defaults: &Defaults,
) -> (bool, Option<String>) {
    let fields = brief_header(text);
    let get = |name: &str| fields.get(name).map(String::as_str).unwrap_or("");
    let shown = |name: &str| {
        let first = get(name).trim().split('\n').next().unwrap_or("");
        if first.is_empty() { "missing" } else { first }
    };
    let fail = |why: String| (false, Some(why));
    if fields.is_empty() {
        return fail("Header missing".into());
    }
    let category = header_word(&fields, "category", defaults);
    let spec = header_word(&fields, "spec", defaults);
    if !(shape_only || explore) && codex_class(category.as_deref(), defaults).route != "codex" {
        return fail(format!("Category {}", shown("category")));
    }
    if spec.as_deref() != Some("exact")
        && !(spec.as_deref() == Some("measured")
            && matches!(category.as_deref(), Some("tooling" | "native-check")))
    {
        return fail(format!("Spec {}", shown("spec")));
    }
    if let Some(why) = vocabulary_refusal(&fields, explore, defaults) {
        return fail(why);
    }
    if !COMMAND.is_match(get("done when")) {
        return fail("Done when has no command".into());
    }
    if !get("open questions").trim().is_empty() {
        return fail("Open questions".into());
    }
    if let Some(why) = limits_refusal(&fields, defaults) {
        return fail(why);
    }
    (true, None)
}

fn vocabulary_refusal(
    fields: &BTreeMap<String, String>,
    explore: bool,
    defaults: &Defaults,
) -> Option<String> {
    let (surfaces, verifies) = if explore {
        (
            &["shell", "docs", "nix", "assets"][..],
            &["none", "local-test", "native", "visual"][..],
        )
    } else {
        (
            &["shell", "docs", "nix"][..],
            &["none", "local-test", "native"][..],
        )
    };
    for (name, title, allowed) in [
        ("scope", "Scope", &["one-file", "module"][..]),
        ("surface", "Surface", surfaces),
        ("verify", "Verify", verifies),
    ] {
        if !header_word(fields, name, defaults).is_some_and(|word| allowed.contains(&word.as_str()))
        {
            return Some(format!("{title} {}", shown(fields, name)));
        }
    }
    None
}

fn limits_refusal(fields: &BTreeMap<String, String>, defaults: &Defaults) -> Option<String> {
    let get = |name: &str| fields.get(name).map(String::as_str).unwrap_or("");
    if header_minutes(get("eta")).is_none_or(|eta| eta > defaults.codex_eta_max_min) {
        return Some(format!("ETA {}", shown(fields, "eta")));
    }
    let files = header_files(get("files"));
    if files.is_empty()
        || files.len() as u64 > defaults.codex_files_max
        || !files.iter().all(|name| PATH.is_match(name))
    {
        return Some("Files".into());
    }
    get("follows")
        .to_lowercase()
        .contains("gpt-")
        .then(|| "Follows a Codex run".into())
}

fn shown(fields: &BTreeMap<String, String>, name: &str) -> String {
    let first = fields
        .get(name)
        .map(|value| value.trim().split('\n').next().unwrap_or("").to_owned())
        .unwrap_or_default();
    if first.is_empty() {
        "missing".into()
    } else {
        first
    }
}

pub fn review_admits(
    fields: &BTreeMap<String, String>,
    family: &str,
    defaults: &Defaults,
) -> (bool, String) {
    match header_word(fields, "reviews", defaults) {
        Some(author) => (
            author != family,
            format!("Reviews {}", shown(fields, "reviews")),
        ),
        None => (false, "Reviews missing".into()),
    }
}

/// The heading the launch puts before the full query form; a brief that the prompt hook grounded
/// carries the same prefix, so `north ledger grounding` counts either.
pub const GROUNDING_PACK: &str = "Grounding (auto-retrieved";

/// The line experiment G2 stamps on a first prompt whose launch drew the plain arm, which carries no pack.
pub const PLAIN_ARM_LINE: &str = "Grounding: plain";

/// The categories whose work changes code; the rest end in a document, a review, a plan or a check.
pub const CODE_CATEGORIES: [&str; 8] = ["mechanical", "tooling", "feature", "bug-known-cause", "debugging-unknown-cause", "netcode-determinism", "performance", "balance-tuning"];
/// The categories whose work changes no code.
pub const NON_CODE_CATEGORIES: [&str; 5] = ["docs-policy", "native-check", "research", "planning", "review"];

/// Whether a brief asks for a code change, from its own `Category:` line; a brief with none counts when
/// it names `Files:` or an `Item:`, since a hand launch may pass a bare task.
pub fn code_change(text: &str) -> bool {
    let fields = brief_header(text);
    let category = fields
        .get("category")
        .into_iter()
        .flat_map(|category| category.split(|c: char| c.is_whitespace() || c == ',' || c == '·'))
        .find(|word| !word.is_empty())
        .map(|word| word.trim_matches(['.', '`']).to_lowercase());
    match category.as_deref() {
        Some(category) if NON_CODE_CATEGORIES.contains(&category) => false,
        Some(category) if CODE_CATEGORIES.contains(&category) => true,
        Some(_) | None => fields.contains_key("files") || fields.contains_key("item"),
    }
}

const BUG_CATEGORIES: [&str; 2] = ["bug-known-cause", "debugging-unknown-cause"];

pub fn class_refusal(text: &str) -> Option<String> {
    let fields = brief_header(text);
    let category = fields.get("category")?.split(|c: char| c.is_whitespace() || c == ',' || c == '·').find(|word| !word.is_empty())?.trim_matches(['.', '`']).to_lowercase();
    if !BUG_CATEGORIES.contains(&category.as_str()) || fields.get("class").is_some_and(|class| !class.trim().is_empty()) {
        return None;
    }
    Some(format!("Category {category} has no Class: line; name the class of failure before the first blank line and write a Done when that covers every instance of it"))
}

/// A launch brief whose Category, Spec, Scope, Surface or Verify line opens with a word outside that facet's list, which the run
/// ledger would drop; every such line is named, so one refusal lists them all.
pub fn facet_refusal(text: &str) -> Option<String> {
    let fields = brief_header(text);
    let facets = [("Category", &CATEGORIES[..])].into_iter().chain(FACETS.iter().copied());
    let bad: Vec<String> = facets
        .filter_map(|(name, values)| {
            let declared = fields.get(&name.to_lowercase())?.trim().trim_matches('`').to_lowercase();
            let word = TOKEN.find(&declared)?.as_str();
            (!values.contains(&word)).then(|| format!("{name}: {word} is not a facet value; use one of {}, so the run ledger records it", values.join(", ")))
        })
        .collect();
    (!bad.is_empty()).then(|| bad.join("\n"))
}

/// Whether `value`, the text of a `name` line, opens with a word in that facet's list; a name with no list admits any value.
pub fn facet_known(name: &str, value: &str) -> bool {
    let Some((_, values)) = FACETS.iter().find(|(facet, _)| facet.eq_ignore_ascii_case(name)) else { return true };
    let lowered = value.trim().trim_matches('`').to_lowercase();
    TOKEN.find(&lowered).is_some_and(|word| values.contains(&word.as_str()))
}

static CITATION: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"docs/references\.md|\bn\s*=\s*\d+").unwrap());

/// A planning box designs from evidence: its Spec line is `measured` with a note citing a ledger
/// measurement (`n=40`) or a docs/references.md entry.
pub fn grounding_refusal(text: &str) -> Option<String> {
    let fields = brief_header(text);
    let category = fields.get("category")?.split(|c: char| c.is_whitespace() || c == ',' || c == '·').find(|word| !word.is_empty())?.trim_matches(['.', '`']).to_lowercase();
    if category != "planning" {
        return None;
    }
    let spec = fields.get("spec").map(|spec| spec.trim().trim_matches('`')).unwrap_or("");
    let grounded = WORD.captures(&spec.to_lowercase()).is_some_and(|captures| &captures[1] == "measured" && captures.get(2).is_some_and(|note| CITATION.is_match(note.as_str())));
    (!grounded).then(|| format!("Category planning has Spec: {}; a design is grounded in evidence, so write `Spec: measured (...)` citing a ledger measurement with its n (n=40) or a docs/references.md entry", if spec.is_empty() { "missing" } else { spec }))
}

pub fn codex_box_refusal(text: &str, defaults: &Defaults) -> Option<String> {
    let fields = brief_header(text);
    if !fields.contains_key("category") {
        return None;
    }
    if header_word(&fields, "category", defaults).as_deref() == Some("review") {
        let (admitted, why) = review_admits(&fields, "codex", defaults);
        return (!admitted).then_some(why);
    }
    let (_, why) = if header_word(&fields, "explore", defaults).as_deref() == Some("codex") {
        codex_explore_qualifies(text, defaults)
    } else {
        codex_qualifies(text, false, defaults)
    };
    why
}

static LANDING_FIELD: LazyLock<Vec<(&str, Regex)>> = LazyLock::new(|| {
    ["Lands-before", "Verify", "Category"]
        .into_iter()
        .map(|name| {
            (
                name,
                Regex::new(&format!(r"(?mi)^[ \t]*[-*]?[ \t]*{name}:[ \t]*`?([A-Za-z0-9-]+)"))
                    .unwrap(),
            )
        })
        .collect()
});

static NOTHING_TO_LAND: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)\b(no landing|nothing to land|investigate only)\b").unwrap());

fn landing_word(text: &str, name: &str) -> Option<String> {
    let (_, pattern) = LANDING_FIELD.iter().find(|(field, _)| *field == name)?;
    pattern
        .captures(text)
        .map(|found| found[1].to_lowercase())
}

/// A box lands before its native capture when it is verified natively and is not itself a
/// `Category: native-check` capture box, which has nothing to land.
pub fn lands_before_native(verify: &str, category: &str) -> bool {
    let first = |value: &str| {
        TOKEN
            .find(&value.to_lowercase())
            .map(|found| found.as_str().to_owned())
    };
    first(verify).as_deref() == Some("native") && first(category).as_deref() != Some("native-check")
}

/// Refuses a brief that would hold its landing for a native capture: a `Verify: native` box lands
/// on its passing sim, headless and Lua32 checks and says so with `Lands-before: native`.
pub fn landing_refusal(text: &str) -> Option<String> {
    if NOTHING_TO_LAND.is_match(text) || matches!(landing_word(text, "Category").as_deref(), Some("review" | "research" | "planning")) {
        return None;
    }
    let word = |name: &str| landing_word(text, name).unwrap_or_default();
    let order = word("Lands-before");
    if !order.is_empty() && order != "native" {
        return Some(format!(
            "Lands-before: {order} is not a landing order; the one value is native, for a box that lands before its native capture."
        ));
    }
    (order.is_empty() && lands_before_native(&word("Verify"), &word("Category"))).then(|| {
        "Verify: native needs a `Lands-before: native` line: the box lands as soon as its sim, headless and Lua32 checks pass, then takes its native capture on main's build and fixes forward; only a release candidate waits on native captures, and a Category: native-check capture box has nothing to land.".into()
    })
}

