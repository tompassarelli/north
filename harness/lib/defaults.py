from pathlib import Path
import tomllib

PATH = Path(__file__).resolve().parent.parent / "defaults.toml"


def registry(path=PATH):
    with open(path, "rb") as f:
        return tomllib.load(f)


DEFAULTS = {name: entry["value"] for name, entry in registry().items()}
