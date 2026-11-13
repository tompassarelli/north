use super::hands_over_a_command;

#[test]
fn the_proxy_replies_that_handed_tom_rm_commands_on_2026_11_08_are_refused_and_plain_reports_pass_north_937() {
    let handed = [
        "Type this in my prompt to delete that one 41-byte file:\n\n```\n! rm /home/tom/code/operator/main/.git/refs/remotes/origin/main.lock\n```\n\nOnce it's gone, the next push goes out the new way.",
        "Thanks for deleting that lock. This clears the three and updates the checkout:\n\n```\n! rm /home/tom/code/operator/main/.git/index.lock /home/tom/code/operator/main/.git/ORIG_HEAD.lock /home/tom/code/operator/main/.git/refs/remotes/origin/HEAD.lock && git -C /home/tom/code/operator/main merge --ff-only origin/main\n```",
        "The guard stops agents deleting it there. To remove it:\n\n```\n! rm /home/tom/code/wisp/main/window/wisp-window/src/XXQw5ucM\n```",
        "Run this in your terminal:\n\n```\ngit -C ~/code/operator/main merge --ff-only origin/main\n```",
    ];
    for reply in handed {
        assert!(hands_over_a_command(reply), "{reply}");
    }
    let reports = [
        "Not resumed yet. Every landing has been blocked since 1:46 PM.\n\n- **Landed:** the fallback for the lander's check environment (1274d536e, #782).\n- That limit is checked only after landing, not before!",
        "The check printed:\n\n```\nlane unlock: removed /home/tom/code/operator/main/.git/index.lock\n```\n\n![chart](chart.png)",
    ];
    for reply in reports {
        assert!(!hands_over_a_command(reply), "{reply}");
    }
}
