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
    /// The device does not list the dma-buf extensions. This is **not** a refusal: reims-vgpu
    /// copies the surfaces instead of importing them, which is slower but works. It is reported
    /// as a degradation, never as an unmet requirement.
    DmaBufUnavailable { missing: Vec<String> },
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
            Requirement::DmaBufUnavailable { missing } => format!(
                "the device lists no {}; reims-vgpu copies surfaces to the GPU instead of \
                 importing them, which costs bandwidth but works",
                missing.join(", ")
            ),
            Requirement::NoDeviceUuid => {
                "the device reports no deviceUUID, which reims-vgpu's device choice requires".into()
            }
        }
    }
}

/// What the probe saw of one device, as [`select`] takes it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    pub name: String,
    pub caps: VulkanCaps,
    /// Whether the device is a CPU implementation (lavapipe), which is never acceleration.
    pub cpu: bool,
    /// Dma-buf extensions the device does not list. These do not make the device unusable: the
    /// runtime copies surfaces instead. Empty when the probe could not be asked.
    pub missing_device_extensions: Vec<String>,
}

impl Candidate {
    /// Every requirement reims-vgpu actually refuses a device for, checked against what this
    /// device reported: Vulkan 1.2, a graphics+compute queue, and a `deviceUUID`
    /// (`device_choice.rs`, `Seen::candidate`). The dma-buf extensions are deliberately absent:
    /// they change how surfaces reach the GPU, not whether the guest can start.
    pub fn unmet(&self) -> Vec<Requirement> {
        let mut unmet = Vec::new();
        let (major, minor, _) = self.caps.api_version;
        if !(major > 1 || (major == 1 && minor >= 2)) {
            unmet.push(Requirement::VulkanTooOld {
                found: self.caps.version_string(),
            });
        }
        if self.caps.device_uuid.is_none() {
            unmet.push(Requirement::NoDeviceUuid);
        }
        unmet
    }

    /// Whether the device shares guest memory with the GPU as dma-bufs, or the runtime has to
    /// copy surfaces. `false` is a real slowdown, not a failure to start.
    pub fn shares_dma_bufs(&self) -> bool {
        self.missing_device_extensions.is_empty()
    }

    /// Whether this device can host the guest.
    pub fn usable(&self) -> bool {
        self.unmet().is_empty()
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
    devices: &[Candidate],
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

    // A real GPU that meets every requirement wins over a CPU implementation that also meets
    // them, no matter what order the loader listed them in.
    if let Some(d) = devices.iter().find(|d| d.usable() && !d.cpu) {
        return Decision {
            backend: Backend::Accelerated {
                gpu: d.name.clone(),
                vulkan: d.caps.version_string(),
            },
            unmet: Vec::new(),
            legacy: intel_device.map(|(v, id)| legacy::classify(v, id, Some(&d.caps))),
        };
    }
    if let Some(d) = devices.iter().find(|d| d.usable() && d.cpu) {
        return Decision {
            backend: Backend::VulkanOnCpu {
                gpu: d.name.clone(),
                vulkan: d.caps.version_string(),
            },
            // A CPU device is a deliberate fallback, not a failed requirement.
            unmet: Vec::new(),
            legacy: intel_device.map(|(v, id)| legacy::classify(v, id, Some(&d.caps))),
        };
    }

    // Nothing meets every requirement. Say precisely why, using the device that came closest:
    // the one with the fewest unmet requirements, then the highest version.
    let best = devices
        .iter()
        .min_by_key(|d| (d.unmet().len(), std::cmp::Reverse(d.caps.api_version)))
        .expect("devices is not empty");
    Decision {
        backend: Backend::Software {
            reason: format!("{} cannot host reims-graphics-pci", best.name),
        },
        unmet: best.unmet(),
        legacy: intel_device.map(|(v, id)| legacy::classify(v, id, Some(&best.caps))),
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

    /// A device that lists every extension reims-vgpu needs. `uuid` and `cpu` are the two facts
    /// the tests vary; the extension list is added by `dev_missing`.
    fn dev(name: &str, v: (u32, u32, u32), uuid: bool, cpu: bool) -> Candidate {
        dev_missing(name, v, uuid, cpu, &[])
    }

    fn dev_missing(
        name: &str,
        v: (u32, u32, u32),
        uuid: bool,
        cpu: bool,
        missing: &[&str],
    ) -> Candidate {
        Candidate {
            name: name.into(),
            caps: caps(v, uuid, "test"),
            cpu,
            missing_device_extensions: missing.iter().map(|s| (*s).to_owned()).collect(),
        }
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

    /// A Haswell whose HASVK reports only 1.0 (an old Mesa, or a broken install) fails on the
    /// version and says so. Note the device here is what the *probe* saw; a current HASVK
    /// reports 1.2, which the next test covers.
    #[test]
    fn a_vulkan_too_old_device_fails_with_a_real_reason() {
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

    /// Haswell on a current HASVK reports 1.2 with a UUID, so the gate passes and the backend
    /// is chosen — but the classification stays experimental, because Mesa does not test the
    /// path here and neither has this project.
    #[test]
    fn haswell_on_a_current_hasvk_is_selected_but_flagged_experimental() {
        let d = select(
            true,
            &[dev("Intel HD 4600 (intel_hasvk)", (1, 2, 0), true, false)],
            Some((0x8086, 0x0412)),
        );
        assert!(d.unmet.is_empty(), "unmet: {:?}", d.unmet);
        assert!(d.backend.accelerates());
        let legacy = d.legacy.expect("Haswell is classified");
        assert_eq!(
            legacy.support,
            legacy::LegacySupport::Experimental,
            "a generation Mesa does not test must not be called supported"
        );
        assert!(legacy.support.accelerated());
    }

    /// The dma-buf extensions are a degradation, not a refusal, and this test pins that because
    /// the opposite was believed here once. `device_choice.rs` accepts a device on
    /// `api_version >= 1.2 && graphics_compute && uuid.is_some()` and nothing else. Missing
    /// dma-buf extensions make the runtime copy surfaces ("IOSurfaces are copied for the GPU"),
    /// which is slower but starts. Measured: the real `reims-graphics-pci` initialised against
    /// llvmpipe, which lacks `VK_EXT_image_drm_format_modifier`, and ran.
    #[test]
    fn a_device_without_dma_buf_extensions_still_runs() {
        let d = select(
            true,
            &[dev_missing(
                "llvmpipe",
                (1, 4, 305),
                true,
                true,
                &["VK_EXT_image_drm_format_modifier"],
            )],
            None,
        );
        assert!(
            d.unmet.is_empty(),
            "missing extensions must not refuse a device: {:?}",
            d.unmet
        );
        assert_eq!(
            d.backend.device(),
            "reims-graphics-pci",
            "the device is still offered to the guest; only its speed differs"
        );
        assert!(!d.backend.accelerates(), "a CPU device never accelerates");
    }

    /// The degradation is still visible in the report, so a slow machine can be explained.
    #[test]
    fn a_missing_dma_buf_extension_is_reported_as_a_degradation() {
        let c = dev_missing(
            "Intel HD 4600 (intel_hasvk)",
            (1, 2, 0),
            true,
            false,
            &["VK_EXT_external_memory_dma_buf"],
        );
        assert!(c.usable(), "the device must still be selectable");
        assert!(!c.shares_dma_bufs());
        let why = Requirement::DmaBufUnavailable {
            missing: c.missing_device_extensions.clone(),
        }
        .describe();
        assert!(why.contains("VK_EXT_external_memory_dma_buf"), "{why}");
        assert!(why.contains("copies surfaces"), "{why}");
    }

    /// A device that meets every real requirement is accelerated, and one that misses several
    /// reports all of them, not just the first.
    #[test]
    fn every_unmet_requirement_is_reported() {
        let d = select(
            true,
            &[dev_missing(
                "broken",
                (1, 0, 0),
                false,
                false,
                &["VK_KHR_external_memory_fd", "VK_EXT_queue_family_foreign"],
            )],
            None,
        );
        assert_eq!(
            d.unmet.len(),
            2,
            "version and uuid only, never extensions: {:?}",
            d.unmet
        );
        assert!(matches!(d.unmet[0], Requirement::VulkanTooOld { .. }));
        assert_eq!(d.unmet[1], Requirement::NoDeviceUuid);
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
