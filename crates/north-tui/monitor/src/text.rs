const BARS: [char; 8] = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];

pub fn sparkline(values: &[u32]) -> String {
    let floats: Vec<f64> = values.iter().map(|value| f64::from(*value)).collect();
    sparkline_f(&floats)
}

pub fn sparkline_f(values: &[f64]) -> String {
    if values.is_empty() {
        return "(no samples yet)".into();
    }
    let top = values.iter().copied().fold(0.0, f64::max);
    values
        .iter()
        .map(|value| if top <= 0.0 { BARS[0] } else { BARS[((value / top) * 7.0).round().clamp(0.0, 7.0) as usize] })
        .collect()
}

pub fn duration(seconds: u64) -> String {
    match seconds {
        0..60 => format!("{seconds}s"),
        60..3600 => format!("{}m{:02}s", seconds / 60, seconds % 60),
        3600..86_400 => format!("{}h{:02}m", seconds / 3600, (seconds % 3600) / 60),
        _ => format!("{}d{:02}h", seconds / 86_400, (seconds % 86_400) / 3600),
    }
}

pub fn clock(at: u64, offset_s: i32) -> String {
    let local = (at as i64 + i64::from(offset_s)).rem_euclid(86_400);
    let (hour, minute, second) = (local / 3600, (local % 3600) / 60, local % 60);
    let twelve = if hour % 12 == 0 { 12 } else { hour % 12 };
    format!("{twelve}:{minute:02}:{second:02} {}", if hour < 12 { "AM" } else { "PM" })
}
