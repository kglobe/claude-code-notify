# Needs-input toast

## Sub-features

- Notification hook: status from `notification_type` (`permission_prompt` 需要授權, `idle_prompt` 等你回覆, `elicitation_dialog` MCP 詢問, else 需要你輸入). Message from `message`.
- PreToolUse `AskUserQuestion`: status 有問題要問你, message is the first question text.
- PreToolUse `ExitPlanMode`: status 計畫待確認.
- `auth_success`, `elicitation_complete`, `elicitation_response` show no toast.
- Orange badge with `!`.

## How to get to it (user POV)

Claude asks for a permission, asks a question, or finishes a plan.

## Driving it with verify.ps1

- `verify.ps1 -Mode input` for a permission prompt.
- `verify.ps1 -Mode input -Title q -Message '<question>'` for AskUserQuestion.
- Quoting proof: use a message that ends in a backslash or contains `"C:\dir\"`. The screenshot and debug log must show the message exactly, and the status line must not contain message text.

## Gotchas

- Message and title travel to the `--notify-show` child on the command line, so quoting bugs show up only here.
