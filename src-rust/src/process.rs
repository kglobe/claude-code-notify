//! Process tree walking to find the caller application.
//!
//! Walks up the process tree (max 10 levels) to find the first non-shell process,
//! which is used to extract an icon for the toast notification.

use windows::Win32::Foundation::*;
use windows::Win32::System::Diagnostics::ToolHelp::*;
use windows::Win32::System::Threading::*;

/// Shell/runtime processes to skip (exact match, case-insensitive).
const SKIP_LIST: &[&str] = &[
    // Windows shells
    "cmd", "powershell", "pwsh", "conhost", "explorer",
    // Unix shells (WSL/Git Bash)
    "bash", "zsh", "fish", "sh", "wsl", "mintty",
    // Git
    "git", "git-bash",
    // JavaScript/TypeScript runtimes
    "node", "deno", "bun", "npx", "ts-node", "npm", "yarn", "pnpm",
    // Python
    "python", "python3", "uv", "pip", "poetry", "pdm",
    // Other languages
    "ruby", "java", "dotnet", "php", "go", "cargo", "rustc", "perl", "lua",
    // Claude CLI
    "claude",
    // Remote/containers
    "ssh", "docker", "podman",
];

/// Known application processes (immediate match).
/// Match is exact OR prefix-dash (e.g. "code" matches "code-insiders").
const KNOWN_APPS: &[&str] = &[
    // VS Code variants
    "code", "code-insiders", "codium", "cursor", "windsurf",
    // JetBrains IDEs
    "idea", "idea64", "webstorm", "webstorm64",
    "pycharm", "pycharm64", "rider", "rider64",
    "goland", "goland64", "clion", "clion64",
    // Terminal emulators
    "windowsterminal", "wt", "conemu", "conemu64",
    "tabby", "wezterm", "wezterm-gui",
];

/// Find the caller application's exe path by walking up the process tree.
pub fn find_caller_exe_path() -> String {
    let parents = parent_map();
    pick_caller(
        unsafe { GetCurrentProcessId() },
        |pid| parents.get(&pid).copied().unwrap_or(0),
        get_process_exe_path,
    )
}

/// Walk up at most 10 ancestors; return the first known app or non-shell exe.
fn pick_caller(start: u32, parent_of: impl Fn(u32) -> u32, exe_of: impl Fn(u32) -> String) -> String {
    let mut pid = start;

    for _ in 0..10 {
        let parent_pid = parent_of(pid);
        if parent_pid == 0 || parent_pid == pid {
            break;
        }

        let exe_path = exe_of(parent_pid);
        if exe_path.is_empty() {
            pid = parent_pid;
            continue;
        }

        let exe_name = file_name_without_ext(&exe_path).to_lowercase();

        // Check known apps first (prefix-dash matching)
        if is_known_app(&exe_name) {
            return exe_path;
        }

        // Check skip list (exact match)
        if SKIP_LIST.contains(&exe_name.as_str()) {
            pid = parent_pid;
            continue;
        }

        // Unknown but valid process - use it
        return exe_path;
    }

    String::new()
}

fn is_known_app(exe_name: &str) -> bool {
    for app in KNOWN_APPS {
        if exe_name == *app || exe_name.starts_with(&format!("{}-", app)) {
            return true;
        }
    }
    false
}

/// pid -> parent pid for every process, from a single Toolhelp snapshot.
fn parent_map() -> std::collections::HashMap<u32, u32> {
    let mut map = std::collections::HashMap::new();
    unsafe {
        let Ok(snapshot) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) else {
            return map;
        };

        let mut entry = PROCESSENTRY32W {
            dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
            ..Default::default()
        };

        if Process32FirstW(snapshot, &mut entry).is_ok() {
            loop {
                map.insert(entry.th32ProcessID, entry.th32ParentProcessID);
                if Process32NextW(snapshot, &mut entry).is_err() {
                    break;
                }
            }
        }

        let _ = CloseHandle(snapshot);
    }
    map
}

fn get_process_exe_path(pid: u32) -> String {
    unsafe {
        let handle = match OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) {
            Ok(h) => h,
            Err(_) => return String::new(),
        };

        let mut buf = [0u16; 1024];
        let mut size = buf.len() as u32;
        let result = QueryFullProcessImageNameW(
            handle,
            PROCESS_NAME_WIN32,
            windows::core::PWSTR(buf.as_mut_ptr()),
            &mut size,
        );
        let _ = CloseHandle(handle);

        match result {
            Ok(_) => String::from_utf16_lossy(&buf[..size as usize]),
            Err(_) => String::new(),
        }
    }
}

fn file_name_without_ext(path: &str) -> String {
    let name = path
        .rsplit(['\\', '/'])
        .next()
        .unwrap_or(path);
    match name.rfind('.') {
        Some(pos) => name[..pos].to_string(),
        None => name.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::pick_caller;

    /// Chain: pid i's parent is i+1; exe of pid i is names[i].
    fn walk(names: &[&str]) -> String {
        pick_caller(0, |p| p + 1, |p| names.get(p as usize).map(|s| s.to_string()).unwrap_or_default())
    }

    #[test]
    fn skips_shells_and_returns_known_or_unknown_app() {
        assert_eq!(walk(&["", r"C:\bash.exe", r"C:\node.EXE", r"C:\Code-Insiders.exe"]), r"C:\Code-Insiders.exe");
        assert_eq!(walk(&["", r"C:\cmd.exe", "", r"C:\foo.exe"]), r"C:\foo.exe");
        assert_eq!(walk(&["", r"C:\cmd.exe", r"C:\cmd.exe"]), "");
    }

    #[test]
    fn stops_at_root_self_parent_and_depth_limit() {
        assert_eq!(pick_caller(5, |_| 0, |_| "x.exe".into()), "");
        assert_eq!(pick_caller(5, |p| p, |_| "x.exe".into()), "");
        let deep: Vec<&str> = std::iter::repeat_n(r"C:\cmd.exe", 11).chain([r"C:\app.exe"]).collect();
        assert_eq!(walk(&deep), "");
    }
}
