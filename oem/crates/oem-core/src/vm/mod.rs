//! OEM VM: the macOS guest's lifecycle.
//!
//! The VM is not managed by the user. There is no VM list, no settings dialog, no QEMU window.
//! OEM VM owns exactly one guest, described by a [`Config`] written at install time, and it
//! drives that guest through the engine that actually implements it: `reims-ctl`.
//!
//! The split of responsibility is deliberate and is the whole point of the design:
//!
//! - **`reims-ctl`** owns the parts that must agree with upstream exactly — the OpenCore image,
//!   the Apple download, the QEMU command line, the disk layout. Reimplementing those here
//!   would drift from upstream and break silently at the next update.
//! - **OEM Core** owns what upstream has no opinion about: which guest to run, when, in what
//!   display mode, how failures are detected, and what the user sees.
//!
//! So this module builds the `reims-ctl` arguments, reads its machine-readable output, and
//! decides whether a start succeeded. It never re-implements QEMU's command line.

pub mod config;
pub mod engine;
#[cfg(unix)]
pub mod qmp;

pub use config::{Config, Display, Graphics};
pub use engine::{Engine, State};
#[cfg(unix)]
pub use qmp::ShutdownReason;
