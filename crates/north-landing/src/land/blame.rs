use super::checks::Verdict;

pub fn culprit(lanes: usize, room: usize, mut round: impl FnMut(&[usize]) -> Vec<Verdict>) -> Option<usize> {
    if lanes == 0 {
        return None;
    }
    if room >= lanes
        && let Some(found) = at_once(lanes, &round(&(0..lanes).collect::<Vec<_>>()))
    {
        return found;
    }
    let red = bisected(lanes, &mut round)?;
    if red == 1 && matches!(round(&[0]).pop(), Some(Verdict::RedMain(_))) {
        return None;
    }
    Some(red - 1)
}

fn at_once(lanes: usize, verdicts: &[Verdict]) -> Option<Option<usize>> {
    if matches!(verdicts.first(), Some(Verdict::RedMain(_))) {
        return Some(None);
    }
    for (count, verdict) in verdicts.iter().enumerate().skip(1) {
        match verdict {
            Verdict::Green => {}
            Verdict::RedMain(_) => return Some(Some(count - 1)),
            _ => return None,
        }
    }
    Some(Some(lanes - 1))
}

fn bisected(lanes: usize, round: &mut impl FnMut(&[usize]) -> Vec<Verdict>) -> Option<usize> {
    let (mut green, mut red) = (0, lanes);
    while red - green > 1 {
        let middle = green + (red - green) / 2;
        match round(&[middle]).pop() {
            Some(Verdict::RedMain(_)) => red = middle,
            Some(Verdict::Green) => green = middle,
            _ => return None,
        }
    }
    Some(red)
}

#[cfg(test)]
#[path = "blame_tests.rs"]
mod tests;
