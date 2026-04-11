//! Rendering helper used by the CLI dispatcher.

use std::fmt::Display;

use serde::Serialize;

/// Render a value as a `String` for the CLI.
///
/// - `as_json = true`: pretty-printed JSON via `serde_json`
/// - `as_json = false`: the value's `Display` implementation
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
        format!("{value}\n")
    }
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
}
