---
name: private-desktop-development
description: >-
  Run GPU games and graphical tools on a private Wayland desktop beside Niri (private-desktop.sh start, control, type, capture) without touching Tom's input or focus. Also use before repeated screenshots or capture loops, after an image request-size failure, and for unattended web work in automation-chrome (Playwright actions, screenshots, page-to-page secret transfer).
grounded: 2026-10-15
written: 2026-10-15
metadata:
  kind: playbook
---

# Private desktop development

Graphical work runs on a separate hardware-rendered Wayland desktop so Tom's Niri desktop keeps its focus, clipboard and input. The clipboard-guard hook refuses clipboard reads, writes and Niri screenshot actions, and the north-hook desktop action guard refuses or wraps commands aimed at Tom's desktop while he uses it.

- Never switch Niri or inject input globally, because that reaches Tom's windows.
- Never use `ydotool`, the normal desktop's DISPLAY or X authority, or normal-desktop focus or window targets, because each reaches Tom's desktop.

## Which reference

- Starting, controlling, typing into or capturing a private desktop for a game or graphical tool, or checking one before reporting it usable: read [private desktop runs](references/private-desktop.md).
- About to take repeated screenshots or run any capture loop, or a request just failed on size after images: read [image context budget](references/image-context-budget.md) first.
- About to browse, fill forms, screenshot pages or move a secret between pages without Tom: read [automation Chrome](references/automation-chrome.md).
