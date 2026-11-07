#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Delivery {
    Queued,
    Now,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Status {
    Starting,
    Working,
    Idle,
    Parked,
    Exited,
}

impl Status {
    pub fn label(self) -> &'static str {
        match self {
            Status::Starting => "starting",
            Status::Working => "working",
            Status::Idle => "idle",
            Status::Parked => "parked",
            Status::Exited => "exited",
        }
    }
}
