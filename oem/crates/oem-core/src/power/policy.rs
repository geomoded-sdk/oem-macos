//! What to do when the guest or the host asks for a power change.
//!
//! This is a pure function of two facts — what the guest did, and what the host can do — so
//! every combination is tested without a VM, a D-Bus connection, or a machine that can suspend.

/// What the guest did, as OEM Core observes it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GuestEvent {
    /// macOS asked to shut down, and QEMU exited cleanly.
    ShutDown,
    /// macOS asked to restart.
    Restart,
    /// QEMU exited without being asked to: a crash.
    Crashed,
    /// macOS asked to sleep. ACPI S3 is disabled, so this arrives as a dark wake at most.
    SleepRequested,
}

/// What the host can do about power.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HostPower {
    /// Whether the host can suspend (`/sys/power/state` lists `mem`).
    pub can_suspend: bool,
    /// Whether the host can power itself off (`/sys/power/state` or the ACPI path exists).
    pub can_poweroff: bool,
    /// Whether the host can reboot.
    pub can_reboot: bool,
}

/// What OEM Core should do next.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// Power the host off.
    PowerOff,
    /// Reboot the host.
    Reboot,
    /// Suspend the host. Only ever with the guest already stopped.
    Suspend,
    /// Start the guest again.
    RestartGuest,
    /// Do nothing, and say why in the journal.
    None(&'static str),
}

/// Decides the action.
///
/// The rules, and their reasons:
///
/// 1. A clean guest shutdown powers the host off, because the user shut the machine down: this
///    is a dedicated macOS computer, so "shut down" means the whole computer.
/// 2. A guest restart reboots the host, for the same reason.
/// 3. A crash restarts the guest once, not the host: a guest crash must not look like a host
///    fault, and an unattended host that powers off on a guest bug is a bricked machine.
/// 4. A sleep request does **not** suspend the host. ACPI S3 is disabled in this platform, so
///    the guest cannot be resumed; suspending the host would strand the user at a black screen.
pub fn decide(event: GuestEvent, host: HostPower) -> Action {
    match event {
        GuestEvent::ShutDown => {
            if host.can_poweroff {
                Action::PowerOff
            } else {
                Action::None("the guest shut down, but this host cannot power itself off")
            }
        }
        GuestEvent::Restart => {
            if host.can_reboot {
                Action::Reboot
            } else {
                Action::None("the guest asked to restart, but this host cannot reboot")
            }
        }
        GuestEvent::Crashed => Action::RestartGuest,
        GuestEvent::SleepRequested => Action::None(
            "the guest asked to sleep; ACPI S3 is disabled in this platform, so the host stays \
             awake and macOS keeps running",
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn host(can_suspend: bool, can_poweroff: bool, can_reboot: bool) -> HostPower {
        HostPower {
            can_suspend,
            can_poweroff,
            can_reboot,
        }
    }

    #[test]
    fn a_guest_shutdown_powers_the_computer_off() {
        assert_eq!(
            decide(GuestEvent::ShutDown, host(true, true, true)),
            Action::PowerOff
        );
    }

    #[test]
    fn a_guest_restart_reboots_the_computer() {
        assert_eq!(
            decide(GuestEvent::Restart, host(true, true, true)),
            Action::Reboot
        );
    }

    /// A guest crash restarts the guest, never the host: an unattended host that reboots on a
    /// guest bug is a machine the user cannot use.
    #[test]
    fn a_guest_crash_restarts_only_the_guest() {
        assert_eq!(
            decide(GuestEvent::Crashed, host(true, true, true)),
            Action::RestartGuest
        );
    }

    /// The critical honesty rule: a sleep request must not suspend the host, because the guest
    /// cannot come back from S3 on this platform.
    #[test]
    fn a_sleep_request_does_not_suspend_the_host() {
        let a = decide(GuestEvent::SleepRequested, host(true, true, true));
        assert_ne!(a, Action::Suspend);
        assert!(matches!(a, Action::None(reason) if reason.contains("S3")));
    }

    /// A host that cannot power off says so instead of pretending.
    #[test]
    fn an_incapable_host_reports_why() {
        assert!(matches!(
            decide(GuestEvent::ShutDown, host(false, false, false)),
            Action::None(r) if r.contains("cannot power itself off")
        ));
        assert!(matches!(
            decide(GuestEvent::Restart, host(false, false, false)),
            Action::None(r) if r.contains("cannot reboot")
        ));
    }

    /// Suspend is never produced by any event today. If that changes, this test must change
    /// with it — which is the point: it pins the current, honest behaviour.
    #[test]
    fn suspend_is_never_chosen_without_a_resumable_guest() {
        let events = [
            GuestEvent::ShutDown,
            GuestEvent::Restart,
            GuestEvent::Crashed,
            GuestEvent::SleepRequested,
        ];
        for e in events {
            assert_ne!(decide(e, host(true, true, true)), Action::Suspend, "{e:?}");
        }
    }
}
