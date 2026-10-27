mod capacity_host;

use capacity_host::{Host, SCOPE_SYSTEMCTL};
use std::fs;
use std::process::Stdio;

const SYSTEMD_RUN: &str = "#!/bin/sh
for argument; do case $argument in --unit=*) unit=${argument#--unit=} ;; --setenv=*) export \"${argument#--setenv=}\" ;; esac; done
while [ \"$1\" != -- ]; do shift; done
shift
read host _ </proc/self/stat; echo \"$$ $host\" >\"$XDG_STATE_HOME/scopes/$unit\"
exec \"$@\"
";

fn user_manager_systemctl() -> String {
    let body = SCOPE_SYSTEMCTL.trim_start_matches("#!/bin/sh\n");
    format!("#!/bin/sh\n[ \"$DBUS_SESSION_BUS_ADDRESS\" = \"unix:path=/run/user/$(id -u)/bus\" ] && [ -z \"$XDG_RUNTIME_DIR\" ] || exit 1\n{body}")
}

#[test]
fn a_session_inside_a_private_desktop_reaches_the_user_manager_and_the_command_keeps_the_desktop_bus() {
    let host = Host::new("private-desktop.", &[("systemd-run", SYSTEMD_RUN), ("systemctl", &user_manager_systemctl())]);
    let private_bus = format!("unix:path={}/bus", host.path("").display());
    let seen = host.path("seen");
    let report = format!("printf '%s %s' \"$DBUS_SESSION_BUS_ADDRESS\" \"$XDG_RUNTIME_DIR\" >'{}'", seen.display());
    let output = host
        .helper(&["session", "--class", "native", "--memory-gib", "1", "--owner", "desktop-probe", "--", "sh", "-c", &report])
        .env("DBUS_SESSION_BUS_ADDRESS", &private_bus)
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "{stderr}");
    assert!(stderr.contains(r#""decision":"RESERVED""#), "{stderr}");
    assert_eq!(fs::read_to_string(&seen).unwrap(), format!("{private_bus} {}", host.path("").display()));
    assert_eq!(host.leases("native"), 0);
}
