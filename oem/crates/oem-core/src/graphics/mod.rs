//! GPU identification and backend selection.
//!
//! Section 16 of the brief asks for legacy Intel support. The honest answer is decided by
//! measured capability, not by a marketing table, so this module does two separate things:
//!
//! - [`classify`] predicts what a known PCI device can do, from a table built on documented
//!   driver support. It is used only to explain a result, never to override one.
//! - [`select`] decides the backend from what the Vulkan loader actually reported. It is the
//!   authority: a device the table calls "experimental" that reports Vulkan 1.2 with a device
//!   UUID is treated as capable, and a device the table calls "supported" that reports 1.0 is
//!   not.
//!
//! reims-vgpu's own requirement is the gate: `crates/reims-vgpu-vulkan/src/device_choice.rs`
//! takes a physical device only where `apiVersion >= 1.2`, a graphics+compute queue exists, and
//! the device reports a `deviceUUID`. A backend that cannot meet that cannot host macOS.

pub mod backend;
pub mod legacy;
#[cfg(all(target_os = "linux", feature = "vulkan"))]
pub mod probe;

pub use backend::{select, Backend, Decision, Requirement};
pub use legacy::{classify, IntelFamily, LegacySupport};
#[cfg(all(target_os = "linux", feature = "vulkan"))]
pub use probe::{run as probe, ProbeResult, ProbedDevice};

/// What the Vulkan loader reported for one physical device.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VulkanCaps {
    /// `apiVersion` as the driver reported it, unpacked: (major, minor, patch).
    pub api_version: (u32, u32, u32),
    /// The device's `VK_KHR_device_group`/1.1 `deviceUUID`, if it reports one.
    pub device_uuid: Option<[u8; 16]>,
    /// The driver's own name, for the report (`driverName`).
    pub driver_name: String,
}

impl VulkanCaps {
    /// The version as `vulkaninfo` prints it, `1.2.305`.
    pub fn version_string(&self) -> String {
        let (a, b, c) = self.api_version;
        format!("{a}.{b}.{c}")
    }

    /// Whether this device meets every requirement reims-vgpu states for its GPU.
    pub fn meets_reims(&self) -> bool {
        let (major, minor, _) = self.api_version;
        let at_least_1_2 = major > 1 || (major == 1 && minor >= 2);
        at_least_1_2 && self.device_uuid.is_some()
    }
}

/// A GPU as the PCI bus reports it, before any driver has spoken.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PciGpu {
    pub vendor_id: u16,
    pub device_id: u16,
    /// The kernel driver bound to the device, as `/sys/bus/pci/devices/*/driver` names it.
    pub driver: Option<String>,
    /// The PCI slot, `0000:00:02.0`.
    pub slot: String,
}

impl PciGpu {
    pub fn vendor_name(&self) -> &'static str {
        match self.vendor_id {
            0x8086 => "Intel",
            0x10de => "NVIDIA",
            0x1002 | 0x1022 => "AMD",
            0x1af4 => "virtio",
            0x15ad => "VMware",
            0x1234 => "QEMU",
            _ => "unknown",
        }
    }

    /// The id as the report and `MESA_VK_DEVICE_SELECT` write it, `8086:0166`.
    pub fn id(&self) -> String {
        format!("{:04x}:{:04x}", self.vendor_id, self.device_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn caps(a: u32, b: u32, c: u32, uuid: bool) -> VulkanCaps {
        VulkanCaps {
            api_version: (a, b, c),
            device_uuid: uuid.then_some([7u8; 16]),
            driver_name: "test".into(),
        }
    }

    #[test]
    fn version_string_is_dotted() {
        assert_eq!(caps(1, 2, 305, true).version_string(), "1.2.305");
        assert_eq!(caps(1, 4, 0, true).version_string(), "1.4.0");
    }

    /// The exact gate reims-vgpu's device choice applies.
    #[test]
    fn reims_needs_1_2_and_a_uuid() {
        assert!(caps(1, 2, 0, true).meets_reims());
        assert!(caps(1, 3, 290, true).meets_reims());
        assert!(caps(2, 0, 0, true).meets_reims());
        // 1.1 is short of the requirement even with a UUID.
        assert!(!caps(1, 1, 0, true).meets_reims());
        // 1.2 without a UUID is short too.
        assert!(!caps(1, 2, 0, false).meets_reims());
    }

    #[test]
    fn vendor_names() {
        let g = |v| PciGpu {
            vendor_id: v,
            device_id: 0,
            driver: None,
            slot: "0000:00:02.0".into(),
        };
        assert_eq!(g(0x8086).vendor_name(), "Intel");
        assert_eq!(g(0x10de).vendor_name(), "NVIDIA");
        assert_eq!(g(0x1002).vendor_name(), "AMD");
        assert_eq!(g(0x9999).vendor_name(), "unknown");
        assert_eq!(g(0x8086).id(), "8086:0000");
    }
}
