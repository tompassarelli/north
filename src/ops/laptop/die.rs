#[derive(Debug)]
pub(crate) struct Die {
    pub(crate) code: u8,
    pub(crate) message: String,
}

impl Die {
    pub(super) fn new(code: u8, message: impl Into<String>) -> Self {
        Self { code, message: message.into() }
    }

    pub(super) fn silent(code: u8) -> Self {
        Self::new(code, "")
    }
}
