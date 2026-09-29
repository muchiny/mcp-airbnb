//! Rendering helper used by the CLI dispatcher.

use std::fmt::{Display, Write as _};

use serde::Serialize;

/// Render a value as a `String` for the CLI.
///
/// - `as_json = true`: pretty-printed JSON via `serde_json`
/// - `as_json = false`: the value's `Display` implementation, with terminal
///   control characters escaped (see [`sanitize_for_terminal`])
///
/// Returns a `String` (rather than writing to stdout directly) so that
/// `dispatch` remains testable without capturing stdout.
pub fn render<T: Serialize + Display>(value: &T, as_json: bool) -> String {
    if as_json {
        match serde_json::to_string_pretty(value) {
            Ok(s) => format!("{s}\n"),
            Err(e) => format!("error serializing to JSON: {e}\n"),
        }
    } else {
        format!("{}\n", sanitize_for_terminal(&value.to_string()))
    }
}

/// Write the rendered output and flush it.
///
/// Rust ignores SIGPIPE, so writing to a pipe whose reader has exited
/// (`airbnb … | head`) returns `BrokenPipe`. `print!` would panic, and the
/// release profile's `panic = "abort"` would then kill the process. A
/// closed pipe is a normal way to stop reading, so it counts as success;
/// other I/O errors are returned.
pub fn write_output<W: std::io::Write>(out: &mut W, rendered: &str) -> std::io::Result<()> {
    match out
        .write_all(rendered.as_bytes())
        .and_then(|()| out.flush())
    {
        Err(e) if e.kind() == std::io::ErrorKind::BrokenPipe => Ok(()),
        other => other,
    }
}

/// Make terminal control characters visible in human-readable output.
///
/// Host- and guest-written text (listing names, descriptions, reviews, host
/// bios) can carry ESC/OSC sequences that move the cursor, rewrite earlier
/// lines, create hyperlinks or write the clipboard (OSC 52). Every C0/C1
/// control character except `\n` and `\t`, and the Unicode bidirectional
/// controls used for "Trojan Source" spoofing, is replaced by its visible
/// `\u{..}` escape.
pub fn sanitize_for_terminal(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        if (c.is_control() && c != '\n' && c != '\t') || is_bidi_control(c) {
            let _ = write!(out, "\\u{{{:x}}}", u32::from(c));
        } else {
            out.push(c);
        }
    }
    out
}

fn is_bidi_control(c: char) -> bool {
    matches!(
        c,
        '\u{061C}' | '\u{200E}' | '\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}'
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(serde::Serialize)]
    struct Dummy {
        value: i32,
    }

    impl std::fmt::Display for Dummy {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "value={}", self.value)
        }
    }

    #[test]
    fn render_text_uses_display() {
        let out = render(&Dummy { value: 42 }, false);
        assert!(out.contains("value=42"));
        assert!(out.ends_with('\n'));
    }

    #[test]
    fn render_json_is_pretty() {
        let out = render(&Dummy { value: 42 }, true);
        let parsed: serde_json::Value = serde_json::from_str(out.trim()).unwrap();
        assert_eq!(parsed["value"], 42);
    }

    #[test]
    fn sanitize_escapes_osc52_and_escape_sequences() {
        let out = sanitize_for_terminal("a\u{1b}]52;c;ZXZpbA==\u{7}b");
        assert_eq!(out, "a\\u{1b}]52;c;ZXZpbA==\\u{7}b");
    }

    #[test]
    fn sanitize_keeps_newlines_tabs_and_accents() {
        assert_eq!(
            sanitize_for_terminal("Crème brûlée\n\tok"),
            "Crème brûlée\n\tok"
        );
    }

    #[test]
    fn sanitize_escapes_carriage_return_c1_and_bidi_overrides() {
        assert_eq!(
            sanitize_for_terminal("x\ry\u{9b}z\u{202e}"),
            "x\\u{d}y\\u{9b}z\\u{202e}"
        );
    }

    #[derive(serde::Serialize)]
    struct Hostile {
        text: String,
    }

    impl std::fmt::Display for Hostile {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "{}", self.text)
        }
    }

    #[test]
    fn render_sanitizes_text_but_not_json() {
        let value = Hostile {
            text: "rating 5\u{1b}[1A\u{1b}[2Krating 1".into(),
        };
        let text = render(&value, false);
        assert!(!text.contains('\u{1b}'), "{text}");
        assert!(text.contains("\\u{1b}[1A"), "{text}");
        let json = render(&value, true);
        let parsed: serde_json::Value = serde_json::from_str(json.trim()).unwrap();
        assert_eq!(parsed["text"], "rating 5\u{1b}[1A\u{1b}[2Krating 1");
    }

    struct ClosedPipe;

    impl std::io::Write for ClosedPipe {
        fn write(&mut self, _buf: &[u8]) -> std::io::Result<usize> {
            Err(std::io::Error::from(std::io::ErrorKind::BrokenPipe))
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    struct FailingDisk;

    impl std::io::Write for FailingDisk {
        fn write(&mut self, _buf: &[u8]) -> std::io::Result<usize> {
            Err(std::io::Error::other("disk full"))
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn broken_pipe_is_not_an_error() {
        let big = "x".repeat(100_000);
        assert!(write_output(&mut ClosedPipe, &big).is_ok());
    }

    #[test]
    fn other_write_errors_are_reported() {
        assert!(write_output(&mut FailingDisk, "data").is_err());
    }

    #[test]
    fn write_output_writes_everything() {
        let mut buf = Vec::new();
        write_output(&mut buf, "hello\n").unwrap();
        assert_eq!(buf, b"hello\n");
    }
}
