//! Structured logging: one JSON object per line, or a human line when a terminal reads it.
//!
//! The log is the only diagnostic channel OEM-macOS has in normal operation, because no desktop
//! and no terminal are on screen. Every subsystem reports through [`Log`], so a failure that
//! happens after the display is handed to macOS is still readable from the journal.

use std::fmt::Write as _;
use std::io::{IsTerminal, Write};

pub enum Level {
    Debug,
    Info,
    Warn,
    Error,
}

impl Level {
    fn as_str(&self) -> &'static str {
        match self {
            Level::Debug => "debug",
            Level::Info => "info",
            Level::Warn => "warn",
            Level::Error => "error",
        }
    }

    fn rank(&self) -> u8 {
        match self {
            Level::Debug => 0,
            Level::Info => 1,
            Level::Warn => 2,
            Level::Error => 3,
        }
    }
}

pub struct Log {
    min: Level,
    json: bool,
}

impl Log {
    pub fn new(min: Level, json: bool) -> Self {
        Log { min, json }
    }

    /// The level from `OEM_LOG` (`debug`, `info`, `warn`, `error`), defaulting to info. JSON is
    /// for a reader that parses (the journal); a terminal gets the human line.
    pub fn from_env() -> Self {
        let level = match std::env::var("OEM_LOG").ok().as_deref() {
            Some("debug") => Level::Debug,
            Some("warn") => Level::Warn,
            Some("error") => Level::Error,
            _ => Level::Info,
        };
        let json = std::env::var_os("OEM_LOG_JSON").is_some() || !std::io::stderr().is_terminal();
        Log::new(level, json)
    }

    pub fn enabled(&self, level: &Level) -> bool {
        level.rank() >= self.min.rank()
    }

    pub fn log(&self, level: Level, target: &str, message: &str) {
        if !self.enabled(&level) {
            return;
        }
        let mut err = std::io::stderr().lock();
        if self.json {
            // Hand-built so logging needs no serialiser and cannot fail on an odd string.
            let _ = writeln!(
                err,
                "{{\"level\":\"{}\",\"target\":\"{}\",\"msg\":\"{}\"}}",
                level.as_str(),
                escape(target),
                escape(message)
            );
        } else {
            let _ = writeln!(err, "oem {}: {}: {}", level.as_str(), target, message);
        }
    }

    pub fn debug(&self, target: &str, message: &str) {
        self.log(Level::Debug, target, message);
    }

    pub fn info(&self, target: &str, message: &str) {
        self.log(Level::Info, target, message);
    }

    pub fn warn(&self, target: &str, message: &str) {
        self.log(Level::Warn, target, message);
    }

    pub fn error(&self, target: &str, message: &str) {
        self.log(Level::Error, target, message);
    }
}

/// `s` with the characters that would break a JSON string escaped: the quote, the backslash, and
/// every control character, which becomes `\u00XX`.
pub fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escape_leaves_plain_text() {
        assert_eq!(escape("the disk is missing"), "the disk is missing");
    }

    #[test]
    fn escape_quotes_and_backslash() {
        assert_eq!(escape(r#"a"b\c"#), r#"a\"b\\c"#);
    }

    #[test]
    fn escape_control_characters() {
        assert_eq!(escape("a\nb\tc"), "a\\nb\\tc");
        assert_eq!(escape("\u{1}"), "\\u0001");
    }

    /// The line the logger writes, with a path that holds a quote, is still one JSON object.
    #[test]
    fn logged_line_parses_as_json() {
        let line = format!(
            "{{\"level\":\"error\",\"target\":\"{}\",\"msg\":\"{}\"}}",
            escape("vm"),
            escape(r#"the disk "/vms/a b.qcow2" is missing"#)
        );
        let value: serde_json::Value = serde_json::from_str(&line).expect("valid JSON");
        assert_eq!(value["level"], "error");
        assert_eq!(value["msg"], r#"the disk "/vms/a b.qcow2" is missing"#);
    }

    #[test]
    fn level_filters_lower_ranks() {
        let log = Log::new(Level::Warn, true);
        assert!(!log.enabled(&Level::Debug));
        assert!(!log.enabled(&Level::Info));
        assert!(log.enabled(&Level::Warn));
        assert!(log.enabled(&Level::Error));
    }
}
