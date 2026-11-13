# Task-completed toast

## Sub-features

- Title is the last folder of `cwd`. Falls back to `Claude Code` when `cwd` is empty.
- Status line `已完成` in green with a check badge.
- Message is the user's last prompt saved by `--save`, whitespace collapsed, capped at 300 chars, two lines max with ellipsis.
- Bottom line is the full `cwd`, path-ellipsized.
- Plays `assets/sound/*.wav`. Falls back to a system beep.

## How to get to it (user POV)

Send a prompt in Claude Code. When Claude stops, the toast appears in the corner nearest the taskbar.

## Driving it with verify.ps1

`verify.ps1 -Mode notify`. Check the screenshot for `claude-code-notify`, `已完成`, `verify prompt line`, and the repo path.

## Gotchas

- Without `--save` first, the message is empty because the prompt comes from the state file.
- Light or dark card follows `AppsUseLightTheme` in the registry.
