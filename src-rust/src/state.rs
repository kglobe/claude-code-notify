//! State file save/load/delete.
//!
//! State file: %TEMP%\claude-notify-{session_id}.txt
//! Format: 4 lines (HWND, RuntimeId, caller exe path, user prompt)

use windows::Win32::Foundation::HWND;
use windows::Win32::UI::WindowsAndMessaging::IsWindow;

/// Data stored in and loaded from the state file.
#[derive(Default)]
pub struct State {
    pub target_hwnd: HWND,
    pub wt_hwnd: HWND,
    pub wt_runtime_id: String,
    pub icon_path: String,
    pub user_prompt: String,
}

/// Get the state file path for a session.
pub fn state_file_path(session_id: &str) -> std::path::PathBuf {
    let temp = std::env::temp_dir();
    temp.join(format!("claude-notify-{}.txt", session_id))
}

/// Save state to the state file (4 lines).
pub fn save_state(session_id: &str, hwnd: HWND, runtime_id: &str, icon_path: &str, prompt: &str) {
    let path = state_file_path(session_id);
    let hwnd_val = hwnd.0 as usize;
    let content = format!("{}\n{}\n{}\n{}", hwnd_val, runtime_id, icon_path, prompt);
    let _ = std::fs::write(&path, content);
}

/// Load state from the state file.
pub fn load_state(session_id: &str) -> State {
    let mut state = State::default();
    let path = state_file_path(session_id);

    let content = match std::fs::read_to_string(&path) {
        Ok(c) => c,
        Err(_) => return state,
    };

    let lines: Vec<&str> = content.lines().collect();

    // Line 1: HWND
    if let Some(line) = lines.first() {
        if let Ok(val) = line.trim().parse::<usize>() {
            let hwnd = HWND(val as *mut _);
            if unsafe { IsWindow(Some(hwnd)).as_bool() } {
                state.target_hwnd = hwnd;
                // Check if this is Windows Terminal
                let class = crate::util::get_class_name(hwnd);
                if class == "CASCADIA_HOSTING_WINDOW_CLASS" {
                    state.wt_hwnd = hwnd;
                }
            }
        }
    }

    // Line 2: RuntimeId
    if let Some(line) = lines.get(1) {
        let trimmed = line.trim();
        if !trimmed.is_empty() {
            state.wt_runtime_id = trimmed.to_string();
        }
    }

    // Line 3: Caller exe path
    if let Some(line) = lines.get(2) {
        let trimmed = line.trim();
        if !trimmed.is_empty() {
            state.icon_path = trimmed.to_string();
        }
    }

    // Line 4: User prompt (may contain the rest of the file if there were newlines)
    if lines.len() > 3 {
        // Join remaining lines back (prompt may contain newlines)
        state.user_prompt = lines[3..].join("\n");
    }

    state
}

/// Target window, WT tab and caller exe saved by an earlier prompt in this session, if the window still exists.
pub fn saved_target(session_id: &str) -> Option<(HWND, String, String)> {
    let s = load_state(session_id);
    (s.target_hwnd != HWND::default()).then_some((s.target_hwnd, s.wt_runtime_id, s.icon_path))
}

/// Delete the state file for a session.
pub fn delete_state(session_id: &str) {
    let path = state_file_path(session_id);
    let _ = std::fs::remove_file(&path);
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::UI::WindowsAndMessaging::GetDesktopWindow;

    #[test]
    fn saved_target_only_returns_a_live_window() {
        let id = format!("test-saved-target-{}", std::process::id());
        assert_eq!(saved_target(&id), None);

        let desktop = unsafe { GetDesktopWindow() };
        save_state(&id, desktop, "rid", r"C:\app.exe", "p");
        assert_eq!(saved_target(&id), Some((desktop, "rid".to_string(), r"C:\app.exe".to_string())));

        save_state(&id, HWND(0x7fff_fff0 as *mut _), "rid", "", "p");
        assert_eq!(saved_target(&id), None);
        delete_state(&id);
    }
}
