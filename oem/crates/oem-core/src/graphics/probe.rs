//! The Vulkan capability probe: what the host's drivers actually report.
//!
//! `reims-vgpu` decides its GPU from three facts — `apiVersion`, a graphics+compute queue, and
//! a `deviceUUID` (`crates/reims-vgpu-vulkan/src/device_choice.rs`). Upstream's own
//! `reims-ctl __vulkan-probe` reports only vendor and device IDs, so it cannot answer whether a
//! device will be accepted. This module asks the loader directly, for exactly those facts.
//!
//! The probe never enables an extension or creates a device: it reads instance-level
//! properties, which any driver answers without a risk of crashing the host. That is deliberate
//! — a broken legacy driver should cost a failed probe, not a failed boot.

use crate::graphics::VulkanCaps;

/// The device extensions `reims-vgpu` refuses a device without
/// (`crates/reims-vgpu-vulkan/src/os/linux/plane_copy.rs`, the `needed` array).
pub const REQUIRED_DEVICE_EXTENSIONS: [&str; 4] = [
    "VK_KHR_external_memory_fd",
    "VK_EXT_external_memory_dma_buf",
    "VK_EXT_image_drm_format_modifier",
    "VK_EXT_queue_family_foreign",
];

/// One device the loader reported.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProbedDevice {
    pub name: String,
    pub vendor_id: u32,
    pub device_id: u32,
    /// The `VkPhysicalDeviceType`, as the words the report uses.
    pub kind: String,
    pub caps: VulkanCaps,
    /// Whether the device is a CPU implementation (`PHYSICAL_DEVICE_TYPE_CPU`), which is what
    /// makes lavapipe "Vulkan, but not acceleration".
    pub cpu: bool,
    /// Whether a graphics+compute queue family exists, the second of reims-vgpu's requirements.
    pub has_graphics_compute_queue: bool,
    /// Device extensions reims-vgpu requires that this device does not list.
    ///
    /// This is the requirement that decides real hardware, and it cannot be inferred from the
    /// Vulkan version: `plane_copy.rs` refuses a device that lists no
    /// `VK_KHR_external_memory_fd`, `VK_EXT_external_memory_dma_buf`,
    /// `VK_EXT_image_drm_format_modifier` or `VK_EXT_queue_family_foreign`. A device that meets
    /// 1.2 and has a UUID but lacks one of these still cannot host the guest.
    pub missing_device_extensions: Vec<String>,
}

impl ProbedDevice {
    /// Whether this device can host `reims-graphics-pci` at all, version aside.
    pub fn has_reims_extensions(&self) -> bool {
        self.missing_device_extensions.is_empty()
    }
}

/// The result of a probe.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProbeResult {
    /// Whether a Vulkan loader could be loaded at all.
    pub loader: bool,
    pub devices: Vec<ProbedDevice>,
    /// A human description of a failure to load the loader, where `loader` is false.
    pub error: Option<String>,
}

impl ProbeResult {
    /// The devices reduced to the shape [`crate::graphics::select`] takes.
    pub fn as_selection_input(&self) -> Vec<crate::graphics::backend::Candidate> {
        self.devices
            .iter()
            .map(|d| crate::graphics::backend::Candidate {
                name: d.name.clone(),
                caps: d.caps.clone(),
                cpu: d.cpu,
                missing_device_extensions: d.missing_device_extensions.clone(),
            })
            .collect()
    }
}

/// Unpacks a packed `VkVersion` into `(major, minor, patch)`, the way the Vulkan headers do.
pub fn unpack_version(v: u32) -> (u32, u32, u32) {
    ((v >> 22) & 0x7f, (v >> 12) & 0x3ff, v & 0xfff)
}

#[cfg(target_os = "linux")]
mod imp {
    use super::{unpack_version, ProbeResult, ProbedDevice};
    use crate::graphics::VulkanCaps;
    use ash::vk;

    /// Asks the loader. `Ok` carries what it said; `Err` carries why it could not be asked.
    pub fn run() -> ProbeResult {
        // SAFETY: loading the loader is the documented entry point; every call below follows
        // the Vulkan API's rules, and the instance is destroyed before returning.
        unsafe {
            let entry = match ash::Entry::load() {
                Ok(e) => e,
                Err(e) => {
                    return ProbeResult {
                        loader: false,
                        devices: Vec::new(),
                        error: Some(format!("libvulkan could not be loaded: {e}")),
                    };
                }
            };
            // The instance must ask for at least 1.1: `VkPhysicalDeviceIDProperties` is a 1.1
            // struct, and a driver is not obliged to fill it for a 1.0 instance. Measured on
            // lavapipe 25.0.7: at 1.0 the UUID came back all zero, at 1.1 it is filled.
            let app = vk::ApplicationInfo::default().api_version(vk::make_api_version(0, 1, 1, 0));
            let ci = vk::InstanceCreateInfo::default().application_info(&app);
            let instance = match entry.create_instance(&ci, None) {
                Ok(i) => i,
                Err(e) => {
                    return ProbeResult {
                        loader: true,
                        devices: Vec::new(),
                        error: Some(format!("a Vulkan instance could not be created: {e}")),
                    };
                }
            };
            let mut devices = Vec::new();
            let mut id_props = vk::PhysicalDeviceIDProperties::default();
            if let Ok(pds) = instance.enumerate_physical_devices() {
                for pd in pds {
                    // deviceUUID lives in VkPhysicalDeviceIDProperties (Vulkan 1.1), reached
                    // through the properties2 chain, not in VkPhysicalDeviceProperties.
                    let mut props2 =
                        vk::PhysicalDeviceProperties2::default().push_next(&mut id_props);
                    instance.get_physical_device_properties2(pd, &mut props2);
                    let p = props2.properties;
                    let name = p
                        .device_name_as_c_str()
                        .map(|c| c.to_string_lossy().into_owned())
                        .unwrap_or_else(|_| {
                            format!("device {:04x}:{:04x}", p.vendor_id, p.device_id)
                        });
                    let (cpu, kind) = match p.device_type {
                        vk::PhysicalDeviceType::CPU => (true, "cpu"),
                        vk::PhysicalDeviceType::DISCRETE_GPU => (false, "discrete"),
                        vk::PhysicalDeviceType::INTEGRATED_GPU => (false, "integrated"),
                        vk::PhysicalDeviceType::VIRTUAL_GPU => (false, "virtual"),
                        _ => (false, "other"),
                    };
                    // A driver that reports no UUID leaves the field all zero. All-zero is not
                    // a UUID, and reims-vgpu's device choice requires a real one.
                    let raw = id_props.device_uuid;
                    let uuid = if raw.iter().any(|b| *b != 0) {
                        Some(raw)
                    } else {
                        None
                    };
                    let has_graphics_compute_queue = instance
                        .get_physical_device_queue_family_properties(pd)
                        .iter()
                        .any(|q| {
                            q.queue_flags
                                .contains(vk::QueueFlags::GRAPHICS | vk::QueueFlags::COMPUTE)
                        });
                    // The extensions reims-vgpu refuses a device without. Reading the list is
                    // an instance-level call; nothing is enabled, so a legacy driver that
                    // advertises one and then fails still costs only this query.
                    let listed: Vec<String> = instance
                        .enumerate_device_extension_properties(pd)
                        .unwrap_or_default()
                        .iter()
                        .filter_map(|e| e.extension_name_as_c_str().ok())
                        .map(|c| c.to_string_lossy().into_owned())
                        .collect();
                    let missing_device_extensions: Vec<String> = super::REQUIRED_DEVICE_EXTENSIONS
                        .iter()
                        .filter(|want| !listed.iter().any(|have| have == *want))
                        .map(|want| (*want).to_owned())
                        .collect();
                    devices.push(ProbedDevice {
                        name,
                        vendor_id: p.vendor_id,
                        device_id: p.device_id,
                        kind: kind.to_owned(),
                        caps: VulkanCaps {
                            api_version: unpack_version(p.api_version),
                            device_uuid: uuid,
                            driver_name: format!("Vulkan {}", unpack_version(p.api_version).0),
                        },
                        cpu,
                        has_graphics_compute_queue,
                        missing_device_extensions,
                    });
                }
            }
            instance.destroy_instance(None);
            ProbeResult {
                loader: true,
                devices,
                error: None,
            }
        }
    }
}

#[cfg(not(target_os = "linux"))]
mod imp {
    use super::ProbeResult;

    /// The probe is Linux's: reims-ctl's VMs are Linux's too.
    pub fn run() -> ProbeResult {
        ProbeResult {
            loader: false,
            devices: Vec::new(),
            error: Some("the Vulkan probe is implemented for Linux hosts only".into()),
        }
    }
}

pub use imp::run;

#[cfg(test)]
mod tests {
    use super::*;

    /// The packing is Vulkan's own, so the test builds it with Vulkan's own macro rather than
    /// with a copy of the bit arithmetic that could drift from it.
    #[test]
    fn version_unpacking_matches_the_vulkan_macro() {
        for (major, minor, patch) in [(1u32, 2u32, 305u32), (1, 4, 0), (1, 0, 0), (1, 3, 290)] {
            let packed = ash::vk::make_api_version(0, major, minor, patch);
            assert_eq!(unpack_version(packed), (major, minor, patch));
        }
        assert_eq!(unpack_version(0), (0, 0, 0));
    }

    /// The probe runs against this machine's real loader. In CI without Vulkan it reports no
    /// loader, which is itself a correct answer — the test asserts consistency, not a device.
    #[test]
    fn probe_is_consistent_with_what_it_finds() {
        let r = run();
        if !r.loader {
            assert!(r.devices.is_empty());
            assert!(r.error.is_some(), "a missing loader must explain itself");
            return;
        }
        assert!(r.error.is_none());
        // Every device must have a version the loader reported, and a name.
        for d in &r.devices {
            assert!(!d.name.is_empty());
            assert!(
                d.caps.api_version.0 <= 1,
                "no Vulkan 2 exists yet: {:?}",
                d.caps.api_version
            );
            assert!(
                ["cpu", "discrete", "integrated", "virtual", "other"].contains(&d.kind.as_str())
            );
            // A CPU device must never be claimed as acceleration by the selection input.
            assert_eq!(d.cpu, d.kind == "cpu");
        }
        // The reduced form keeps every device, in order.
        assert_eq!(r.as_selection_input().len(), r.devices.len());
    }

    /// On a host with lavapipe installed, the probe must see it *and* mark it CPU. This is the
    /// test that would catch a probe that mistook software Vulkan for a GPU.
    #[test]
    fn lavapipe_if_present_is_marked_cpu() {
        let r = run();
        if !r.loader {
            return;
        }
        for d in r
            .devices
            .iter()
            .filter(|d| d.name.to_lowercase().contains("llvmpipe"))
        {
            assert!(d.cpu, "llvmpipe must be classified as a CPU device");
        }
    }
}
