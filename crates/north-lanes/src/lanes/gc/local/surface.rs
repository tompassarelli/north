use super::super::Gc;
use super::super::state::Owned;
use super::Container;

impl Gc {
    pub(super) fn surface(&mut self, sweep: &mut Container, place: &str, branch: &str, age_h: i64, refs: &[String]) -> Result<String, String> {
        let owner = self.owner(&sweep.name)?;
        sweep.owned.push(Owned { place: place.into(), branch: branch.into(), age_h, refs: refs.to_vec(), owner: owner.clone() });
        Ok(owner)
    }
}
