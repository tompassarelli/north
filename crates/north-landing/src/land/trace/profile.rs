use std::collections::{BTreeMap, HashMap, HashSet};

use super::{AFTER, Category, PHASE, Span};

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Split {
    pub categories: BTreeMap<Category, f64>,
    pub crates: BTreeMap<String, f64>,
}

impl Split {
    pub fn total(&self) -> f64 {
        self.categories.values().sum()
    }

    pub fn get(&self, category: Category) -> f64 {
        self.categories.get(&category).copied().unwrap_or_default()
    }

    fn add(&mut self, span: &Span, seconds: f64) {
        let category = if span.category == Category::Landing { Category::Lander } else { span.category };
        *self.categories.entry(category).or_default() += seconds;
        if category == Category::Test
            && let Some(name) = span.attribute("crate")
        {
            *self.crates.entry(name.to_owned()).or_default() += seconds;
        }
    }
}

#[derive(Clone, Debug)]
pub struct Profile<'a> {
    pub landing: &'a Span,
    pub before: Split,
    pub after: Split,
    pub after_wall: f64,
    pub spans: Vec<&'a Span>,
}

impl Profile<'_> {
    pub fn wall(&self) -> f64 {
        self.landing.seconds()
    }
}

pub struct Trace<'a> {
    spans: Vec<&'a Span>,
    children: HashMap<&'a str, Vec<&'a Span>>,
}

impl<'a> Trace<'a> {
    pub fn of(spans: &'a [Span]) -> Self {
        let mut seen = HashSet::new();
        let spans: Vec<&Span> = spans.iter().filter(|span| seen.insert(span.id.as_str())).collect();
        let mut children: HashMap<&str, Vec<&Span>> = HashMap::new();
        for span in &spans {
            if let Some(parent) = &span.parent {
                children.entry(parent.as_str()).or_default().push(span);
            }
        }
        Self { spans, children }
    }

    pub fn landings(&self, since: f64) -> Vec<&'a Span> {
        let mut landings: Vec<&Span> = self.spans.iter().copied().filter(|span| span.category == Category::Landing && span.end >= since).collect();
        landings.sort_by(|left, right| left.end.total_cmp(&right.end));
        landings
    }

    fn descend(&self, span: &'a Span, window: (f64, f64), into: &mut Vec<&'a Span>) {
        for child in self.children.get(span.id.as_str()).into_iter().flatten() {
            if child.start >= window.0 - 1.0 && child.start <= window.1 + 1.0 {
                into.push(child);
                self.descend(child, window, into);
            }
        }
    }

    pub fn profile(&self, landing: &'a Span) -> Profile<'a> {
        let window = (landing.start, landing.end);
        let mut before = vec![landing];
        self.descend(landing, window, &mut before);
        for shared in self.spans.iter().copied().filter(|span| span.trace == landing.trace && span.parent.is_none() && span.category != Category::Landing && span.attribute(PHASE) != Some(AFTER)) {
            if shared.start >= window.0 - 1.0 && shared.end <= window.1 + 1.0 {
                before.push(shared);
                self.descend(shared, window, &mut before);
            }
        }
        let landed = landing.attribute("landed").unwrap_or_default();
        let mut after = Vec::new();
        for root in self.spans.iter().copied().filter(|span| span.trace == landed && span.attribute(PHASE) == Some(AFTER) && span.start >= window.1 - 1.0) {
            after.push(root);
            self.descend(root, (root.start, root.end), &mut after);
        }
        let after_wall = union(&after);
        let before_split = attribute(&before, Some(landing), window);
        let after_split = attribute(&after, None, (after.iter().map(|span| span.start).fold(f64::INFINITY, f64::min), after.iter().map(|span| span.end).fold(f64::NEG_INFINITY, f64::max)));
        before.extend(after);
        Profile { landing, before: before_split, after: after_split, after_wall, spans: before }
    }
}

fn union(spans: &[&Span]) -> f64 {
    let mut intervals: Vec<(f64, f64)> = spans.iter().map(|span| (span.start, span.end)).collect();
    intervals.sort_by(|left, right| left.0.total_cmp(&right.0));
    let mut total = 0.0;
    let mut current: Option<(f64, f64)> = None;
    for (start, end) in intervals {
        current = match current {
            Some((from, to)) if start <= to => Some((from, to.max(end))),
            Some((from, to)) => {
                total += to - from;
                Some((start, end))
            }
            None => Some((start, end)),
        };
    }
    total + current.map_or(0.0, |(from, to)| to - from)
}

fn attribute(members: &[&Span], root: Option<&Span>, window: (f64, f64)) -> Split {
    let mut split = Split::default();
    if members.is_empty() || window.1 <= window.0 {
        return split;
    }
    let ids: HashSet<&str> = members.iter().map(|span| span.id.as_str()).collect();
    let parent_of = |span: &Span| -> Option<String> {
        match &span.parent {
            Some(parent) if ids.contains(parent.as_str()) => Some(parent.clone()),
            _ if root.is_some_and(|root| root.id != span.id) => root.map(|root| root.id.clone()),
            _ => None,
        }
    };
    let parents: Vec<Option<String>> = members.iter().map(|span| parent_of(span)).collect();
    let mut cuts: Vec<f64> = members.iter().flat_map(|span| [span.start, span.end]).chain([window.0, window.1]).map(|at| at.clamp(window.0, window.1)).collect();
    cuts.sort_by(f64::total_cmp);
    cuts.dedup();
    for pair in cuts.windows(2) {
        let (from, to) = (pair[0], pair[1]);
        let middle = f64::midpoint(from, to);
        let active: Vec<usize> = (0..members.len()).filter(|&index| members[index].start <= middle && middle < members[index].end).collect();
        let busy: HashSet<&str> = active.iter().filter_map(|&index| parents[index].as_deref()).collect();
        let leaves: Vec<usize> = active.iter().copied().filter(|&index| !busy.contains(members[index].id.as_str())).collect();
        if leaves.is_empty() {
            if let Some(root) = root {
                split.add(root, to - from);
            }
            continue;
        }
        #[allow(clippy::cast_precision_loss)]
        let share = (to - from) / leaves.len() as f64;
        for index in leaves {
            split.add(members[index], share);
        }
    }
    split
}

pub fn percentile(values: &[f64], fraction: f64) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    let mut sorted = values.to_vec();
    sorted.sort_by(f64::total_cmp);
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss, clippy::cast_precision_loss)]
    let rank = ((sorted.len() as f64 * fraction).ceil() as usize).clamp(1, sorted.len());
    sorted[rank - 1]
}
