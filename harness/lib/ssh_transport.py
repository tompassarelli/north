import os
from pathlib import Path


def ssh_argv(*arguments):
    configured = os.environ.get("NORTH_SSH_CONFIG")
    profile = Path(configured) if configured is not None else Path.home() / ".config/threads/ssh-config"
    route = ["-F", str(profile)] if configured is not None or profile.is_file() else []
    return ["ssh", *route, *arguments]
