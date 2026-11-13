---
landmark: decision-log
---

| Date | Context | Decision | Why | Source | Status | Ref |
|---|---|---|---|---|---|---|
| 2026-11-13 | 全面優化時，趨勢科技把每次新 build 的 ToastWindow.exe 當新程式攔下 | 開發中不跑實機 exe，只用 cargo test 驗；實機由使用者安裝後實際使用驗證 | 每次 rebuild hash 都變，跑一次就跳一次封鎖視窗 | 使用者對話 | active | refactor/cleanup-and-fixes |
| 2026-11-13 | 在 Herdr 送出 prompt 後切到 Edge 或 Obsidian，點卡片跳到那個程式。量到按 Enter 到 `--save` 存檔約 1.7 秒 | `--save` 改存「跑 Claude 的 app」的視窗：先找擁有視窗的最近 ancestor process，沒有就找 exe 名稱在第一個 `-` 前跟呼叫端相同的程式（herdr-core 對 herdr-app），都沒有才用前景視窗 | hook 起來時前景視窗可能已經換人。Herdr 的終端機掛在獨立的 herdr-core 底下，process tree 走不到 herdr-app，只能靠名稱對 | 使用者對話、實機量測 | active | src-rust/src/process.rs `host_window` |
