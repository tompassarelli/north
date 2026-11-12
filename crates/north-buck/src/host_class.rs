use crate::env;
use crate::rule::{rule, Cases, Rule, RuleKind};
use crate::sources::Tree;
use crate::workspace::{DepKind, Kind, Package, Target};

/// `{name}-unittest` runs every case; a library that names host cases also gets `{name}-unittest-host`, which runs just those on the host.
pub fn unittest_rules(name: &str, binary_id: String, package: &Package, target: &Target, tree: &Tree, context: &env::Context, own_lib: Option<&Package>) -> Result<Vec<Rule>, String> {
    let kinds = [DepKind::Normal, DepKind::Dev];
    let host: Vec<String> = if target.kind == Kind::Lib { package.host_cases.iter().map(|case| case.name.clone()).collect() } else { Vec::new() };
    let mut unittest = rule(RuleKind::Test, format!("{name}-unittest"), Some(binary_id.clone()), tree, context, own_lib, &kinds)?;
    if host.is_empty() {
        return Ok(vec![unittest]);
    }
    unittest.cases.skip.clone_from(&host);
    let class = rule(RuleKind::Test, format!("{name}-unittest-host"), Some(binary_id), tree, context, own_lib, &kinds)?;
    Ok(vec![unittest, Rule { cases: Cases { only: host, skip: Vec::new(), host: true }, ..class }])
}
