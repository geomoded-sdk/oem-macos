//! OEM Power: host and guest power, coordinated.
//!
//! The brief asks for sleep and resume to be coordinated between the guest and the host. The
//! honest engineering answer, established by reading upstream rather than assuming, is:
//!
//! **ACPI S3 is disabled in this platform.** `vendor/qemu` sets `disable_s3` on the
//! `ICH9-LPC` (see `docs/qemu.md`, "Sleep"), because a guest that enters S3 never wakes with
//! its display: the GPU device's state is not restored by the firmware's resume path. So
//! "macOS sleeps and the host suspends" cannot be implemented on top of this engine today, and
//! claiming otherwise would be exactly the kind of unverified claim the brief forbids.
//!
//! What is implemented, and is genuinely useful:
//!
//! - **Display sleep** inside macOS, which the guest does on its own. No host action.
//! - **Guest shutdown and restart**, coordinated so the host powers off or reboots only after
//!   the guest has stopped. This is [`Action`].
//! - **Host suspend with the guest stopped**, which is safe: the guest is shut down first, and
//!   on resume it is started again. The guest does not survive a host suspend, and the state
//!   is not misrepresented as a resume.
//!
//! The decision of what to do is pure and tested; the effects are thin wrappers.

pub mod policy;

pub use policy::{decide, Action, GuestEvent, HostPower};
