import re
PATCH_BEGIN = "*** Begin Patch"
PATCH_FILE_HEADER = re.compile(r"^\*\*\* (?:Add|Update|Delete) File:\s+(.+?)\s*$", re.M)
PATCH_MOVE_HEADER = re.compile(r"^\*\*\* Move to:\s+(.+?)\s*$", re.M)
def _patch_targets(envelope):
    """Raw file targets named by one apply_patch envelope."""
    return (PATCH_FILE_HEADER.findall(envelope)
            + PATCH_MOVE_HEADER.findall(envelope))

def _find_envelope(value):
    """The first nested string containing an apply_patch envelope."""
    if isinstance(value, str):
        return value if PATCH_BEGIN in value else None
    if isinstance(value, dict):
        for child in value.values():
            found = _find_envelope(child)
            if found is not None:
                return found
    return None
