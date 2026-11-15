use north_pace::router::brief::CATEGORIES;

/// The brief of the Haiku pass that completes an issue its filing left without a type label, category, box or command.
pub(super) fn intake_brief(repo: &str, number: u64, gaps: &[String]) -> String {
    let categories = CATEGORIES.join(", ");
    format!(
        "Item: {repo}#{number} intake\n\
         Category: mechanical\n\
         Spec: exact\n\
         Scope: one-file\n\
         Surface: workflow\n\
         Verify: local-test\n\
         Tier-reason: writing labels and boxes onto one existing issue is a mechanical edit (north#1632)\n\
         Done when: `gh issue view {number} -R {repo} --json body -q .body | grep -q '^Category:' && gh issue view {number} -R {repo} --json labels -q '.labels[].name' | grep -qxE 'bug|enhancement|documentation'`\n\
         \n\
         The filing of {repo}#{number} left out: {gaps}. Read the issue with `gh issue view {number} -R {repo} --json title,body,labels`, then make only these changes:\n\
         - add the type label that fits its title and body (bug, enhancement or documentation) with `gh issue edit {number} -R {repo} --add-label LABEL`;\n\
         - add a `priority:*` label from `gh label list -R {repo}` with `--add-label`, the one its body supports;\n\
         - add a `Category: NAME` line on its own before the first blank line, NAME one of: {categories};\n\
         - for a bug-known-cause or debugging-unknown-cause category, add a `Class:` line naming the class of failure;\n\
         - end each unticked `- [ ]` box that names no command with a Done when: followed by one backticked command whose exit 0 shows the box done; a box that waits on Tom says `Needs: Tom`.\n\
         Write the new body to a file in your scratch directory and run `gh issue edit {number} -R {repo} --body-file FILE`. Keep every existing word of the title and body. Do not file, close or comment on any issue. When the Done when check passes, stop and report.",
        gaps = gaps.join("; ")
    )
}
