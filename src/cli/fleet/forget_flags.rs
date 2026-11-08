/// The `--force REASON` and `--to SUCCESSOR` of `north fleet forget NAME`, each at most once, in either order.
pub(super) fn parse<'a>(flags: &[&'a str]) -> Option<(Option<&'a str>, Option<&'a str>)> {
    let (mut force, mut successor) = (None, None);
    for pair in flags.chunks(2) {
        let [flag, value] = pair else { return None };
        let slot = match *flag {
            "--force" => &mut force,
            "--to" => &mut successor,
            _ => return None,
        };
        if slot.replace(*value).is_some() {
            return None;
        }
    }
    Some((force, successor))
}
