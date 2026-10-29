//! Python-compatible figures: the float `sum` and the Wilson interval.

/// Python's `sum` over floats: Neumaier-compensated since 3.12.
pub(crate) fn float_sum(values: impl IntoIterator<Item = f64>) -> f64 {
    let (mut total, mut carry) = (0.0f64, 0.0f64);
    for x in values {
        let t = total + x;
        carry += if total.abs() >= x.abs() { (total - t) + x } else { (x - t) + total };
        total = t;
    }
    if carry != 0.0 && carry.is_finite() { total + carry } else { total }
}

const Z95: f64 = 1.959964;

pub(crate) fn wilson(conformance: f64, n: usize) -> (f64, f64) {
    if n == 0 {
        return (0.0, 1.0);
    }
    let n = n as f64;
    let p = (conformance * n).round() / n;
    let centre = (p + Z95 * Z95 / (2.0 * n)) / (1.0 + Z95 * Z95 / n);
    let half = Z95 * (p * (1.0 - p) / n + Z95 * Z95 / (4.0 * n * n)).sqrt() / (1.0 + Z95 * Z95 / n);
    ((centre - half).max(0.0), (centre + half).min(1.0))
}
