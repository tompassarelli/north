pub mod infra;

pub struct Snapshot {
    pub infra: Option<infra::Infra>,
}

impl Snapshot {
    pub fn new() -> Self {
        Self { infra: None }
    }

    pub fn infra(mut self) -> Result<Self, String> {
        self.infra = Some(infra::read()?);
        Ok(self)
    }
}
