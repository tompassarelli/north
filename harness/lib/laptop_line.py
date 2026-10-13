import subprocess


def laptop_line():
    try:
        r = subprocess.run(["laptop", "status"], capture_output=True, text=True, timeout=20)
    except (OSError, subprocess.TimeoutExpired):
        return "laptop: unknown (laptop status did not answer)"
    first = r.stdout.strip().splitlines()[:1]
    if r.returncode == 0 and first and first[0].startswith("laptop: "):
        return first[0]
    return "laptop: unknown (laptop status failed)"
