/// The cases one run executes: `only` names the cases to run (none means all), and `skip` leaves named cases to another run.
/// A name that no case of the binary has is an error, so a renamed case cannot pass by running nothing.
pub fn select(listed: &[String], only: &[String], skip: &[String]) -> Result<Vec<String>, String> {
    only.iter().chain(skip).find(|name| !listed.contains(name)).map_or(Ok(()), |name| Err(format!("no case named {name} in this binary")))?;
    let wanted = |name: &&String| only.is_empty() || only.contains(name);
    Ok(listed.iter().filter(wanted).filter(|name| !skip.contains(name)).cloned().collect())
}
