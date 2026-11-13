---
name: verify-toast
description: Drive the real ToastWindow.exe (claude-code-notify) the way Claude Code's hooks do, capture screenshots and debug logs of the toast. Use to prove a change to the Rust notifier (src-rust/) works on the desktop, not just that it compiles.
---

# verify-toast

The app is a Windows GUI exe that Claude Code hooks call with JSON on stdin (`hooks/hooks.json`). A user sees a toast in the screen corner. There is no server. Every drive starts fresh processes.

## Launch

1. Build: `cargo build --release --manifest-path src-rust/Cargo.toml` (about 2 minutes clean, seconds incremental). Ready when it prints `Finished`.
2. Each drive goes through `verify.ps1`, which copies `src-rust/target/release/toast-window.exe` to `verify-evidence/stage/ToastWindow.exe` next to a copy of `notifications/assets/`. That mirrors the plugin layout, so the sound and the Claude logo load. Running the exe straight from `target/release` shows no logo and no sound.

Run from the repo root in PowerShell 7:

```powershell
.\.claude\skills\verify-toast\verify.ps1 -Mode doctor
.\.claude\skills\verify-toast\verify.ps1 -Mode notify
.\.claude\skills\verify-toast\verify.ps1 -Mode input -Message 'Path ends with C:\temp\'
.\.claude\skills\verify-toast\verify.ps1 -Mode input -Title q -Message 'Use "C:\dir\" here?'
.\.claude\skills\verify-toast\verify.ps1 -Mode stack
```

`-Title <anything>` switches input mode to a PreToolUse AskUserQuestion payload. The message becomes the question text.

## Doctor

`-Mode doctor` prints the built exe size and timestamp and how many toasts are visible now. If the timestamp is older than your last source edit, rebuild. If toasts are already visible, they belong to the user's real Claude Code session. The script only closes processes it started, but screenshots will include theirs.

## Drive

`verify.ps1` pipes the same JSON a hook sends (`session_id`, `cwd`, `prompt`, `notification_type`, `message`, `tool_name`, `tool_input`) into `--save`, then `--notify` or `--input` with `--debug`. The hook-mode process spawns a detached `--notify-show` child that owns the toast window. Toast windows are found by class name `ClaudeCodeToast`.

## Evidence

All proof lands in `verify-evidence/` (git-ignored) and survives cleanup:

- `<stamp>-<mode>.png`: screenshot of the union of visible toast rects.
- `<stamp>-<mode>-debug.log`: the `--debug` log, which shows parsed title, status, message, and paths.
- stdout: the toast table (HWND, PID, rect) and, for `stack`, a `FADE-ORDER PASS` or `FADE-ORDER FAIL` line.

Proof standard: read the screenshot and confirm the title, status line, message, and path are what the payload implies. For argument-quoting changes, the message in the screenshot and in the debug log must match the input byte for byte.

## Cleanup

The script stops only toast processes whose PID was not visible before the run, and deletes `%TEMP%\claude-notify-verify-*.txt` state files it created. It never kills by process name.

## Features

See `features/README.md`.
