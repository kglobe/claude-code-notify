//! Detached child process spawning.
//!
//! Uses CreateProcessW with CREATE_NEW_PROCESS_GROUP | DETACHED_PROCESS
//! to spawn a child that outlives the parent.

use windows::Win32::System::Threading::*;
use windows::Win32::UI::WindowsAndMessaging::SW_HIDE;
use windows::core::PWSTR;

/// Spawn a detached child process with the given command line.
/// Returns true on success.
pub fn spawn_detached(cmd_line: &str) -> bool {
    let mut cmd_wide = crate::util::encode_wide(cmd_line);

    let si = STARTUPINFOW {
        cb: std::mem::size_of::<STARTUPINFOW>() as u32,
        dwFlags: STARTF_USESHOWWINDOW,
        wShowWindow: SW_HIDE.0 as u16,
        ..Default::default()
    };

    let mut pi = PROCESS_INFORMATION::default();

    let result = unsafe {
        CreateProcessW(
            None,
            Some(PWSTR(cmd_wide.as_mut_ptr())),
            None,
            None,
            false,
            CREATE_NEW_PROCESS_GROUP | DETACHED_PROCESS,
            None,
            None,
            &si,
            &mut pi,
        )
    };

    match result {
        Ok(_) => {
            unsafe {
                let _ = windows::Win32::Foundation::CloseHandle(pi.hProcess);
                let _ = windows::Win32::Foundation::CloseHandle(pi.hThread);
            }
            true
        }
        Err(_) => false,
    }
}

/// Quote one argument for a Windows command line (MSVC / CommandLineToArgvW rules).
pub fn quote_arg(s: &str) -> String {
    let mut out = String::from("\"");
    let mut backslashes = 0;
    for c in s.chars() {
        match c {
            '\\' => backslashes += 1,
            '"' => {
                // Backslashes before a quote are doubled, then the quote itself is escaped.
                out.extend(std::iter::repeat_n('\\', backslashes * 2 + 1));
                backslashes = 0;
            }
            _ => {
                out.extend(std::iter::repeat_n('\\', backslashes));
                backslashes = 0;
            }
        }
        if c != '\\' {
            out.push(c);
        }
    }
    // Trailing backslashes precede the closing quote, so they are doubled too.
    out.extend(std::iter::repeat_n('\\', backslashes * 2));
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::quote_arg;
    use windows::Win32::UI::Shell::CommandLineToArgvW;

    /// Split a command line the way the child process will.
    fn split(cmd: &str) -> Vec<String> {
        let wide = crate::util::encode_wide(cmd);
        let mut n = 0;
        unsafe {
            let argv = CommandLineToArgvW(windows::core::PCWSTR(wide.as_ptr()), &mut n);
            (0..n as usize).map(|i| (*argv.add(i)).to_string().unwrap()).collect()
        }
    }

    #[test]
    fn quoted_args_round_trip_through_argv_parsing() {
        for s in [
            r#"Use "C:\dir\" here?"#,
            r"C:\path\",
            r"C:\a\\",
            r#"say "hi""#,
            r#"back\"quote"#,
            "",
            "plain",
        ] {
            let cmd = format!("exe.exe --message {} --title {}", quote_arg(s), quote_arg("t"));
            let args = crate::cli::parse_args(split(&cmd));
            assert_eq!((args.message.as_str(), args.title.as_str()), (s, "t"), "cmd: {cmd}");
        }
    }
}
