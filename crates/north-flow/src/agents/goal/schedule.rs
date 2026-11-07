use std::collections::{BTreeSet, HashMap};

pub struct Task {
    pub key: String,
    pub eta: f64,
    pub deps: BTreeSet<String>,
}

fn order(left: f64, right: f64) -> std::cmp::Ordering {
    left.partial_cmp(&right).unwrap_or(std::cmp::Ordering::Equal)
}

fn tail(at: usize, duration: &[f64], children: &[Vec<usize>], tasks: &[Task], tails: &mut [Option<f64>], visiting: &mut [bool]) -> Result<f64, String> {
    if let Some(known) = tails[at] {
        return Ok(known);
    }
    if visiting[at] {
        return Err(format!("dependency cycle at {}", tasks[at].key));
    }
    visiting[at] = true;
    let mut longest = 0.0_f64;
    for child in &children[at] {
        longest = longest.max(tail(*child, duration, children, tasks, tails, visiting)?);
    }
    tails[at] = Some(duration[at] + longest);
    Ok(duration[at] + longest)
}

/// Minutes to finish every task: list scheduling at `width` parallel workers, longest remaining path first, each task taking its ETA
/// plus the landing overhead and starting only after its dependencies land.
pub fn schedule(tasks: &[Task], width: usize, landing: f64) -> Result<f64, String> {
    if width < 1 {
        return Err("width must be at least 1".into());
    }
    let duration: Vec<f64> = tasks.iter().map(|task| task.eta + landing).collect();
    let children: Vec<Vec<usize>> = tasks.iter().map(|parent| (0..tasks.len()).filter(|child| tasks[*child].deps.contains(&parent.key)).collect()).collect();
    let position: HashMap<&str, usize> = tasks.iter().enumerate().map(|(at, task)| (task.key.as_str(), at)).collect();
    let (mut tails, mut visiting) = (vec![None; tasks.len()], vec![false; tasks.len()]);
    for at in 0..tasks.len() {
        tail(at, &duration, &children, tasks, &mut tails, &mut visiting)?;
    }
    let tails: Vec<f64> = tails.into_iter().map(|known| known.unwrap_or_default()).collect();
    let (mut clock, mut done, mut running) = (0.0_f64, vec![false; tasks.len()], Vec::<(f64, usize)>::new());
    let mut pending: BTreeSet<usize> = (0..tasks.len()).collect();
    while !pending.is_empty() || !running.is_empty() {
        let mut ready: Vec<usize> = pending.iter().copied().filter(|at| tasks[*at].deps.iter().all(|dep| position.get(dep.as_str()).is_none_or(|found| done[*found]))).collect();
        ready.sort_by(|left, right| order(tails[*right], tails[*left]).then_with(|| tasks[*left].key.cmp(&tasks[*right].key)));
        for at in ready.into_iter().take(width.saturating_sub(running.len())) {
            pending.remove(&at);
            running.push((clock + duration[at], at));
        }
        running.sort_by(|left, right| order(left.0, right.0).then_with(|| tasks[left.1].key.cmp(&tasks[right.1].key)));
        let Some(first) = running.first() else { return Err("nothing can start".into()) };
        clock = first.0;
        while running.first().is_some_and(|next| next.0 == clock) {
            done[running.remove(0).1] = true;
        }
    }
    Ok(clock)
}

pub fn serial(tasks: &[Task], landing: f64) -> f64 {
    tasks.iter().map(|task| task.eta + landing).sum()
}
