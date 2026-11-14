use crate::error::NorthResult;
use crate::fleet::remote::{self, HeldOn};
use crate::fleet::runtime::binary;

pub trait HeldUnits: Send + Sync {
    fn retire(&self, held: &HeldOn) -> NorthResult<()>;
}

pub struct OverSshHeld;

impl HeldUnits for OverSshHeld {
    fn retire(&self, held: &HeldOn) -> NorthResult<()> {
        remote::retire(&binary("ssh"), held)
    }
}
