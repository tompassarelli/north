import os
import re
from pathlib import Path


def ssh_argv(*arguments):
    configured = os.environ.get("NORTH_SSH_CONFIG")
    profile = Path(configured) if configured is not None else Path.home() / ".config/threads/ssh-config"
    route = ["-F", str(profile)] if configured is not None or profile.is_file() else []
    runtime = os.environ.get("XDG_RUNTIME_DIR")
    reuse = []
    explicit = any(arg.startswith("-S") or
                   re.split(r"[=\s]", (arguments[i + 1] if arg == "-o" and i + 1 < len(arguments) else arg[2:] if arg.startswith("-o") else "").strip(), maxsplit=1)[0]
                   .lower() in ("controlmaster", "controlpath", "controlpersist")
                   for i, arg in enumerate(arguments))
    if runtime and re.fullmatch(r"/[A-Za-z0-9_./-]*", runtime) and os.path.isabs(runtime) and not explicit and len(os.fsencode(runtime + "/north-ssh-" + "0" * 40)) + 20 <= 107:
        reuse = ["-o", "ControlMaster=auto", "-o", f"ControlPath={runtime}/north-ssh-%C",
                 "-o", "ControlPersist=10m", "-o", "ServerAliveInterval=5", "-o", "ServerAliveCountMax=2"]
    return ["ssh", *route, *reuse, *arguments]
