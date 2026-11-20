//! Choosing the graphics backend from measured capability.
//!
//! The brief's priority list (native acceleration, then Vulkan, then OpenGL, then a
//! translation layer, then software) is implemented here as a decision over a live probe,
//! with one important correction: **a Vulkan device that is a CPU implementation is not
//! acceleration.** lavapipe reports Vulkan 1.4 and a device UUID, so it passes reims-vgpu's
//! technical gate, but it renders on the CPU. It is therefore classified as
//! [`Backend::VulkanOnCpu`], never as [`Backend::Accelerated`], so the report cannot claim
//! hardware acceleration that is not there.

use super::{legacy, VulkanCaps};

/// The backend OEM-macOS will use.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Backend {
    /// `reims-graphics-pci` on a real GPU: the guest gets Metal on the host's Vulkan device.
    Accelerated { gpu: String, vulkan: String },
    /// `reims-graphics-pci` on a CPU Vulkan implementation. Functionally valid, very slow.
    VulkanOnCpu { gpu: String, vulkan: String },
    /// No usable Vulkan device: VMware SVGA, so macOS gets no acceleration at all.
    Software { reason: String },
}

impl Backend {
    /// Whether the macOS guest gets GPU acceleration. Only [`Backend::Accelerated`] does.
    pub fn accelerates(&self) -> bool {
        matches!(self, Backend::Accelerated { .. })
    }

    /// The `-device` name this backend boots with.
    pub fn device(&self) -> &'static str {
        match self {
            Backend::Accelerated { .. } | Backend::VulkanOnCpu { .. } => "reims-graphics-pci",
            Backend::Software { .. } => "vmware-svga",
        }
    }

    /// One line for the compatibility report.
    pub fn summary(&self) -> String {
        match self {
            Backend::Accelerated { gpu, vulkan } => {
                format!("accelerated: reims-graphics-pci on {gpu} (Vulkan {vulkan})")
            }
            Backend::VulkanOnCpu { gpu, vulkan } => format!(
                "not accelerated: reims-graphics-pci on {gpu} (Vulkan {vulkan}, CPU rendering)"
            ),
            Backend::Software { reason } => {
                format!("not accelerated: vmware-svga ({reason})")
            }
        }
    }
}

/// A requirement that was not met, for the report.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Requirement {
    /// No Vulkan loader on the host at all.
    NoLoader,
    /// A loader is present, but no physical device answered.
    NoDevice,
    /// The device's Vulkan version is below 1.2.
    VulkanTooOld { found: String },
    /// The device reports no `deviceUUID`, which reims-vgpu's device choice requires.
    NoDeviceUuid,
}

impl Requirement {
    pub fn describe(&self) -> String {
        match self {
            Requirement::NoLoader => {
                "no Vulkan loader: install vulkan-icd-loader and a GPU driver".into()
            }
            Requirement::NoDevice => {
                "the Vulkan loader found no physical device: install the GPU's Vulkan driver \
                 (vulkan-intel, vulkan-radeon, nvidia-utils, or lavapipe)"
                    .into()
            }
            Requirement::VulkanTooOld { found } => {
                format!("the device reports Vulkan {found}; reims-vgpu needs 1.2 or newer")
            }
            Requirement::NoDeviceUuid => {
                "the device reports no deviceUUID, which reims-vgpu's device choice requires".into()
            }
        }
    }
}

/// The outcome of the decision.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Decision {
    pub backend: Backend,
    /// Every requirement that failed, in the order they were checked. Empty for a clean
    /// acceleration decision.
    pub unmet: Vec<Requirement>,
    /// The Intel legacy classification, where the device is an Intel iGPU.
    pub legacy: Option<legacy::Classification>,
}

/// Decides the backend from what the probe saw.
///
/// `loader_present` is whether a Vulkan loader could be loaded at all; `devices` is every
/// physical device the loader enumerated, with its capabilities. A CPU device is recognised by
/// `cpu` being true (the probe sets it from `VK_PHYSICAL_DEVICE_TYPE_CPU`).
pub fn select(
    loader_present: bool,
    devices: &[(String, VulkanCaps, bool)],
    intel_device: Option<(u16, u16)>,
) -> Decision {
    if !loader_present {
        return Decision {
            backend: Backend::Software {
                reason: "no Vulkan loader".into(),
            },
            unmet: vec![Requirement::NoLoader],
            legacy: intel_device.map(|(v, d)| legacy::classify(v, d, None)),
        };
    }
    if devices.is_empty() {
        return Decision {
            backend: Backend::Software {
                reason: "no Vulkan device".into(),
            },
            unmet: vec![Requirement::NoDevice],
            legacy: intel_device.map(|(v, d)| legacy::classify(v, d, None)),
        };
    }

    // A real GPU that meets the gate wins over a CPU implementation that also meets it, no
    // matter what order the loader listed them in.
    let usable = |(_, c, cpu): &(String, VulkanCaps, bool)| c.meets_reims() && !cpu;
    let cpu_usable = |(_, c, cpu): &(String, VulkanCaps, bool)| c.meets_reims() && *cpu;

    if let Some((name, caps, _)) = devices.iter().find(|d| usable(d)) {
        return Decision {
            backend: Backend::Accelerated {
                gpu: name.clone(),
                vulkan: caps.version_string(),
            },
            unmet: Vec::new(),
            legacy: intel_device.map(|(v, d)| legacy::classify(v, d, Some(caps))),
        };
    }
    if let Some((name, caps, _)) = devices.iter().find(|d| cpu_usable(d)) {
        return Decision {
            backend: Backend::VulkanOnCpu {
                gpu: name.clone(),
                vulkan: caps.version_string(),
            },
            // A CPU device is a deliberate fallback, not a failed requirement.
            unmet: Vec::new(),
            legacy: intel_device.map(|(v, d)| legacy::classify(v, d, Some(caps))),
        };
    }

    // Nothing meets the gate. Say precisely why, using the best device present.
    let (name, caps, _) = devices
        .iter()
        .max_by_key(|(_, c, _)| c.api_version)
        .expect("devices is not empty");
    let mut unmet = Vec::new();
    let (major, minor, _) = caps.api_version;
    if !(major > 1 || (major == 1 && minor >= 2)) {
        unmet.push(Requirement::VulkanTooOld {
            found: caps.version_string(),
        });
    }
    if caps.device_uuid.is_none() {
        unmet.push(Requirement::NoDeviceUuid);
    }
    Decision {
        backend: Backend::Software {
            reason: format!("{name} cannot host reims-graphics-pci"),
        },
        unmet,
        legacy: intel_device.map(|(v, d)| legacy::classify(v, d, Some(caps))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn caps(v: (u32, u32, u32), uuid: bool, driver: &str) -> VulkanCaps {
        VulkanCaps {
            api_version: v,
            device_uuid: uuid.then_some([3; 16]),
            driver_name: driver.into(),
        }
    }

    fn dev(name: &str, v: (u32, u32, u32), uuid: bool, cpu: bool) -> (String, VulkanCaps, bool) {
        (name.into(), caps(v, uuid, "test"), cpu)
    }

    #[test]
    fn no_loader_is_reported_as_such() {
        let d = select(false, &[], None);
        assert_eq!(d.unmet, vec![Requirement::NoLoader]);
        assert!(!d.backend.accelerates());
    }

    #[test]
    fn no_device_is_reported_as_such() {
        let d = select(true, &[], None);
        assert_eq!(d.unmet, vec![Requirement::NoDevice]);
    }

    /// A modern discrete GPU is accelerated.
    #[test]
    fn modern_gpu_accelerates() {
        let d = select(true, &[dev("RTX 4090", (1, 4, 0), true, false)], None);
        assert!(d.backend.accelerates());
        assert_eq!(d.backend.device(), "reims-graphics-pci");
        assert!(d.unmet.is_empty());
        assert!(d.backend.summary().contains("RTX 4090"));
    }

    /// The critical honesty test: lavapipe passes the technical gate but must never be
    /// reported as hardware acceleration.
    #[test]
    fn cpu_vulkan_is_not_acceleration() {
        let d = select(true, &[dev("llvmpipe", (1, 4, 305), true, true)], None);
        assert!(!d.backend.accelerates());
        assert!(matches!(d.backend, Backend::VulkanOnCpu { .. }));
        assert!(d.backend.summary().contains("CPU rendering"));
        assert!(d.backend.summary().starts_with("not accelerated"));
    }

    /// A real GPU beats a CPU device even when the CPU device is listed first.
    #[test]
    fn real_gpu_wins_over_cpu_listed_first() {
        let d = select(
            true,
            &[
                dev("llvmpipe", (1, 4, 305), true, true),
                dev("Arc A770", (1, 3, 290), true, false),
            ],
            None,
        );
        assert!(d.backend.accelerates());
        assert!(d.backend.summary().contains("Arc A770"));
    }

    /// Haswell: Vulkan 1.0 through HASVK, so the gate fails for the right reason.
    #[test]
    fn haswell_fails_on_version_with_a_real_reason() {
        let d = select(
            true,
            &[dev("Intel HD 4600 (HASVK)", (1, 0, 0), true, false)],
            Some((0x8086, 0x0412)),
        );
        assert!(!d.backend.accelerates());
        assert_eq!(d.backend.device(), "vmware-svga");
        assert_eq!(
            d.unmet,
            vec![Requirement::VulkanTooOld {
                found: "1.0.0".into()
            }]
        );
        let legacy = d.legacy.expect("Haswell is classified");
        assert_eq!(legacy.family, Some(legacy::IntelFamily::Haswell));
        assert!(d.unmet[0].describe().contains("needs 1.2"));
    }

    /// A 1.2 device without a UUID fails on the UUID alone.
    #[test]
    fn missing_uuid_is_its_own_failure() {
        let d = select(true, &[dev("odd driver", (1, 2, 0), false, false)], None);
        assert_eq!(d.unmet, vec![Requirement::NoDeviceUuid]);
    }

    /// Ivy Bridge with no Vulkan driver at all: two requirements fail at once.
    #[test]
    fn ivy_bridge_fails_on_both_version_and_uuid() {
        let d = select(
            true,
            &[dev("Mesa", (0, 0, 0), false, false)],
            Some((0x8086, 0x0166)),
        );
        assert!(!d.backend.accelerates());
        assert_eq!(d.unmet.len(), 2);
        assert!(d
            .unmet
            .iter()
            .any(|r| matches!(r, Requirement::VulkanTooOld { .. })));
        assert!(d
            .unmet
            .iter()
            .any(|r| matches!(r, Requirement::NoDeviceUuid)));
        assert_eq!(
            d.legacy.unwrap().family,
            Some(legacy::IntelFamily::IvyBridge)
        );
    }

    /// With several devices that all fail, the newest is the one reported.
    #[test]
    fn newest_device_is_the_one_described() {
        let d = select(
            true,
            &[
                dev("old", (1, 0, 0), true, false),
                dev("newer", (1, 1, 0), true, false),
            ],
            None,
        );
        assert!(
            d.backend.summary().contains("newer"),
            "{}",
            d.backend.summary()
        );
    }
}
