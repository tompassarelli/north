import os
from pathlib import Path


def ssh_argv(*arguments):
    configured = os.environ.get("NORTH_SSH_CONFIG")
    profile = Path(configured) if configured is not None else Path.home() / ".config/threads/ssh-config"
    route = ["-F", str(profile)] if configured is not None or profile.is_file() else []
    runtime = os.environ.get("XDG_RUNTIME_DIR")
    reuse = []
    if runtime and os.path.isabs(runtime):
        reuse = ["-o", "ControlMaster=auto", "-o", f"ControlPath={runtime}/north-ssh-%C",
                 "-o", "ControlPersist=10m", "-o", "ServerAliveInterval=5", "-o", "ServerAliveCountMax=2"]
    return ["ssh", *route, *reuse, *arguments]
