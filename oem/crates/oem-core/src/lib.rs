//! OEM Core: the coordinator of an OEM-macOS host.
//!
//! The host exists to run one macOS guest well. Everything here is in service of that:
//! detect what the machine is, decide honestly what graphics it can do, start the guest, and
//! keep the host invisible while it runs.
//!
//! The modules are ordered as the boot sequence uses them.

pub mod detect;
pub mod graphics;
pub mod log;
pub mod power;
pub mod recovery;
pub mod vm;

/// The version the report and the journal name.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
