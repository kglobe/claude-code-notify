//! CLI argument parsing for ToastWindow.
//!
//! Modes: --save, --notify, --input, --notify-show, --cleanup
//! Flags: --debug/-d, --input-mode, --session <val>, --message <val>, --title <val>

#[derive(Debug, PartialEq)]
pub enum Mode {
    Save,
    Notify,
    Input,
    NotifyShow,
    Cleanup,
    None,
}

#[derive(Debug)]
pub struct Args {
    pub mode: Mode,
    pub debug: bool,
    pub input_mode: bool,
    pub session: String,
    pub message: String,
    pub title: String,
    pub cwd: String,
}

/// Parse a full argv (program name first).
pub fn parse_args(args: impl IntoIterator<Item = String>) -> Args {
    let mut result = Args {
        mode: Mode::None,
        debug: false,
        input_mode: false,
        session: String::new(),
        message: String::new(),
        title: String::new(),
        cwd: String::new(),
    };

    let mut it = args.into_iter().skip(1);
    while let Some(arg) = it.next() {
        let slot = match arg.as_str() {
            "--save" => { result.mode = Mode::Save; continue; }
            "--notify" => { result.mode = Mode::Notify; continue; }
            "--input" => { result.mode = Mode::Input; continue; }
            "--notify-show" => { result.mode = Mode::NotifyShow; continue; }
            "--cleanup" => { result.mode = Mode::Cleanup; continue; }
            "--debug" | "-d" => { result.debug = true; continue; }
            "--input-mode" => { result.input_mode = true; continue; }
            "--session" => &mut result.session,
            "--message" => &mut result.message,
            "--title" => &mut result.title,
            "--cwd" => &mut result.cwd,
            _ => continue,
        };
        *slot = it.next().unwrap_or_default();
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> Args {
        parse_args(std::iter::once("exe").chain(args.iter().copied()).map(String::from))
    }

    #[test]
    fn parses_mode_flags_and_values() {
        let a = parse(&["--notify-show", "--input-mode", "--session", "s1", "--message", "m", "--title", "t", "--cwd", r"C:\x", "-d"]);
        assert_eq!(a.mode, Mode::NotifyShow);
        assert!(a.input_mode && a.debug);
        assert_eq!((a.session.as_str(), a.message.as_str(), a.title.as_str(), a.cwd.as_str()), ("s1", "m", "t", r"C:\x"));
    }

    #[test]
    fn missing_value_and_unknown_args_are_ignored() {
        let a = parse(&["--bogus", "--save", "--session"]);
        assert_eq!(a.mode, Mode::Save);
        assert_eq!(a.session, "");
        assert_eq!(parse(&[]).mode, Mode::None);
    }
}
