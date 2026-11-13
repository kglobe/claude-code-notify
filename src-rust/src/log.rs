//! Debug logging system.
//!
//! When --debug is active, appends to <exe_dir>\debug.log.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// Log file path; None when --debug is off.
static LOG_PATH: OnceLock<Option<PathBuf>> = OnceLock::new();

/// Initialize the logger. Call once at startup.
pub fn init(debug: bool) {
    let log_path = debug.then(|| {
        let exe = std::env::current_exe().unwrap_or_default();
        exe.parent().unwrap_or(Path::new(".")).join("debug.log")
    });
    let _ = LOG_PATH.set(log_path);
    // Append, not truncate: the hook process and its --notify-show child share this file.
    if debug {
        log(&format!("=== ToastWindow pid {} ===", std::process::id()));
    }
}

/// Log a message. Only outputs if --debug was specified.
pub fn log(msg: &str) {
    // Debug output goes ONLY to the log file: AllocConsole() would flash a CMD window.
    let Some(Some(path)) = LOG_PATH.get() else { return };
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(f, "{}", msg);
    }
}

/// Convenience macro for formatted logging with [DEBUG] prefix.
#[macro_export]
macro_rules! debug_log {
    ($($arg:tt)*) => {
        $crate::log::log(&format!("[DEBUG] {}", format!($($arg)*)))
    };
}
