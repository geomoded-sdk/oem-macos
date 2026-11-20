//! `oem-core`: the host coordinator's entry point.
//!
//! Subcommands map onto the boot sequence: `doctor` is the pre-boot check, `gpu` decides the
//! graphics backend, and `report` writes what both found. The boot orchestration itself lives
//! in the units under `host/systemd/`, which call these.

use std::process::ExitCode;

use oem_core::detect::{self, HostReport, HostRoot};
use oem_core::graphics::{self, Backend, Decision};
use oem_core::log::Log;

const USAGE: &str = "\
usage: oem-core <command>

commands:
  doctor    Check the host: virtualization, kvm, udmabuf, map count, GPUs
  gpu       Decide the graphics backend from what the drivers report
  report    Print the full host and graphics report as JSON
  boot      Run the boot sequence: check, decide graphics, start the guest
  guest-exited  Act on the guest having ended: power off, reboot, or restart it
";

fn main() -> ExitCode {
    let log = Log::from_env();
    let mut args = std::env::args().skip(1);
    let Some(command) = args.next() else {
        eprint!("{USAGE}");
        return ExitCode::from(2);
    };
    if args.next().is_some() {
        eprintln!("oem-core: {command} takes no arguments");
        return ExitCode::from(2);
    }
    match command.as_str() {
        "doctor" => doctor(&log),
        "gpu" => gpu(&log),
        "report" => report(&log),
        "boot" => boot(&log),
        "guest-exited" => guest_exited(&log),
        "-h" | "--help" | "help" => {
            print!("{USAGE}");
            ExitCode::SUCCESS
        }
        other => {
            eprintln!("oem-core: unknown command {other:?}\n\n{USAGE}");
            ExitCode::from(2)
        }
    }
}

/// Runs every host check and prints one line each. Exit code 1 where any check failed, so a
/// systemd unit's `ExecStartPre` stops the boot.
fn doctor(log: &Log) -> ExitCode {
    let root = HostRoot::system();
    let cpuinfo = std::fs::read_to_string(root.proc.join("cpuinfo")).unwrap_or_default();
    let max_map =
        std::fs::read_to_string(root.proc.join("sys/vm/max_map_count")).unwrap_or_default();
    let checks = vec![
        detect::cpu_virtualization(&cpuinfo),
        detect::kvm(&root),
        detect::udmabuf(&root),
        detect::max_map_count(&max_map),
    ];
    let report = HostReport {
        checks,
        gpus: detect::pci_gpus(&root),
    };
    for c in &report.checks {
        let mark = if c.ok { "ok  " } else { "FAIL" };
        println!("{mark}  {:<20} {}", c.name, c.detail);
    }
    for g in &report.gpus {
        println!(
            "gpu   {:<20} {} {} ({})",
            g.slot,
            g.vendor_name(),
            g.id(),
            g.driver.as_deref().unwrap_or("no driver")
        );
    }
    if report.ready() {
        log.info("doctor", "the host passes every check");
        ExitCode::SUCCESS
    } else {
        log.error(
            "doctor",
            &format!("{} check(s) failed", report.failures().len()),
        );
        ExitCode::FAILURE
    }
}

/// The graphics decision, on one line, for a human.
fn gpu(log: &Log) -> ExitCode {
    let decision = decide(log);
    println!("{}", decision.backend.summary());
    for r in &decision.unmet {
        println!("  unmet: {}", r.describe());
    }
    if let Some(l) = &decision.legacy {
        println!("  legacy: {} — {}", l.support.label(), l.reason);
    }
    // A CPU or software backend is not an error: the system boots, slowly. Only a missing
    // loader or device is worth a non-zero exit, because the installer refuses on those.
    if matches!(decision.backend, Backend::Software { .. }) {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

fn report(log: &Log) -> ExitCode {
    let decision = decide(log);
    let root = HostRoot::system();
    let cpuinfo = std::fs::read_to_string(root.proc.join("cpuinfo")).unwrap_or_default();
    let max_map =
        std::fs::read_to_string(root.proc.join("sys/vm/max_map_count")).unwrap_or_default();
    let checks = [
        detect::cpu_virtualization(&cpuinfo),
        detect::kvm(&root),
        detect::udmabuf(&root),
        detect::max_map_count(&max_map),
    ];
    let gpus = detect::pci_gpus(&root);
    let value = serde_json::json!({
        "version": oem_core::VERSION,
        "checks": checks.iter().map(|c| serde_json::json!({
            "name": c.name, "ok": c.ok, "detail": c.detail,
        })).collect::<Vec<_>>(),
        "gpus": gpus.iter().map(|g| serde_json::json!({
            "slot": g.slot, "vendor": g.vendor_name(), "id": g.id(), "driver": g.driver,
        })).collect::<Vec<_>>(),
        "graphics": {
            "backend": decision.backend.summary(),
            "device": decision.backend.device(),
            "accelerated": decision.backend.accelerates(),
            "unmet": decision.unmet.iter().map(|r| r.describe()).collect::<Vec<_>>(),
            "legacy": decision.legacy.as_ref().map(|l| serde_json::json!({
                "family": l.family.map(|f| f.name()),
                "support": l.support.label(),
                "reason": l.reason,
            })),
        },
    });
    println!(
        "{}",
        serde_json::to_string_pretty(&value).unwrap_or_default()
    );
    ExitCode::SUCCESS
}

/// The boot sequence, in the order the brief specifies.
///
/// Each step either succeeds or stops the boot with a reason in the journal. The guest is
/// started through the engine only after the host checks and the graphics decision agree that
/// it can be.
fn boot(log: &Log) -> ExitCode {
    let attempts_path = std::env::var_os("OEM_STATE_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::PathBuf::from("/var/lib/oem"));
    let mut attempts = oem_core::recovery::Attempts::load(attempts_path.join("attempts"));
    match attempts.mode() {
        oem_core::recovery::Mode::Maintenance => {
            log.warn("boot", "too many failed boots: taking the maintenance path");
            println!(
                "maintenance mode: the guest has failed to start {} times",
                attempts.count()
            );
            return ExitCode::from(3);
        }
        oem_core::recovery::Mode::Normal => {}
    }
    if let Err(e) = attempts.record_attempt() {
        // A counter that cannot be written is not fatal: the boot proceeds, and the risk is a
        // loop rather than a refusal to start.
        log.warn(
            "boot",
            &format!("the attempt counter could not be written: {e}"),
        );
    }
    let decision = decide(log);
    if matches!(decision.backend, Backend::Software { .. }) {
        log.error(
            "boot",
            "no usable graphics backend, so the guest cannot start",
        );
        for r in &decision.unmet {
            log.error("boot", &r.describe());
        }
        return ExitCode::FAILURE;
    }
    let config = match load_config(log) {
        Some(c) => c,
        None => return ExitCode::FAILURE,
    };
    let engine = oem_core::vm::Engine::default();
    match engine.state(&config.name) {
        Ok(s) if s.is_running() => {
            log.info("boot", "the guest is already running");
            let _ = attempts.record_success();
            return ExitCode::SUCCESS;
        }
        Ok(_) => {}
        Err(e) => {
            log.error("boot", &format!("the guest state could not be read: {e}"));
            return ExitCode::FAILURE;
        }
    }
    log.info(
        "boot",
        &format!(
            "starting the guest {} with {}",
            config.name,
            decision.backend.device()
        ),
    );
    match engine.start(&config) {
        Ok(()) => {
            let _ = attempts.record_success();
            log.info("boot", "the guest started");
            spawn_exit_monitor(&config, &attempts_path);
            ExitCode::SUCCESS
        }
        Err(e) => {
            log.error("boot", &format!("the guest did not start: {e}"));
            ExitCode::FAILURE
        }
    }
}

/// Watches the guest's QMP socket and writes why it ended.
///
/// `reims-ctl start` detaches QEMU, so the reason the guest stops is only observable while it
/// runs. The watcher waits for QEMU's socket, blocks on it until `SHUTDOWN` or the connection
/// ends, and leaves the answer in the state directory for `guest-exited`.
fn spawn_exit_monitor(config: &oem_core::vm::Config, state_dir: &std::path::Path) {
    let socket = config.qmp_socket();
    let state = state_dir.join("guest-exit");
    // A previous run's verdict must not be read as this run's.
    let _ = std::fs::remove_file(&state);
    let state_dir = state_dir.to_path_buf();
    std::thread::spawn(move || {
        // QEMU creates the socket a moment after `reims-ctl start` returns.
        for _ in 0..600 {
            if socket.exists() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        if !socket.exists() {
            // No socket, so no guest to watch: a guest that never appeared is a crash.
            let _ = std::fs::create_dir_all(&state_dir);
            let _ = std::fs::write(&state, "crashed");
            return;
        }
        let _ = oem_core::vm::qmp::watch_and_record(&socket, &state);
    });
}

/// What to do now that the guest has ended.
///
/// The event is `OEM_GUEST_EVENT` (`shutdown`, `restart`, `crashed`, `sleep`), set by the unit
/// that observed the exit. Without it the safe reading is a crash, which restarts the guest
/// rather than powering the machine off.
fn guest_exited(log: &Log) -> ExitCode {
    use oem_core::power::{decide, Action};
    let event = recorded_event();
    let host = host_power();
    let action = decide(event, host);
    log.info(
        "power",
        &format!("the guest ended ({event:?}); the action is {action:?}"),
    );
    match action {
        Action::PowerOff => {
            let _ = std::process::Command::new("systemctl")
                .arg("poweroff")
                .status();
            ExitCode::SUCCESS
        }
        Action::Reboot => {
            let _ = std::process::Command::new("systemctl")
                .arg("reboot")
                .status();
            ExitCode::SUCCESS
        }
        Action::RestartGuest => {
            let _ = std::process::Command::new("systemctl")
                .arg("start")
                .arg("oem-core.service")
                .status();
            ExitCode::SUCCESS
        }
        Action::Suspend => {
            // Never chosen today (see oem_core::power), but implemented rather than left as a
            // hole: if the policy ever returns it, it does the right thing.
            let _ = std::process::Command::new("systemctl")
                .arg("suspend")
                .status();
            ExitCode::SUCCESS
        }
        Action::None(reason) => {
            log.warn("power", reason);
            ExitCode::SUCCESS
        }
    }
}

/// What the guest did, from the file the QMP watcher wrote.
///
/// The watcher is the only thing that can tell a shutdown the user asked for from QEMU stopping
/// on its own, because the evidence is an event that exists only while the guest runs. A
/// missing file is read as a crash: restarting the guest is recoverable, powering the machine
/// off is not.
fn recorded_event() -> oem_core::power::GuestEvent {
    use oem_core::power::GuestEvent;
    let path = std::env::var_os("OEM_STATE_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::PathBuf::from("/var/lib/oem"))
        .join("guest-exit");
    match std::fs::read_to_string(&path)
        .ok()
        .as_deref()
        .map(str::trim)
    {
        Some("shutdown") => GuestEvent::ShutDown,
        Some("restart") => GuestEvent::Restart,
        Some("sleep") => GuestEvent::SleepRequested,
        _ => GuestEvent::Crashed,
    }
}

/// What this host can do about power, read from the kernel's own interface.
fn host_power() -> oem_core::power::HostPower {
    let states = std::fs::read_to_string("/sys/power/state").unwrap_or_default();
    oem_core::power::HostPower {
        can_suspend: states.split_whitespace().any(|s| s == "mem"),
        can_poweroff: std::path::Path::new("/sys/power/state").exists(),
        can_reboot: std::path::Path::new("/proc/sys/kernel").exists(),
    }
}

/// The guest's config, from `OEM_CONFIG` or `/etc/oem/vm.toml`.
fn load_config(log: &Log) -> Option<oem_core::vm::Config> {
    let path = std::env::var_os("OEM_CONFIG")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::PathBuf::from("/etc/oem/vm.toml"));
    let text = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(e) => {
            log.error(
                "boot",
                &format!("{} could not be read: {e}", path.display()),
            );
            return None;
        }
    };
    match toml::from_str::<oem_core::vm::Config>(&text) {
        Ok(c) => Some(c),
        Err(e) => {
            log.error(
                "boot",
                &format!("{} is not a valid guest config: {e}", path.display()),
            );
            None
        }
    }
}

/// The decision, from the probe. With the `vulkan` feature off there is no probe, and the
/// answer is honestly "no loader".
fn decide(log: &Log) -> Decision {
    #[cfg(all(target_os = "linux", feature = "vulkan"))]
    {
        let probe = graphics::probe::run();
        if let Some(e) = &probe.error {
            log.warn("gpu", e);
        }
        let intel = probe
            .devices
            .iter()
            .find(|d| d.vendor_id == 0x8086)
            .map(|d| (0x8086u16, d.device_id as u16));
        let decision = graphics::select(probe.loader, &probe.as_selection_input(), intel);
        log.info("gpu", &decision.backend.summary());
        decision
    }
    #[cfg(not(all(target_os = "linux", feature = "vulkan")))]
    {
        let _ = log;
        graphics::select(false, &[], None)
    }
}
