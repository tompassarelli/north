#!/usr/bin/env python3
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile

TARGET = Path(os.environ.get("OUTSIDE_WATCH_TARGET", Path(__file__).with_name("outside-watch"))).resolve()
API = "https://api.github.com/"
REPO = "repos/tompassarelli/smashcraft"
FAKE_GH = r'''#!/usr/bin/env python3
import json, os, sys
args = sys.argv[2:]
with open(os.environ["FAKE_GH_CALLS"], "a") as fh:
    fh.write(json.dumps(args) + "\n")
method, fields, endpoint, i = "GET", [], None, 0
while i < len(args):
    a = args[i]
    if a == "-X":
        method = args[i + 1]; i += 2; continue
    if a == "-f":
        fields.append(args[i + 1]); i += 2; continue
    if not a.startswith("-") and endpoint is None:
        endpoint = a
    i += 1
endpoint = endpoint.removeprefix("https://api.github.com/").split("?")[0]
if endpoint == "graphql":
    endpoint += ":minimize" if any("minimizeComment" in f for f in fields) else ":search"
fixtures = json.load(open(os.environ["FAKE_GH_FIXTURES"]))
key = f"{method} {endpoint}"
if key not in fixtures:
    if method == "GET" and endpoint.startswith("user/blocks/"):
        sys.exit(1)
    body = [] if "--paginate" in args else {}
else:
    body = fixtures[key]
print(json.dumps([body] if "--slurp" in args else body))
'''


def user(login, kind="User"):
    return {"login": login, "type": kind}


def comment(cid, number, login, body, association="NONE", created="2026-10-14T09:22:00Z", kind="User"):
    return {"id": cid, "node_id": f"IC_{cid}", "issue_url": f"{API}{REPO}/issues/{number}", "user": user(login, kind),
            "body": body, "author_association": association, "created_at": created, "updated_at": created,
            "html_url": f"https://github.com/tompassarelli/smashcraft/issues/{number}#issuecomment-{cid}"}


SPAM = comment(1, 422, "AILIFE1", "Can you share more? API is at cathedral-ai.com, docs at cathedral-ai.com/docs.")
REAL = comment(2, 450, "515", "OCR output like @515,460 pings accounts named 515; see https://github.com/tompassarelli/smashcraft/issues/450")
LINKER = comment(3, 422, "carol", "Same crash here with the build from stormlib.org, happy to send a log.")
OURS = [comment(4, 422, "tompassarelli", "agent update at example.com", "OWNER"),
        comment(5, 422, "github-actions[bot]", "Autoland result", "CONTRIBUTOR", kind="Bot")]
NOTIFICATIONS = [
    {"id": "n-ci", "subject": {"type": "CheckSuite", "url": None}, "last_read_at": None},
    {"id": "n-ours", "subject": {"type": "Issue", "url": f"{API}{REPO}/issues/550"}, "last_read_at": None},
    {"id": "n-real", "subject": {"type": "Issue", "url": f"{API}{REPO}/issues/450"}, "last_read_at": "2026-10-14T08:00:00Z"},
]
FIXTURES = {
    "GET search/issues": {"items": [
        {"repository_url": f"{API}{REPO}", "number": 422, "node_id": "I_422", "user": user("tompassarelli"), "title": "Kind tags",
         "body": "tags", "author_association": "OWNER", "created_at": "2026-10-01T00:00:00Z", "html_url": "x"},
        {"repository_url": f"{API}{REPO}", "number": 600, "node_id": "PR_600", "user": user("newbie"), "pull_request": {},
         "title": "Add launcher", "body": "Packaged at newbie-tools.io", "author_association": "FIRST_TIME_CONTRIBUTOR",
         "created_at": "2026-10-14T09:21:00Z", "html_url": "https://github.com/tompassarelli/smashcraft/pull/600"},
    ]},
    f"GET {REPO}/issues/comments": [SPAM, REAL, LINKER, *OURS],
    f"GET {REPO}/issues/422": {"title": "Kind tags", "body": "tags", "user": user("tompassarelli"),
                               "created_at": "2026-10-01T00:00:00Z"},
    f"GET {REPO}/issues/422/comments": [SPAM, LINKER, *OURS],
    f"GET {REPO}/issues/550": {"user": user("tompassarelli"), "created_at": "2026-10-14T07:00:00Z"},
    f"GET {REPO}/issues/550/comments": OURS,
    f"GET {REPO}/issues/450": {"user": user("tompassarelli"), "created_at": "2026-10-09T00:00:00Z"},
    f"GET {REPO}/issues/450/comments": [REAL],
    "GET users/AILIFE1": {"created_at": "2026-01-03T15:22:35Z", "blog": "https://cathedral-ai.com", "bio": "Creator of Cathedral"},
    "GET users/carol": {"created_at": "2015-04-01T00:00:00Z", "blog": "https://carol.dev", "bio": None},
    "GET users/newbie": {"created_at": "2026-10-13T00:00:00Z", "blog": "newbie-tools.io"},
    "GET graphql:search": {"data": {"search": {"nodes": []}}},
    "POST graphql:minimize": {"data": {"minimizeComment": {"minimizedComment": {"isMinimized": True}}}},
    "GET notifications": NOTIFICATIONS,
}

passed = failed = 0


def check(name, ok, detail=""):
    global passed, failed
    if ok:
        passed += 1
        print(f"PASS  {name}")
    else:
        failed += 1
        print(f"FAIL  {name}\n  {detail}")


with tempfile.TemporaryDirectory(prefix="outside-watch-test-") as temporary:
    root = Path(temporary)
    (root / "bin").mkdir()
    (root / "bin/gh").write_text(FAKE_GH)
    (root / "bin/gh").chmod(0o755)
    (root / "fixtures.json").write_text(json.dumps(FIXTURES))
    inbox = root / "proxy-inbox.md"
    inbox.write_text("earlier line\n")
    calls = root / "calls"
    env = {**os.environ, "PATH": f"{root / 'bin'}:{os.environ['PATH']}", "FAKE_GH_FIXTURES": str(root / "fixtures.json"),
           "FAKE_GH_CALLS": str(calls), "OUTSIDE_WATCH_STATE": str(root / "state"), "OUTSIDE_WATCH_INBOX": str(inbox),
           "OUTSIDE_WATCH_NOW": "2026-10-14T09:30:00Z"}

    def watch(*args):
        calls.write_text("")
        out = subprocess.run([sys.executable, str(TARGET), *args], env=env, capture_output=True, text=True)
        made = [json.loads(line) for line in calls.read_text().splitlines()]
        return out, made

    def writes(made):
        return [c for c in made if "PUT" in c or "PATCH" in c or any("minimizeComment" in a for a in c)]

    out, made = watch("run", "--dry-run", "--days", "7")
    lines = out.stdout.splitlines()
    check("a dry run over 7 days lists the promotional comment as spam",
          any(l.startswith("SPAM smashcraft#422 AILIFE1:") for l in lines), out.stdout + out.stderr)
    check("a dry run lists the person reporting the @-mention problem as a real person",
          any(l.startswith("EXTERNAL smashcraft#450 515:") for l in lines), out.stdout)
    check("an established account linking an unrelated site is a real person, not spam",
          any(l.startswith("EXTERNAL smashcraft#422 carol:") for l in lines), out.stdout)
    check("a first-time contributor's pull request is never spam, even with a link to their own site",
          any(l.startswith("EXTERNAL smashcraft#600 newbie:") for l in lines), out.stdout)
    check("our own actors never show up", not any(" tompassarelli:" in l or "github-actions" in l for l in lines), out.stdout)
    check("a dry run writes nothing to GitHub, the inbox or the state",
          not writes(made) and inbox.read_text() == "earlier line\n" and not (root / "state/state.json").exists(),
          json.dumps(writes(made)))

    out, made = watch("run")
    posted = inbox.read_text().splitlines()
    check("a live run minimizes the spam comment as SPAM by its node id",
          any(any("minimizeComment" in a for a in c) and "id=IC_1" in c for c in made), json.dumps(made))
    check("a live run blocks the spam account", ["-X", "PUT", "user/blocks/AILIFE1"] in made, json.dumps(made))
    check("a live run blocks nobody else", sum(1 for c in made if "PUT" in c) == 1, json.dumps(writes(made)))
    check("the proxy inbox gets OPS EXTERNAL for each real person and OPS SPAM for the spam",
          sum(l.startswith("OPS EXTERNAL smashcraft#") for l in posted) == 3
          and any(l.startswith("OPS SPAM smashcraft#422 AILIFE1: minimized and blocked") for l in posted), "\n".join(posted))
    marked = {c[-1] for c in made if "PATCH" in c}
    check("notifications whose every actor is ours are marked read, the real person's thread stays unread",
          marked == {"notifications/threads/n-ci", "notifications/threads/n-ours"}, str(marked))
    check("the run logs a clean run", "clean run" in (root / "state/log").read_text(), out.stderr)

    out, made = watch("run")
    check("a second run posts nothing it already posted", inbox.read_text().splitlines() == posted and
          not any("PUT" in c or any("minimizeComment" in a for a in c) for c in made), inbox.read_text())

print(f"outside-watch: {passed} passed, {failed} failed")
sys.exit(1 if failed else 0)
