import json
import os
from pathlib import Path
import socket
import sys

sys.path.insert(0, str(Path(__file__).resolve().parent))
from usage import account_label, account_settings


def profiles():
    host = socket.gethostname()
    return {label: profile for label, profile in account_settings("claude").get("profiles", {}).items()
            if profile.get("host") == host}


def config_dir(profile):
    return Path(profile["path"]).expanduser() if "path" in profile else None


def account_for(directory):
    import hashlib
    path = (directory or Path.home()) / ".claude.json"
    data = json.loads(path.read_text())
    uuid = (data.get("oauthAccount") or {}).get("accountUuid") or (data.get("cachedUsageUtilization") or {}).get("accountUuid")
    return account_label("claude", hashlib.sha256(uuid.encode()).hexdigest()[:8]) if uuid else None


def select(label):
    profile = profiles().get(label)
    if profile is None:
        raise ValueError(f"Claude account {label} has no profile on {socket.gethostname()}")
    directory = config_dir(profile)
    if directory is not None and not (directory / ".credentials.json").is_file():
        raise ValueError(f"Claude account {label} is waiting for sign-in")
    if account_for(directory) != label:
        raise ValueError(f"Claude profile for account {label} is not signed in to that account")
    return directory


def sync():
    source = Path.home() / ".claude"
    for label, profile in profiles().items():
        directory = config_dir(profile)
        if directory is None:
            continue
        directory.mkdir(parents=True, exist_ok=True)
        for name in ("CLAUDE.md", "settings.json", "hooks", "skills", "agents", "commands"):
            target = source / name
            if not target.exists():
                continue
            link = directory / name
            if link.is_symlink() and link.readlink() == target:
                continue
            if link.exists() or link.is_symlink():
                backup = directory / (name + ".before-harness-sync")
                if backup.exists() or backup.is_symlink():
                    raise ValueError(f"profile projection backup already exists: {backup}")
                link.rename(backup)
            temporary = directory / (name + ".harness-sync")
            temporary.symlink_to(target)
            temporary.replace(link)
        print(f"Claude profile {label}: harness projected to {directory}")


if __name__ == "__main__":
    try:
        sync()
    except (OSError, ValueError) as error:
        print(f"Claude profile sync: {error}", file=sys.stderr)
        sys.exit(1)
