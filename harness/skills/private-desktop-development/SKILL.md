---
name: private-desktop-development
description: >-
  Run GPU-accelerated Linux games and graphical development tools in a private desktop beside Niri, with remote input that never grabs the normal desktop. Also use to inspect repeated screenshots or graphical sessions without accumulating image payloads in conversation history, to recover from image-related request-size failures, and to drive unattended web work in dedicated private headful Chrome (automation-chrome) with Playwright, screenshots and page-to-page secret transfer.
grounded: 2026-10-15
written: 2026-10-15
metadata:
  kind: playbook
---

# Private desktop development

Graphical work runs on a separate hardware-rendered Wayland desktop so Tom's Niri desktop keeps its focus, clipboard and input. The clipboard-guard hook refuses clipboard reads, writes and Niri screenshot actions, and the north-hook desktop action guard refuses or wraps commands aimed at Tom's desktop while he uses it.

## Which reference

- Repeated screenshots, any capture loop, or a request-size failure after images: read [image context budget](references/image-context-budget.md) first.
- Unattended web work in a browser: read [automation Chrome](references/automation-chrome.md).
- Capture or modifier input disagrees with what the app shows: read [private input diagnostics](references/private-input-diagnostics.md).
- Setting up the X11 fallback control: read [private XTEST](references/private-xtest.md).

## Start and own a run

- Resolve the launcher with `agents path private-desktop-development` and use its sibling `scripts/private-desktop.sh`; its `--help` documents start, control, type and capture, resolution, ports, render nodes, deadlines and paths.
- Provide `agents`, `bun`, `nix` and GPU render-node access, because the launcher needs all four.
- Keep game-specific launch commands in the game project, because this skill only owns the desktop.
- Never switch Niri or inject input globally, because that reaches Tom's windows.
- Launch retained desktops directly or inside a native `session`, and keep the foreground capacity allowance until the session ends, because the machine-capacity helper bounds the run for its whole life.
- Use `--seconds` only for a requested finite deadline and respect an enclosing `run` deadline, because the default lifetime is until stopped.
- Never run two clients against one mutable Wine or Proton prefix, because they corrupt each other's state.
- Read the printed run directory and localhost port and control only that run through the launcher, because other runs may belong to other agents.
- Start the application only after the requested output mode is set, using its project launcher and supported credential stores, and keep secrets out of arguments and logs, because arguments and logs are visible to other processes and transcripts.
- Stop with Ctrl-C or SIGTERM to the foreground wrapper and clean only its exact run directory and processes, because the launcher removes its own runtime files.

## Isolation

- Keep passwordless VNC bound to `127.0.0.1` and never expose it to a network, because anyone reaching it controls the session.
- Use the launcher's per-command `dbus-run-session` and never inherit or recreate the normal desktop's D-Bus address, except automation-chrome's proxy that forwards only org.freedesktop.secrets, because anything else routes actions to Tom's session.
- Keep VNC port selection serialized until listener readiness and never replace the launcher lock with independent probes or startup sleeps, because concurrent starts otherwise race for one port.
- Never use `ydotool`, the normal desktop's DISPLAY or X authority, or normal-desktop focus or window targets, because each reaches Tom's desktop.
- Honor explicit current-display testing inside the identified game window when Tom asks for it, and never substitute streaming for a requested native controller or latency trial, because the trial measures the real path.

## Input

- Type with `type` from stdin, including secrets, and never with `control ... type` or `xdotool type`, because only stdin keeps text out of arguments and checks the US layout first.
- Keep typed text within the supported US layout, because the launcher refuses other characters.
- Run `cargo test private_desktop::typing` when changing key mapping, under the `verification` skill, because mapping errors type wrong characters silently.
- Use lowercase VNC key names and control sequences shorter than eight seconds, and keydown/keyup or mousedown/mouseup for holds, because longer sequences hit the control timeout.
- Use private XTEST for X11 pointer or window queries only after VNC input fails and the private window is confirmed active, because XTEST bypasses the VNC path being diagnosed.
- Observe the actual control change before advancing, and never infer keyboard delivery from command success or pointer coordinates, because VNC modifiers have been observed to report success without effect.

## Capture

- Check capture exit status and require an active run, a live private socket and a fresh nonempty absolute PNG before inspection, because a preserved old destination is stale.
- Respect capture's eight-second timeout and one-second termination grace, because the launcher kills captures past it.
- Resolve `grim` once for retained older sessions and never realize a Nix shell per frame, because each realization costs seconds.
- Interpret the application's visible state rather than treating a fresh capture as evidence of advancement, because a frame can be fresh and unchanged.
- Inspect captures as text or bounded previews under the [image context budget](references/image-context-budget.md), because images accumulate in conversation history.

## Before reporting usable

Check the startup `glxinfo -B` hardware renderer, an updating private framebuffer, delivered keys and pointer, one ordinary in-game action, and retained focus and input on Tom's normal desktop, because each has failed independently.
