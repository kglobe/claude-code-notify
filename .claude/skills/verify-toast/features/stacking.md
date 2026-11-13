# Stacking and fade order

## Sub-features

- A new toast stacks above existing ones (below them when the taskbar is on top).
- Only the toast nearest the taskbar counts down (6 s) and fades. The others wait.
- When one closes, the others slide toward the taskbar.
- Hovering any toast pauses every toast's countdown.

## How to get to it (user POV)

Run several Claude Code sessions that finish close together.

## Driving it with verify.ps1

`verify.ps1 -Mode stack` opens three toasts 400 ms apart, screenshots them, then waits up to 12 s and prints `FADE-ORDER PASS` when the bottom toast closes first. Run it several times. One PASS proves little because the old bug depended on HWND values.

## Gotchas

- Keep the mouse off the toasts while it runs, or the countdown pauses and the check times out.
