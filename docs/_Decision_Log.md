---
landmark: decision-log
---

| Date | Context | Decision | Why | Source | Status | Ref |
|---|---|---|---|---|---|---|
| 2026-11-13 | 全面優化時，趨勢科技把每次新 build 的 ToastWindow.exe 當新程式攔下 | 開發中不跑實機 exe，只用 cargo test 驗；實機由使用者安裝後實際使用驗證 | 每次 rebuild hash 都變，跑一次就跳一次封鎖視窗 | 使用者對話 | active | refactor/cleanup-and-fixes |
