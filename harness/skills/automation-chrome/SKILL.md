---
name: automation-chrome
description: Drive unattended web work in dedicated private headful Chrome with Playwright, screenshots, and page-to-page secret transfer.
metadata:
  kind: playbook
---

# Automation Chrome

- Use `automation-chrome` for unattended browser actions. It keeps its own persistent profile under the user's state directory, renders on the private desktop, and exposes CDP only on loopback. The private keyring may need its initial unlock on that desktop; keep encrypted storage. Site sign-in belongs to this profile; ask Tom when the task needs a sign-in he has not completed.
- Run `automation-chrome --help` for the JSON action interface. `automation-chrome tool` reads one request from stdin and returns one reply. Tab IDs remain stable across calls. Pass screenshots an absolute path inside the owned lane and inspect the saved image.
- Use `transfer` to copy a field value from one tab into another inside the tool. Its result and errors omit the value. Do not use `read`, `evaluate`, screenshots, shell arguments, or tool logs to extract secrets into the transcript.
- Close every working tab you create when the task ends. Keep the shared browser service for later work; `stop` ends its private desktop while preserving authentication in the dedicated profile. Never manipulate Tom's ordinary Chrome profile, windows, or focus.
- For whole-desktop capture, typing, or control, use the private run returned by `start`/`status` with the private-desktop-development skill. Never inject input on Tom's normal desktop.
