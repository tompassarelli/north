#[derive(Debug)]
pub struct Die {
    pub code: u8,
    pub message: String,
}

impl Die {
    pub(super) fn new(code: u8, message: impl Into<String>) -> Self {
        Self { code, message: message.into() }
    }

    pub(super) fn silent(code: u8) -> Self {
        Self::new(code, "")
    }
}
