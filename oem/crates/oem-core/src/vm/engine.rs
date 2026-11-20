//! The VM engine: how OEM Core starts, stops and observes the guest.
//!
//! `reims-ctl` is the engine. This module is the seam: it builds the command, runs it, reads
//! the state back, and turns the engine's answer into a [`State`] OEM Core can act on. The
//! `Command` is injectable ([`Engine::with_program`]) so the argument building and the state
//! parsing are tested against a real executable, not a mock.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Deserialize;

use super::config::Config;

/// What the engine says about the guest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum State {
    Running {
        pid: u32,
    },
    Stopped,
    /// `reims-ctl` could not load the VM: its directory or settings are damaged.
    Broken(String),
}

impl State {
    pub fn is_running(&self) -> bool {
        matches!(self, State::Running { .. })
    }
}

/// One entry of `reims-ctl list --json`, whose shape is
/// `{"name": …, "dir": …, "state": "running"|"stopped"|"broken", "pid": n|null}`
/// (`crates/reims-ctl/src/cli.rs`, `fn list`).
#[derive(Debug, Deserialize)]
struct ListEntry {
    name: String,
    state: String,
    #[serde(default)]
    pid: Option<u32>,
}

/// The engine, and the program that implements it.
#[derive(Debug, Clone)]
pub struct Engine {
    program: PathBuf,
}

impl Default for Engine {
    fn default() -> Self {
        Engine {
            program: PathBuf::from("reims-ctl"),
        }
    }
}

impl Engine {
    pub fn new(program: impl Into<PathBuf>) -> Self {
        Engine {
            program: program.into(),
        }
    }

    /// Uses `program` instead of `reims-ctl`, for a test against a script.
    pub fn with_program(program: impl Into<PathBuf>) -> Self {
        Engine::new(program)
    }

    pub fn program(&self) -> &Path {
        &self.program
    }

    /// The state of the guest named `name`, from `reims-ctl list --json`.
    ///
    /// A guest that `list` does not mention is [`State::Stopped`]: `reims-ctl` keeps a registry
    /// of created VMs, and one that is absent has not been created, which for the boot means
    /// "nothing to start yet" rather than an error.
    pub fn state(&self, name: &str) -> Result<State, String> {
        let out = Command::new(&self.program)
            .args(["list", "--json"])
            .output()
            .map_err(|e| format!("{} could not be run: {e}", self.program.display()))?;
        if !out.status.success() {
            return Err(format!(
                "{} list --json failed: {}",
                self.program.display(),
                String::from_utf8_lossy(&out.stderr).trim()
            ));
        }
        Ok(parse_list(&String::from_utf8_lossy(&out.stdout), name))
    }

    /// Starts the guest. Returns the engine's combined output, which is the diagnostic if it
    /// fails.
    pub fn start(&self, config: &Config) -> Result<(), String> {
        config.validate()?;
        self.run(&config.start_args())
    }

    /// Stops the guest.
    pub fn stop(&self, config: &Config) -> Result<(), String> {
        self.run(&config.stop_args())
    }

    fn run(&self, args: &[String]) -> Result<(), String> {
        let out = Command::new(&self.program)
            .args(args)
            .output()
            .map_err(|e| format!("{} could not be run: {e}", self.program.display()))?;
        if out.status.success() {
            return Ok(());
        }
        let mut message = String::from_utf8_lossy(&out.stderr).trim().to_owned();
        if message.is_empty() {
            message = String::from_utf8_lossy(&out.stdout).trim().to_owned();
        }
        if message.is_empty() {
            message = format!("exited with {}", out.status);
        }
        Err(message)
    }
}

/// Reads `reims-ctl list --json` output for one guest.
///
/// Split out so the parsing is testable against captured output, including the malformed case
/// that a partial write or a future format change would produce.
pub fn parse_list(stdout: &str, name: &str) -> State {
    let Ok(entries) = serde_json::from_str::<Vec<ListEntry>>(stdout) else {
        // Not JSON at all: the engine is a different version or broken. Saying "stopped" would
        // hide that, so the guest is reported broken with the reason.
        return State::Broken("reims-ctl list --json did not return JSON".into());
    };
    let Some(entry) = entries.into_iter().find(|e| e.name == name) else {
        return State::Stopped;
    };
    match entry.state.as_str() {
        "running" => match entry.pid {
            Some(pid) => State::Running { pid },
            // "running" without a pid is inconsistent; treat the guest as stopped so a start
            // is attempted, which is the recoverable action.
            None => State::Stopped,
        },
        "broken" => State::Broken(format!("reims-ctl cannot load the VM {name:?}")),
        _ => State::Stopped,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn running_entry_carries_its_pid() {
        let out = r#"[{"name":"mac","dir":"/vms/mac","state":"running","pid":4321}]"#;
        assert_eq!(parse_list(out, "mac"), State::Running { pid: 4321 });
    }

    #[test]
    fn stopped_entry() {
        let out = r#"[{"name":"mac","dir":"/vms/mac","state":"stopped","pid":null}]"#;
        assert_eq!(parse_list(out, "mac"), State::Stopped);
    }

    #[test]
    fn an_absent_guest_is_stopped_not_an_error() {
        let out = r#"[{"name":"other","dir":"/vms/other","state":"running","pid":1}]"#;
        assert_eq!(parse_list(out, "mac"), State::Stopped);
        assert_eq!(parse_list("[]", "mac"), State::Stopped);
    }

    #[test]
    fn a_broken_entry_is_broken() {
        let out = r#"[{"name":"mac","dir":"/vms/mac","state":"broken","pid":null}]"#;
        assert!(matches!(parse_list(out, "mac"), State::Broken(_)));
    }

    /// Output that is not JSON must not be read as "stopped": that would hide a broken engine.
    #[test]
    fn non_json_is_broken_not_stopped() {
        assert!(matches!(
            parse_list("error: no such command", "mac"),
            State::Broken(_)
        ));
        assert!(matches!(parse_list("", "mac"), State::Broken(_)));
    }

    /// "running" with no pid is inconsistent; a start is the recoverable action.
    #[test]
    fn running_without_pid_is_treated_as_stopped() {
        let out = r#"[{"name":"mac","dir":"/vms/mac","state":"running"}]"#;
        assert_eq!(parse_list(out, "mac"), State::Stopped);
    }

    /// The engine runs a real program and reports its real exit status.
    #[test]
    fn engine_runs_the_program_and_reads_its_state() {
        let dir = std::env::temp_dir().join(format!("oem-engine-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let script = dir.join("fake-reims-ctl.sh");
        std::fs::write(
            &script,
            "#!/bin/sh\ncase \"$1\" in\n  list) echo '[{\"name\":\"mac\",\"dir\":\"/vms/mac\",\"state\":\"running\",\"pid\":99}]' ;;\n  *) exit 0 ;;\nesac\n",
        )
        .unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();

        let engine = Engine::with_program(&script);
        assert_eq!(engine.state("mac").unwrap(), State::Running { pid: 99 });
        assert_eq!(engine.state("absent").unwrap(), State::Stopped);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A failing program's stderr is the message the user gets.
    #[test]
    fn a_failing_start_surfaces_the_engine_message() {
        let dir = std::env::temp_dir().join(format!("oem-engine-fail-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let script = dir.join("failing.sh");
        std::fs::write(
            &script,
            "#!/bin/sh\necho 'the disk is missing' >&2\nexit 1\n",
        )
        .unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();

        let engine = Engine::with_program(&script);
        // A config whose paths exist passes validation, so the failure comes from the engine.
        let cfg_dir = dir.join("vm");
        std::fs::create_dir_all(&cfg_dir).unwrap();
        for f in [
            "disk.qcow2",
            "opencore.qcow2",
            "OVMF_CODE.fd",
            "OVMF_VARS.fd",
        ] {
            std::fs::write(cfg_dir.join(f), b"x").unwrap();
        }
        let config = Config {
            name: "mac".into(),
            cpus: 4,
            memory_mib: 8192,
            disk: cfg_dir.join("disk.qcow2"),
            opencore: cfg_dir.join("opencore.qcow2"),
            firmware_code: cfg_dir.join("OVMF_CODE.fd"),
            firmware_vars: cfg_dir.join("OVMF_VARS.fd"),
            graphics: super::super::config::Graphics::ReimsVgpu,
            display: Default::default(),
            network: true,
            audio: true,
        };
        let err = engine.start(&config).unwrap_err();
        assert!(err.contains("the disk is missing"), "{err}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A start of an invalid config never reaches the program.
    #[test]
    fn an_invalid_config_is_refused_before_the_program_runs() {
        let engine = Engine::with_program("/nonexistent/reims-ctl");
        let config = Config {
            name: "mac".into(),
            cpus: 4,
            memory_mib: 8192,
            disk: PathBuf::from("/nonexistent/disk.qcow2"),
            opencore: PathBuf::from("/nonexistent/opencore.qcow2"),
            firmware_code: PathBuf::from("/nonexistent/OVMF_CODE.fd"),
            firmware_vars: PathBuf::from("/nonexistent/OVMF_VARS.fd"),
            graphics: super::super::config::Graphics::ReimsVgpu,
            display: Default::default(),
            network: true,
            audio: true,
        };
        let err = engine.start(&config).unwrap_err();
        assert!(err.contains("missing"), "{err}");
    }

    /// A program that does not exist is reported as such, not as a generic failure.
    #[test]
    fn a_missing_program_is_named() {
        let engine = Engine::with_program("/nonexistent/reims-ctl");
        let err = engine.state("mac").unwrap_err();
        assert!(err.contains("could not be run"), "{err}");
    }
}
