# Click to return to the window

## Sub-features

- Left-click on the body brings back the window that was in front when the prompt was sent. In Windows Terminal it also selects the saved tab.
- Left-click on the X, or right-click anywhere, closes without switching.

## How to get to it (user POV)

Click the toast after switching to another app.

## Driving it with verify.ps1

Not scripted. Run `verify.ps1 -Mode notify` from a Windows Terminal tab, switch to another app and tab, then click the toast body before it fades. Pass when the original tab comes to the front. Clicking moves the user's focus, so do it only when the user is not typing.

## Gotchas

- Windows focus-stealing rules can block activation. The code uses the ALT-key trick and AttachThreadInput.
