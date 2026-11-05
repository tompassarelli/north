use super::checks::Verdict;

pub fn culprit(lanes: usize, mut after_first: impl FnMut(usize) -> Verdict) -> Option<usize> {
    let (mut green, mut red) = (0, lanes);
    while red - green > 1 {
        let middle = green + (red - green) / 2;
        match after_first(middle) {
            Verdict::RedMain(_) => red = middle,
            Verdict::Green => green = middle,
            _ => return None,
        }
    }
    (lanes > 0).then_some(red - 1)
}

#[cfg(test)]
#[path = "blame_tests.rs"]
mod tests;
