# Claude Code Notify

Claude Code 的 Windows 通知卡片。Claude 回完話或需要你輸入時，工作列上方會跳出一張小卡片，點一下就回到你下指令的那個視窗和 Windows Terminal 分頁。

[English](README.md)

![Windows](https://img.shields.io/badge/Windows-10%2F11-0078D6?logo=windows&logoColor=white)
![License](https://img.shields.io/badge/License-MIT-green)

<img src="assets/screenshot.png" width="420" alt="通知卡片，顯示專案資料夾名稱、狀態「已完成」、使用者的 prompt 和工作目錄">

這個專案 fork 自 [chuilishi/claude-code-notify](https://github.com/chuilishi/claude-code-notify)。1.2.0 版重新設計了卡片外觀，修掉參數引號和卡片堆疊的 bug，也精簡了 Rust 程式。

## 安裝

```bash
claude plugin marketplace add kglobe/claude-code-notify
claude plugin install claude-code-notify@claude-code-notify
```

裝完重開 Claude Code。plugin 會自己註冊 hooks，不用改 `settings.json`。

移除：

```bash
claude plugin uninstall claude-code-notify
```

### WSL 出現 `Permission denied`

在 WSL 裡裝 plugin，hook 執行時若出現 `Permission denied`，把附帶的 exe 設成可執行：

```bash
chmod +x ~/.claude/plugins/cache/claude-code-notify/claude-code-notify/*/notifications/ToastWindow.exe
```

這是上游 [issue #2](https://github.com/chuilishi/claude-code-notify/issues/2) 的做法。shell 沒展開 `*` 的話，換成版本目錄，例如 `1.2.0`。

## 卡片內容

由上到下：

| 列 | 內容 |
|----|------|
| 標頭 | Claude 圖示、「Claude Code」、時間、關閉鈕 |
| 標題 | session 工作目錄的資料夾名稱。左邊的徽章完成時是勾，需要你處理時是 `!` |
| 狀態 | 已完成、需要授權、等你回覆、MCP 詢問、有問題要問你、計畫待確認、需要你輸入，七種之一 |
| 訊息 | 你最後一次的 prompt，或 Claude 的問題、要授權的內容 |
| 路徑 | 完整工作目錄 |

卡片跟著系統的亮色或暗色主題，也會依 DPI 縮放。狀態文字固定是繁體中文，沒有設定可以改。

## 操作

| 動作 | 結果 |
|------|------|
| 左鍵點卡片 | 把原本的視窗拉到前面，切回原本的 Windows Terminal 分頁 |
| 右鍵或點 X | 關掉卡片 |
| 滑鼠移到卡片上 | 所有卡片暫停倒數 |

每張卡片顯示 6 秒，再花 0.6 秒淡出。好幾張疊在一起時，只有最靠近工作列的那張在倒數，其他的排隊等。

跳卡片時會播音效，用 `notifications/assets/sound/` 裡第一個 `.wav`，找不到就播 Windows 的「星號」系統音效。

## 運作方式

plugin 只有一支 exe，`notifications/ToastWindow.exe`，由 `hooks/hooks.json` 的五個 hook 呼叫：

| Hook | 參數 | 做什麼 |
|------|------|--------|
| `UserPromptSubmit` | `--save` | 記下跑 Claude Code 的那個 app 的視窗（認不出來就記前景視窗）、Windows Terminal 目前的分頁、呼叫端 app 的 exe 路徑和你的 prompt |
| `Notification` | `--input` | 顯示「需要輸入」卡片（授權、閒置、MCP elicitation） |
| `PreToolUse`，matcher `AskUserQuestion\|ExitPlanMode` | `--input` | 顯示「有問題」或「計畫待確認」卡片 |
| `Stop` | `--notify` | 顯示「已完成」卡片 |
| `SessionEnd` | `--cleanup` | 刪掉這個 session 的狀態檔 |

狀態存在 `%TEMP%\claude-notify-{session_id}.txt`，一個 session 一個檔，所以同時開好幾個 Claude Code 視窗也不會互相蓋掉。

hook 呼叫的那支 exe 會馬上結束，另外用 `--notify-show` 起一支獨立的 exe 畫卡片，這支要等卡片關掉才結束。

視窗和分頁怎麼對回去：

- 視窗：從 hook 的 process 往上找，跳過 shell 和 runtime，第一個遇到的 app 就是目標。要跳過的 process 清單和認得的 app 清單（VS Code、Cursor、Windsurf、JetBrains IDE、Windows Terminal 等）都在 `src-rust/src/process.rs`。
- Windows Terminal 分頁：送出 prompt 時用 UI Automation 記下分頁的 RuntimeId，點卡片時再選回那個分頁。
- 背景 process 要把視窗拉到前面，Windows 會擋，所以要連續呼叫好幾個 Win32 API，寫在 `src-rust/src/activate.rs`。

卡片不會搶焦點（`WS_EX_NOACTIVATE`），永遠在最上層。卡片之間靠視窗類別 `ClaudeCodeToast` 找到彼此，疊起來不會重疊。

## 自己建置

要在 Windows 裝好 Rust。

```bash
cd src-rust
cargo test
cargo build --release
cp target/release/toast-window.exe ../notifications/ToastWindow.exe
```

hook 指令加上 `--debug`，log 會寫到 exe 旁邊的 `debug.log`。

不開 Claude Code 也能看 release 版的卡片：用 PowerShell 7 跑驗證腳本（腳本用到 `ArgumentList`，Windows PowerShell 5 沒有）。`-Mode` 可以是 `notify`、`input`、`stack`、`doctor`，截圖存到 `verify-evidence/`。

```powershell
pwsh .claude/skills/verify-toast/verify.ps1 -Mode stack
```

有些防毒軟體會擋剛建好的 exe，要先放行。

## 授權

MIT
