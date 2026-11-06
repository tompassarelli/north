use crate::shell::git::Git;

#[derive(Debug, Eq, PartialEq)]
pub struct Publication {
    pub refspec: String,
    pub lease: String,
}

pub fn source(current: &str, default: &str) -> Result<(), String> {
    if current == default {
        return Ok(());
    }
    Err(format!(
        "safe-push: non-main branch {current}: publish with safe-push --to main (one-branch-main doctrine)\n\
         safe-push: pushing branch names to origin is disabled\n"
    ))
}

pub fn one_branch_main(git: &dyn Git) -> Result<(), String> {
    let Some(head) = git.symbolic_ref("HEAD") else { return Ok(()) };
    let Some(current) = head.strip_prefix("refs/heads/") else { return Ok(()) };
    source(current, &git.default_branch())
}

pub fn publication(head_oid: &str, destination_ref: &str, destination_oid: &str) -> Publication {
    Publication {
        refspec: format!("{head_oid}:{destination_ref}"),
        lease: format!("--force-with-lease={destination_ref}:{destination_oid}"),
    }
}
