# Automation Chrome

Read this for unattended web work: browsing, form filling, screenshots or moving a secret from one page to another.

- Use `automation-chrome` for unattended browser actions, because it keeps its own persistent profile under the user's state directory, renders on the private desktop and exposes CDP only on loopback.
- Keep encrypted storage, and never ask Tom to unlock a keyring, because Chrome reads the profile's sign-ins through the user session's login keyring (Secret Service only) and that keyring is already unlocked.
- Sign in to sites only inside this profile, and ask Tom when the task needs a sign-in he has not completed, because sign-in is his credential decision.
- Run `automation-chrome --help` for the JSON action interface; `automation-chrome tool` reads one request from stdin and returns one reply, and tab IDs stay stable across calls.
- Pass screenshots an absolute path inside the owned lane and inspect the saved file under [image context budget](image-context-budget.md), because attached images accumulate in history.
- Use `transfer` to copy a field value from one tab into another inside the tool, because its result and errors omit the value.
- Never use `read`, `evaluate`, screenshots, shell arguments or tool logs to extract secrets, because each of those lands the value in the transcript.
- Close every tab you create when the task ends, because the browser service is shared with later work.
- Keep the shared browser service running; `stop` ends its private desktop and keeps authentication in the dedicated profile.
- Never touch Tom's ordinary Chrome profile, windows or focus, because he is using them.
- For whole-desktop capture, typing or control, use the private run returned by `start` or `status` with the private desktop launcher, because input on Tom's normal desktop would reach his windows.
