//! Watching QMP for why the guest ended.
//!
//! `reims-ctl start` detaches QEMU and returns once QMP answers, so it never learns why the
//! guest later stopped. The only reliable evidence is QEMU's own `SHUTDOWN` event, which
//! carries `guest: true` when macOS asked for the shutdown and `false` when QEMU decided
//! itself. That event is only observable while QEMU runs, and the socket is gone afterwards.
//!
//! So OEM Core watches the socket for as long as the guest runs and writes the reason down. The
//! unit that reacts to the guest ending then reads a fact instead of guessing.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::time::Duration;

/// Why QEMU stopped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShutdownReason {
    /// macOS asked: the user chose Shut Down or Restart.
    Guest,
    /// QEMU stopped on its own: a host-side error, or the guest's firmware failed.
    Host,
}

impl ShutdownReason {
    /// The word written to the state file and read back by `guest-exited`.
    pub fn as_str(&self) -> &'static str {
        match self {
            ShutdownReason::Guest => "shutdown",
            ShutdownReason::Host => "crashed",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s.trim() {
            "shutdown" => Some(ShutdownReason::Guest),
            "crashed" => Some(ShutdownReason::Host),
            _ => None,
        }
    }
}

/// A QMP event OEM Core cares about.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    /// The guest is stopping; `reason` says who asked.
    Shutdown(ShutdownReason),
    /// Anything else: `RESET`, `STOP`, the greeting, command replies.
    Other,
}

/// Reads one line of QMP output.
///
/// QMP is one JSON object per line. The `SHUTDOWN` event is
/// `{"event":"SHUTDOWN","data":{"guest":true,"reason":"guest-shutdown"}}`; a missing `guest`
/// field is read as `false`, because a shutdown nobody asked for is the safer assumption —
/// it restarts the guest rather than powering the machine off.
pub fn parse_event(line: &str) -> Event {
    let line = line.trim();
    if line.is_empty() || !line.starts_with('{') {
        return Event::Other;
    }
    let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else {
        return Event::Other;
    };
    if v["event"].as_str() != Some("SHUTDOWN") {
        return Event::Other;
    }
    let guest = v["data"]["guest"].as_bool().unwrap_or(false);
    Event::Shutdown(if guest {
        ShutdownReason::Guest
    } else {
        ShutdownReason::Host
    })
}

/// Whether a read error means QEMU's side of the socket went away, rather than that the
/// monitor is broken.
fn is_connection_ended(e: &std::io::Error) -> bool {
    use std::io::ErrorKind;
    matches!(
        e.kind(),
        ErrorKind::ConnectionReset
            | ErrorKind::BrokenPipe
            | ErrorKind::UnexpectedEof
            | ErrorKind::ConnectionAborted
    )
}

/// Connects to `socket`, negotiates capabilities, and watches until a `SHUTDOWN` event or the
/// connection ends.
///
/// Returns the reason when one is learned. A guest that never reaches `SHUTDOWN` — QEMU killed,
/// the host powered off — returns `Ok(None)`, which the caller treats as a crash.
pub fn watch(socket: &Path) -> Result<Option<ShutdownReason>, String> {
    let stream = UnixStream::connect(socket).map_err(|e| format!("{}: {e}", socket.display()))?;
    // A guest can run for days, so there is no read timeout: the reader blocks until QEMU
    // writes, which is what a monitor should do.
    stream
        .set_write_timeout(Some(Duration::from_secs(10)))
        .map_err(|e| format!("{}: {e}", socket.display()))?;
    let mut writer = stream
        .try_clone()
        .map_err(|e| format!("{}: {e}", socket.display()))?;
    let reader = BufReader::new(stream);

    // QMP requires this before any event is delivered.
    writer
        .write_all(b"{\"execute\":\"qmp_capabilities\"}\n")
        .map_err(|e| format!("{}: {e}", socket.display()))?;

    for line in reader.lines() {
        match line {
            Ok(line) => {
                if let Event::Shutdown(reason) = parse_event(&line) {
                    return Ok(Some(reason));
                }
            }
            // A guest killed outright (SIGKILL, host power loss) resets or closes the
            // connection without a SHUTDOWN event. That is a crash, not a monitoring failure,
            // so it is reported as "no reason learned" and the caller decides.
            Err(e) if is_connection_ended(&e) => return Ok(None),
            Err(e) => return Err(format!("{}: {e}", socket.display())),
        }
    }
    Ok(None)
}

/// Watches `socket` and writes what it learned to `state`, so the unit that reacts to the guest
/// ending can read it. A missing reason is written as `crashed`.
pub fn watch_and_record(socket: &Path, state: &Path) -> Result<(), String> {
    let reason = watch(socket)?.unwrap_or(ShutdownReason::Host);
    if let Some(parent) = state.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
    }
    std::fs::write(state, reason.as_str()).map_err(|e| format!("{}: {e}", state.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::BufWriter;

    #[test]
    fn a_guest_shutdown_is_recognised() {
        let line = r#"{"timestamp":{"seconds":1,"microseconds":0},"event":"SHUTDOWN","data":{"guest":true,"reason":"guest-shutdown"}}"#;
        assert_eq!(parse_event(line), Event::Shutdown(ShutdownReason::Guest));
    }

    /// A host-side shutdown must not be mistaken for the user choosing Shut Down.
    #[test]
    fn a_host_shutdown_is_not_a_guest_shutdown() {
        let line = r#"{"event":"SHUTDOWN","data":{"guest":false,"reason":"host-signal"}}"#;
        assert_eq!(parse_event(line), Event::Shutdown(ShutdownReason::Host));
    }

    /// A SHUTDOWN with no `guest` field is read as a crash: the safe direction, because a
    /// crash restarts the guest rather than powering the machine off.
    #[test]
    fn a_shutdown_without_the_field_is_a_crash() {
        assert_eq!(
            parse_event(r#"{"event":"SHUTDOWN"}"#),
            Event::Shutdown(ShutdownReason::Host)
        );
    }

    #[test]
    fn other_messages_are_ignored() {
        for line in [
            r#"{"QMP":{"version":{"qemu":{"major":11}},"capabilities":[]}}"#,
            r#"{"return":{}}"#,
            r#"{"event":"RESET","data":{}}"#,
            r#"{"event":"STOP"}"#,
            "",
            "not json",
        ] {
            assert_eq!(parse_event(line), Event::Other, "{line}");
        }
    }

    #[test]
    fn reason_words_round_trip() {
        for r in [ShutdownReason::Guest, ShutdownReason::Host] {
            assert_eq!(ShutdownReason::parse(r.as_str()), Some(r));
        }
        assert_eq!(ShutdownReason::parse("poweroff"), None);
    }

    /// The watcher against a real Unix socket, fed the greeting and then a shutdown event in
    /// the shape QEMU writes. This exercises the socket, the capability negotiation and the
    /// parser together, rather than the parser alone.
    #[test]
    fn watch_reads_a_real_socket_until_the_shutdown_event() {
        let dir = std::env::temp_dir().join(format!("oem-qmp-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let sock = dir.join("qmp.sock");
        let listener = std::os::unix::net::UnixListener::bind(&sock).unwrap();

        let peer = std::thread::spawn(move || {
            let (mut s, _) = listener.accept().unwrap();
            // The client must negotiate first; read its command.
            let mut buf = [0u8; 64];
            let _ = std::io::Read::read(&mut s, &mut buf);
            let mut w = BufWriter::new(s.try_clone().unwrap());
            w.write_all(b"{\"QMP\":{\"version\":{},\"capabilities\":[]}}\n")
                .unwrap();
            w.write_all(b"{\"return\":{}}\n").unwrap();
            w.write_all(b"{\"event\":\"RESUME\"}\n").unwrap();
            w.write_all(
                b"{\"event\":\"SHUTDOWN\",\"data\":{\"guest\":true,\"reason\":\"guest-shutdown\"}}\n",
            )
            .unwrap();
            w.flush().unwrap();
        });

        let reason = watch(&sock).expect("watch");
        assert_eq!(reason, Some(ShutdownReason::Guest));
        peer.join().unwrap();
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A connection that ends without a SHUTDOWN event yields no reason, which the caller
    /// treats as a crash.
    #[test]
    fn a_connection_that_ends_early_yields_no_reason() {
        let dir = std::env::temp_dir().join(format!("oem-qmp-end-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let sock = dir.join("qmp.sock");
        let listener = std::os::unix::net::UnixListener::bind(&sock).unwrap();
        std::thread::spawn(move || {
            let (s, _) = listener.accept().unwrap();
            drop(s);
        });
        assert_eq!(watch(&sock).expect("watch"), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_missing_socket_is_an_error_not_a_crash_verdict() {
        let err = watch(Path::new("/nonexistent/qmp.sock")).unwrap_err();
        assert!(err.contains("qmp.sock"), "{err}");
    }

    /// `watch_and_record` writes what it learned, and a missing reason becomes `crashed`.
    #[test]
    fn watch_and_record_writes_the_reason() {
        let dir = std::env::temp_dir().join(format!("oem-qmp-rec-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let sock = dir.join("qmp.sock");
        let state = dir.join("nested/guest-exit");
        let listener = std::os::unix::net::UnixListener::bind(&sock).unwrap();
        std::thread::spawn(move || {
            let (mut s, _) = listener.accept().unwrap();
            let mut buf = [0u8; 64];
            let _ = std::io::Read::read(&mut s, &mut buf);
            let mut w = BufWriter::new(s.try_clone().unwrap());
            w.write_all(b"{\"event\":\"SHUTDOWN\",\"data\":{\"guest\":true}}\n")
                .unwrap();
            w.flush().unwrap();
        });
        watch_and_record(&sock, &state).expect("record");
        assert_eq!(std::fs::read_to_string(&state).unwrap().trim(), "shutdown");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
