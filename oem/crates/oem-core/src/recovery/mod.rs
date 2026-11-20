//! OEM Recovery: what happens when the normal boot cannot continue.
//!
//! A dedicated macOS appliance has one failure mode that matters: the machine powers on and
//! nothing usable appears. Recovery exists so that state is recoverable without a second
//! computer. It has two parts:
//!
//! - **Attempt counting** ([`Attempts`]): a boot that fails before the guest is up is counted,
//!   and after too many the next boot takes the maintenance path instead of looping.
//! - **The maintenance path** ([`Mode`]): a text console with a shell and `reims-ctl`, so the
//!   guest can be repaired or the install redone.
//!
//! The counter is a file, written before the guest starts and cleared once it is confirmed up,
//! because a crash between those two points is exactly the case being guarded against.

pub mod attempts;

pub use attempts::{Attempts, Mode, MAX_ATTEMPTS};
