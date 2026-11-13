#![windows_subsystem = "windows"]

mod activate;
mod assets;
mod cli;
mod json;
mod log;
mod process;
mod spawn;
mod state;
mod toast;
mod uiautomation;
mod util;

use windows::Win32::Foundation::HWND;
use windows::Win32::System::Com::*;
use windows::Win32::UI::WindowsAndMessaging::*;

fn print_usage() {
    unsafe {
        let _ = windows::Win32::System::Console::AllocConsole();
    }
    println!(
        "Usage:\n  \
         ToastWindow.exe --save      Save window state (UserPromptSubmit hook)\n  \
         ToastWindow.exe --notify    Show task-completed notification (Stop hook)\n  \
         ToastWindow.exe --input     Show input-required notification (Notification / PreToolUse hooks)\n  \
         ToastWindow.exe --cleanup   Delete the session state file (SessionEnd hook)\n\n\
         All modes read session_id from stdin JSON for state file isolation."
    );
}

fn exe_path() -> String {
    std::env::current_exe()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned()
}

fn run_save_mode(immediate_hwnd: HWND) -> i32 {
    let input = json::read_stdin_json();
    let session_id = json::extract_string(&input, "session_id");
    let prompt = json::extract_string(&input, "prompt");

    if session_id.is_empty() {
        debug_log!("No session_id, skipping save");
        return 0;
    }

    debug_log!("Session ID: {}", session_id);
    debug_log!("Prompt: {}", prompt);

    // UserPromptSubmit also fires for prompts Claude Code injects (task notifications,
    // subagent hand-backs) while another app may be in front. Never save on those, so the
    // last typed prompt's window and text stay; a typed prompt always recaptures.
    if is_system_prompt(&prompt) {
        debug_log!("System-generated prompt, keeping saved state");
        return 0;
    }

    let (hwnd, runtime_id, caller_path) = capture_target(immediate_hwnd);
    debug_log!("Caller exe path: {}", caller_path);

    state::save_state(&session_id, hwnd, &runtime_id, &caller_path, &prompt);
    debug_log!("State saved to {:?}", state::state_file_path(&session_id));

    0
}

/// Foreground window, its WT tab RuntimeId, and the caller exe path for the icon.
fn capture_target(immediate_hwnd: HWND) -> (HWND, String, String) {
    // Use immediate_hwnd, fall back to GetForegroundWindow if invalid (SPEC 3.2)
    let hwnd = if !immediate_hwnd.is_invalid()
        && immediate_hwnd != HWND::default()
        && unsafe { IsWindow(Some(immediate_hwnd)).as_bool() }
    {
        debug_log!("Using immediate HWND: {:?}", immediate_hwnd);
        immediate_hwnd
    } else {
        let fallback = unsafe { GetForegroundWindow() };
        debug_log!("Immediate HWND invalid, using fallback: {:?}", fallback);
        fallback
    };

    let caller_path = process::find_caller_exe_path();
    let hwnd = process::host_window(hwnd, &caller_path);
    debug_log!("Host window: {:?}", hwnd);

    // Detect Windows Terminal and get RuntimeId
    let mut runtime_id = String::new();
    let class = util::get_class_name(hwnd);
    debug_log!("Window class: {}", class);

    if class == "CASCADIA_HOSTING_WINDOW_CLASS" {
        debug_log!("Detected Windows Terminal, capturing tab RuntimeId");
        runtime_id = uiautomation::get_selected_tab_runtime_id(hwnd);
        debug_log!("RuntimeId: {}", runtime_id);
    }

    (hwnd, runtime_id, caller_path)
}

fn run_notify_mode(debug: bool) -> i32 {
    let input = json::read_stdin_json();
    let session_id = json::extract_string(&input, "session_id");

    if session_id.is_empty() {
        debug_log!("No session_id for notify mode");
        return 1;
    }

    debug_log!("Notify mode, session: {}", session_id);

    spawn_display(&session_id, "", &input, debug);
    0
}

fn run_input_mode(debug: bool) -> i32 {
    let input = json::read_stdin_json();
    let session_id = json::extract_string(&input, "session_id");

    if session_id.is_empty() {
        debug_log!("No session_id for input mode");
        return 1;
    }

    let notification_type = json::extract_string(&input, "notification_type");
    let tool_name = json::extract_string(&input, "tool_name");

    // Filter out non-actionable notification types (auth success, MCP elicitation echoes).
    // These don't require user attention, so we skip the toast entirely.
    if matches!(
        notification_type.as_str(),
        "auth_success" | "elicitation_complete" | "elicitation_response"
    ) {
        debug_log!(
            "Skipping non-actionable notification_type: {}",
            notification_type
        );
        return 0;
    }

    // Decide title and message based on the actual scenario.
    // Priority: PreToolUse tool_name > Notification notification_type > generic fallback.
    let (title, message) = if tool_name == "AskUserQuestion" {
        // Claude is proactively asking the user a question (with options UI).
        let q = json::extract_first_question(&input);
        let msg = if q.is_empty() {
            "Claude 有問題要問你".to_string()
        } else {
            q
        };
        ("有問題要問你".to_string(), msg)
    } else if tool_name == "ExitPlanMode" {
        // Plan mode: Claude finished planning and wants user approval.
        ("計畫待確認".to_string(), "Claude 擬好計畫了，請檢查後核准".to_string())
    } else {
        let msg = json::extract_string(&input, "message");
        let title = match notification_type.as_str() {
            "permission_prompt" => "需要授權",
            "idle_prompt" => "等你回覆",
            "elicitation_dialog" => "MCP 詢問",
            _ => "需要你輸入",
        };
        let msg = if msg.is_empty() {
            "Claude 需要你的輸入".to_string()
        } else {
            msg
        };
        (title.to_string(), msg)
    };

    debug_log!(
        "Input mode, session: {}, tool: {}, type: {}, title: {}, message: {}",
        session_id, tool_name, notification_type, title, message
    );

    let extra = format!(
        " --input-mode --message {} --title {}",
        spawn::quote_arg(&message),
        spawn::quote_arg(&title)
    );
    spawn_display(&session_id, &extra, &input, debug);
    0
}

fn run_cleanup_mode() -> i32 {
    let input = json::read_stdin_json();
    let session_id = json::extract_string(&input, "session_id");

    if !session_id.is_empty() {
        debug_log!("Cleanup: deleting state for session {}", session_id);
        state::delete_state(&session_id);
    }
    0
}

fn run_notify_show_mode(args: &cli::Args) -> i32 {
    if args.session.is_empty() {
        debug_log!("No session ID for notify-show mode");
        return 1;
    }

    debug_log!("NotifyShow mode, session: {}", args.session);

    // 1. Load state from file
    let st = state::load_state(&args.session);
    debug_log!("Loaded state: HWND={:?}, RuntimeId={}, IconPath={}, Prompt={}",
        st.target_hwnd, st.wt_runtime_id, st.icon_path, st.user_prompt);

    // 2. Determine notification content (SPEC 14.1-14.2)
    // Title = project folder, status line = what happened, message = detail / your prompt.
    let (status, message) = if args.input_mode {
        let msg = if !args.message.is_empty() {
            args.message.clone()
        } else {
            "Claude 需要你的輸入".to_string()
        };
        let status = if !args.title.is_empty() {
            args.title.clone()
        } else {
            "需要你輸入".to_string()
        };
        (status, msg)
    } else {
        ("已完成".to_string(), st.user_prompt.clone())
    };
    let title = cwd_folder(&args.cwd); // may be empty: the header then shows "Claude Code" alone

    // 3. Sanitize message (SPEC 14.3)
    let message = sanitize_message(&message);
    debug_log!("Title: {}, Status: {}, Message: {}", title, status, message);

    // 4. Discover assets
    let discovered = assets::discover_assets();
    debug_log!("Sound: {:?}, Icon: {:?}", discovered.sound_file, discovered.default_icon_path);

    // 5. Extract icon from saved exe path
    let icon = assets::extract_icon(&st.icon_path);
    debug_log!("App icon: {:?}", icon);

    // 6. Play sound
    assets::play_sound(&discovered.sound_file);

    // 7. Show toast (blocks until closed)
    toast::show_toast(toast::ToastParams {
        title,
        status,
        message,
        cwd: args.cwd.clone(),
        input_mode: args.input_mode,
        icon,
        default_icon_path: discovered.default_icon_path.unwrap_or_default(),
        target_hwnd: st.target_hwnd,
        wt_hwnd: st.wt_hwnd,
        wt_runtime_id: st.wt_runtime_id,
    });

    // 8. Cleanup
    if !icon.is_invalid() {
        unsafe { let _ = windows::Win32::UI::WindowsAndMessaging::DestroyIcon(icon); }
    }

    0
}

/// Start the detached `--notify-show` process; `extra` is already-quoted args.
fn spawn_display(session_id: &str, extra: &str, input: &serde_json::Value, debug: bool) {
    let mut cmd = format!(
        "{} --notify-show --session {}{}",
        spawn::quote_arg(&exe_path()),
        spawn::quote_arg(session_id),
        extra
    );
    let cwd = json::extract_string(input, "cwd");
    if !cwd.is_empty() {
        cmd.push_str(&format!(" --cwd {}", spawn::quote_arg(&cwd)));
    }
    if debug {
        cmd.push_str(" --debug");
    }
    debug_log!("Spawning: {}", cmd);
    spawn::spawn_detached(&cmd);
}

/// Last path component, e.g. "D:\\0-icsc_project" -> "0-icsc_project".
fn cwd_folder(cwd: &str) -> String {
    let cwd = cwd.trim_end_matches(['\\', '/']);
    cwd.rsplit(['\\', '/']).next().unwrap_or("").to_string()
}

fn sanitize_message(msg: &str) -> String {
    // Collapse whitespace runs (incl. newlines) to single spaces.
    let s = msg.split_whitespace().collect::<Vec<_>>().join(" ");
    // Hard cap only; the toast itself ellipsizes to two lines.
    if s.chars().count() > 300 {
        s.chars().take(300).collect::<String>() + "…"
    } else {
        s
    }
}

/// Prompts Claude Code injects itself (`<task-notification>`, `<agent-message …>`) rather than ones the user typed.
// shortcut: matches known tags only; a new injected format gets saved like a typed prompt. Use a hook JSON source field if Claude Code adds one.
fn is_system_prompt(prompt: &str) -> bool {
    let p = prompt.trim_start();
    p.starts_with("<task-notification>") || p.starts_with("<agent-message ")
}

fn main() {
    // CRITICAL: Capture foreground window IMMEDIATELY (SPEC 3.1)
    let immediate_hwnd = unsafe { GetForegroundWindow() };

    unsafe {
        let hr = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        if hr.is_err() {
            debug_log!("CoInitializeEx failed: {:?}", hr);
        }
    }

    let args = cli::parse_args(std::env::args());
    log::init(args.debug);

    let exit_code = match args.mode {
        cli::Mode::Save => run_save_mode(immediate_hwnd),
        cli::Mode::Notify => run_notify_mode(args.debug),
        cli::Mode::Input => run_input_mode(args.debug),
        cli::Mode::NotifyShow => run_notify_show_mode(&args),
        cli::Mode::Cleanup => run_cleanup_mode(),
        cli::Mode::None => {
            print_usage();
            1
        }
    };

    unsafe {
        CoUninitialize();
    }
    std::process::exit(exit_code);
}

#[cfg(test)]
mod tests {
    use super::is_system_prompt;

    #[test]
    fn only_known_injected_tags_are_system_prompts() {
        assert!(is_system_prompt("<task-notification>\n<task-id>x</task-id>"));
        assert!(is_system_prompt("  <agent-message from=\"a\">\n[Subagent hand-back]"));
        assert!(!is_system_prompt("我如果已經在Herdr 點toast 會亂跳程式"));
        assert!(!is_system_prompt(""));
        assert!(!is_system_prompt("<div>Why is this overflowing?</div>"));
        assert!(!is_system_prompt("<https://example.com/api> returns 500"));
    }
}
