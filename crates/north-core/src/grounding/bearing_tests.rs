use super::*;
use std::time::{Duration, Instant};
use store::{Doc, SCHEMA, quote};

pub const ACCESS_PLAN: &str = "~/code/south/main/agents/decisions/harness/gex45-access-plan.md";
const FEEDBACK_LOOP: &str = "~/code/north/main/docs/feedback-loop.md";
const RENAME_PLAN: &str = "~/code/south/main/agents/decisions/smashcraft/makgora-full-rename-plan.md";
const GEX45: &str = "I recommend building the laptop runtime on gex45 and shipping it.";
const CHANGELOG: &str = "I recommend adding a blank line at the end of the changelog.";
const README: &str = "I recommend renaming the README heading.";

fn doc(source: &str, reference: &str, title: &str, text: &str) -> Doc {
    Doc { source: source.into(), id: format!("{reference}#0"), reference: reference.into(), title: title.into(), text: text.into(), at: 0.0, decays: false }
}

fn corpus() -> Vec<Doc> {
    vec![
        doc("policy", ACCESS_PLAN, "south:agents/decisions/harness/gex45-access-plan.md § P3b", "The laptop runtime is never built on gex45 and never shipped from gex45; gex45 serves builds for its own lanes only (P3b, B3, B17)."),
        doc("policy", "~/code/north/main/harness/skills/kayak/SKILL.md", "skill kayak: river trips", "Kayak river trips and paddles."),
        doc("policy", "~/code/north/main/crates/north-core/src/kiln/glaze.rs", "north:crates/north-core/src/kiln/glaze.rs", "fn fire_glaze() fires the glaze kiln at cone six"),
        doc("policy", "~/code/north/main/docs/garden.md", "north:docs/garden.md § Tomatoes", "Water the tomatoes at dawn and stake the vines."),
        doc("threads", "north#401", "north#401 flaky landing test", "The landing test is flaky; a seeded retry would fix it."),
        doc("policy", FEEDBACK_LOOP, "north:docs/feedback-loop.md § Signals", "Each signal row ends with a blank line, and its reader takes one line per row to the end of the file; we recommend adding nothing between rows."),
        doc("policy", RENAME_PLAN, "south:agents/decisions/smashcraft/makgora-full-rename-plan.md § Order", "The full rename plan: renaming every README heading and title from Smashcraft to Makgora, and we recommend the README heading lands first."),
        doc("threads", "north#77", "north#77 release changelog", "The release changelog lists each landed box under its version."),
        doc("policy", "~/code/north/main/docs/release.md", "north:docs/release.md § Versions", "A release tags the version and publishes the binary for each platform; we recommend tagging on a quiet day."),
        doc("policy", "~/code/north/main/docs/music.md", "north:docs/music.md § Scales", "Practice the minor scales before the chords."),
    ]
}

pub fn index(dir: &Path) -> PathBuf {
    let path = dir.join("index.db");
    let mut db = Sqlite::open(&path, false).unwrap();
    let mut sql = format!("{SCHEMA}\nBEGIN;\n");
    for d in corpus() {
        sql.push_str(&format!(
            "insert into docs(source,id,ref,title,text,at,decays) values ({},{},{},{},{},{},{});\n",
            quote(&d.source),
            quote(&d.id),
            quote(&d.reference),
            quote(&d.title),
            quote(&d.text),
            d.at,
            usize::from(d.decays)
        ));
    }
    sql.push_str("COMMIT;");
    db.exec(&sql).unwrap();
    path
}

#[test]
fn a_recommendation_whose_terms_hit_a_decision_record_names_it_and_nothing_else() {
    let dir = tempfile::tempdir().unwrap();
    let index = index(dir.path());
    let named = records_at("I recommend building the laptop runtime on gex45 and shipping it from there.", &index, 0.0);
    assert_eq!(named, vec![shown(ACCESS_PLAN)]);
}

#[test]
fn a_recommendation_with_no_tier_a_record_names_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let index = index(dir.path());
    assert!(records_at("I recommend paddling the kayak down the river.", &index, 0.0).is_empty());
    assert!(records_at("The fix is a seeded retry in the flaky landing test.", &index, 0.0).is_empty());
    assert!(records_at("I recommend building the laptop runtime on gex45.", &dir.path().join("absent.db"), 0.0).is_empty());
}

#[test]
fn skills_and_code_never_count_as_recorded_decisions() {
    let dir = tempfile::tempdir().unwrap();
    let index = index(dir.path());
    assert!(records_at("Recommended: paddle the kayak river trips.", &index, 0.0).is_empty());
    assert!(records_at("The fix is to fire the glaze kiln at cone six.", &index, 0.0).is_empty());
}

#[test]
fn the_retrieval_stays_under_300_ms_on_a_warm_index() {
    let dir = tempfile::tempdir().unwrap();
    let index = index(dir.path());
    let paragraph = "I recommend building the laptop runtime on gex45 and shipping it from there.";
    records_at(paragraph, &index, 0.0);
    let started = Instant::now();
    let named = records_at(paragraph, &index, 0.0);
    let took = started.elapsed();
    assert_eq!(named, vec![shown(ACCESS_PLAN)]);
    assert!(took < Duration::from_millis(300), "{took:?}");
}


fn tier_a_policy(paragraph: &str, index: &Path) -> Vec<String> {
    let mut db = Sqlite::open(index, true).unwrap();
    let (hits, _) = retrieve_db(paragraph, &mut db, 0.0, 80, 0.0, "").unwrap();
    hits.iter().filter(|hit| hit.cover >= HOOK_FLOOR && category(hit) == "Policy" && !code(hit)).map(|hit| hit.reference.clone()).collect()
}

#[test]
fn only_a_tier_a_hit_sharing_two_subject_terms_bears_on_a_recommendation() {
    let dir = tempfile::tempdir().unwrap();
    let index = index(dir.path());
    assert!(tier_a_policy(CHANGELOG, &index).contains(&FEEDBACK_LOOP.to_owned()), "the fixture reproduces the live over-fire on edit words");
    assert!(tier_a_policy(README, &index).contains(&RENAME_PLAN.to_owned()), "the fixture reproduces the live over-fire on edit words");
    assert!(records_at(CHANGELOG, &index, 0.0).is_empty());
    assert!(records_at(README, &index, 0.0).is_empty());
    assert_eq!(records_at(GEX45, &index, 0.0), vec![shown(ACCESS_PLAN)]);
}

#[test]
fn the_recommendation_words_themselves_are_not_a_subject_term() {
    let dir = tempfile::tempdir().unwrap();
    let index = index(dir.path());
    let paragraph = "I recommend cutting a release.";
    assert!(tier_a_policy(paragraph, &index).contains(&"~/code/north/main/docs/release.md".to_owned()));
    assert!(records_at(paragraph, &index, 0.0).is_empty());
}
