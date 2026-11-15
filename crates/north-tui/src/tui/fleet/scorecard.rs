/// The three readings of the fleet screen's top line; a reading that could not be read stays `None` and the line says so.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Scorecard {
    pub landings: Option<usize>,
    pub first_try: Option<FirstTry>,
    pub comparison: Option<Comparison>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FirstTry {
    pub landed: usize,
    pub started: usize,
}

/// North's time and cost to land a task as a multiple of `rival`'s, below 1 where north is ahead.
#[derive(Clone, Debug, PartialEq)]
pub struct Comparison {
    pub rival: String,
    pub time: f64,
    pub cost: f64,
}

impl Scorecard {
    pub fn line(&self) -> String {
        let landings = self.landings.map_or_else(|| "landings unread".to_owned(), |landings| format!("{landings} landed today"));
        let first_try = match self.first_try {
            Some(FirstTry { started: 0, .. }) => "first try: no worker started today".to_owned(),
            Some(FirstTry { landed, started }) => format!("first try {:.0}% ({landed}/{started})", 100.0 * landed as f64 / started as f64),
            None => "first try unread".to_owned(),
        };
        let comparison = self.comparison.as_ref().map_or_else(|| "comparison unread".to_owned(), |c| format!("vs {} time {:.2}x, cost {:.2}x", c.rival, c.time, c.cost));
        format!("{landings} · {first_try} · {comparison}")
    }
}
