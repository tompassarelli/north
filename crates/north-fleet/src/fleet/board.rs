//! The board's snapshot: one line for the fleet's throughput and four panels, one per concern, in the order the
//! operator views name them. Every layer prints this projection and computes no fact of its own.

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Board {
    /// Landings and closed boxes per hour, the first line every board prints.
    pub first_line: String,
    pub panels: Vec<Panel>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Panel {
    pub title: &'static str,
    /// The one fact the panel opens on.
    pub headline: String,
    pub lines: Vec<String>,
    pub unread: Vec<String>,
}

impl Board {
    pub fn panel(&self, title: &str) -> Option<&Panel> {
        self.panels.iter().find(|panel| panel.title == title)
    }

    pub fn unread(why: impl Into<String>) -> Self {
        Self {
            first_line: "the board could not be read".to_owned(),
            panels: vec![Panel { title: "Work", headline: "the board could not be read".to_owned(), lines: Vec::new(), unread: vec![why.into()] }],
        }
    }
}

impl Panel {
    pub fn new(title: &'static str, headline: impl Into<String>) -> Self {
        Self { title, headline: headline.into(), lines: Vec::new(), unread: Vec::new() }
    }
}
