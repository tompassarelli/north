//! Skill effect over the tick's two weeks: per skill, the closed-box rate and cost per closed box of
//! the boxes whose runs loaded it against the boxes of the same categories whose runs did not, each
//! category weighted by the skill's boxes in it so a skill used mostly on hard categories is not
//! blamed for them. Flags each installed skill no run loaded and each whose boxes did worse.
use crate::ledger::ClosedReport;
use crate::ledger::closed::SkillBox;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

/// Boxes each side needs before a skill's outcome can flag it, so one bad box flags nothing.
pub const SKILL_MIN_BOXES: usize = 3;
/// How far under the matched boxes' closed-box rate a skill's boxes must fall to flag it.
pub const WORSE_RATE: f64 = 0.2;
/// How much dearer than the matched boxes a skill's closed box must be to flag it.
pub const DEARER: f64 = 0.25;

#[derive(Default)]
struct Side {
    boxes: f64,
    closed: f64,
    usd: f64,
}

impl Side {
    fn add(&mut self, boxes: &[&SkillBox], weight: f64) {
        self.boxes += boxes.len() as f64 * weight;
        self.closed += boxes.iter().filter(|spent| spent.closed).count() as f64 * weight;
        self.usd += boxes.iter().map(|spent| spent.usd).sum::<f64>() * weight;
    }

    fn rate(&self) -> f64 {
        self.closed / self.boxes
    }

    fn per_closed(&self) -> Option<f64> {
        (self.closed > 0.0).then(|| self.usd / self.closed)
    }

    fn figures(&self) -> String {
        let cost = self.per_closed().map_or_else(|| "-".to_owned(), |usd| format!("${usd:.2}"));
        format!("closed {:.0}% at {cost}/closed", self.rate() * 100.0)
    }
}

struct Effect {
    skill: String,
    runs: usize,
    boxes: usize,
    with: Side,
    without: Side,
    matched: usize,
}

impl Effect {
    fn of(skill: &str, runs: usize, boxes: &[&SkillBox]) -> Self {
        let mut by_category: BTreeMap<&str, (Vec<&SkillBox>, Vec<&SkillBox>)> = BTreeMap::new();
        for spent in boxes {
            let sides = by_category.entry(spent.category.as_str()).or_default();
            if spent.skills.contains(skill) { sides.0.push(spent) } else { sides.1.push(spent) }
        }
        let mut effect = Self { skill: skill.to_owned(), runs, boxes: 0, with: Side::default(), without: Side::default(), matched: 0 };
        for (with, without) in by_category.values() {
            effect.boxes += with.len();
            if with.is_empty() || without.is_empty() {
                continue;
            }
            effect.with.add(with, 1.0);
            effect.without.add(without, with.len() as f64 / without.len() as f64);
            effect.matched += without.len();
        }
        effect
    }

    fn line(&self) -> String {
        let head = format!("skill {}: {} runs, {} boxes", self.skill, self.runs, self.boxes);
        if self.matched == 0 {
            return format!("{head}, no matched boxes");
        }
        format!("{head}, {} vs {} on {} matched boxes", self.with.figures(), self.without.figures(), self.matched)
    }

    fn harmful(&self) -> bool {
        let counted = self.with.boxes >= SKILL_MIN_BOXES as f64 && self.matched >= SKILL_MIN_BOXES;
        let dearer = matches!((self.with.per_closed(), self.without.per_closed()), (Some(with), Some(without)) if with > without * (1.0 + DEARER));
        counted && (self.with.rate() < self.without.rate() - WORSE_RATE || dearer)
    }
}

/// The skill table and its flags over `week` and `before`; a box both reports hold counts as this week's.
pub fn lines(week: &ClosedReport, before: &ClosedReport, installed: &[String]) -> Vec<String> {
    let seen: BTreeSet<&str> = week.boxes.iter().map(|spent| spent.item.as_str()).collect();
    let boxes: Vec<&SkillBox> = week.boxes.iter().chain(before.boxes.iter().filter(|spent| !seen.contains(spent.item.as_str()))).collect();
    let mut runs: BTreeMap<&str, usize> = BTreeMap::new();
    for (skill, count) in week.loaded.iter().chain(&before.loaded) {
        *runs.entry(skill.as_str()).or_default() += count;
    }
    let loaded: BTreeSet<&str> = runs.keys().copied().chain(boxes.iter().flat_map(|spent| spent.skills.iter().map(String::as_str))).collect();
    if loaded.is_empty() && installed.is_empty() {
        return Vec::new();
    }
    let effects: Vec<Effect> = loaded.iter().map(|skill| Effect::of(skill, runs.get(skill).copied().unwrap_or(0), &boxes)).collect();
    let mut lines = vec![format!("skills over 14 days: {} loaded, {} installed", loaded.len(), installed.len())];
    lines.extend(effects.iter().map(Effect::line));
    lines.extend(installed.iter().filter(|skill| !loaded.contains(skill.as_str())).map(|skill| format!("unused skill {skill}: no run loaded it in 14 days")));
    lines.extend(effects.iter().filter(|effect| effect.harmful()).map(|effect| format!("harmful skill {}: {} vs {} on matched boxes", effect.skill, effect.with.figures(), effect.without.figures())));
    lines
}

/// The skills agents can load: each directory under `root` that holds a SKILL.md, by name.
pub fn installed(root: &Path) -> Result<Vec<String>, String> {
    let entries = std::fs::read_dir(root).map_err(|error| format!("{}: {error}", root.display()))?;
    let mut names: Vec<String> = entries.flatten().filter(|entry| entry.path().join("SKILL.md").is_file()).map(|entry| entry.file_name().to_string_lossy().into_owned()).collect();
    names.sort();
    Ok(names)
}
