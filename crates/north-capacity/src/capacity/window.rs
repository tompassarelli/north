pub fn parse_window_seconds(text: &str) -> Option<f64> {
    let split = text.find(|character: char| !character.is_ascii_digit() && character != '.')?;
    let (amount, unit) = text.split_at(split);
    let amount: f64 = amount.parse().ok()?;
    let scale = match unit {
        "s" => 1.0,
        "m" => 60.0,
        "h" => 3600.0,
        "d" => 86_400.0,
        _ => return None,
    };
    (amount > 0.0).then_some(amount * scale)
}
