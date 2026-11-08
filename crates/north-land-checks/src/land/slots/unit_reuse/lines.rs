pub const MARK: &str = "north-reuse";

#[derive(Clone, Debug, PartialEq)]
pub enum Record {
    Unit { probe: String, unit: String, set: Option<String>, passed: bool, seconds: f64 },
    SetDiffers { probe: String, unit: String, reused: String, real: String, path: String },
}

impl Record {
    pub fn render(&self) -> String {
        match self {
            Self::Unit { probe, unit, set, passed, seconds } => format!("unit\t{probe}\t{unit}\t{}\t{}\t{seconds:.3}", set.as_deref().unwrap_or("-"), if *passed { "pass" } else { "fail" }),
            Self::SetDiffers { probe, unit, reused, real, path } => format!("set-differs\t{probe}\t{unit}\t{reused}\t{real}\t{path}"),
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        let fields: Vec<&str> = text.split('\t').collect();
        match fields.as_slice() {
            ["unit", probe, unit, set, verdict, seconds] => Some(Self::Unit { probe: (*probe).to_owned(), unit: (*unit).to_owned(), set: (*set != "-").then(|| (*set).to_owned()), passed: *verdict == "pass", seconds: seconds.parse().ok()? }),
            ["set-differs", probe, unit, reused, real, path] => Some(Self::SetDiffers { probe: (*probe).to_owned(), unit: (*unit).to_owned(), reused: (*reused).to_owned(), real: (*real).to_owned(), path: (*path).to_owned() }),
            _ => None,
        }
    }

    pub fn marked(&self) -> String {
        format!("{MARK}\t{}", self.render())
    }
}
