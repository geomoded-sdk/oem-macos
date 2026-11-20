//! Legacy Intel integrated graphics: what each family can actually do, and why.
//!
//! The table is built from documented Mesa driver support, not from hope. Two facts decide
//! everything here:
//!
//! 1. **ANV**, Intel's Vulkan driver, requires **Gen8 (Broadwell) or newer**. Gen7 and Gen7.5
//!    are not supported by ANV at all, at any Vulkan version.
//! 2. **HASVK** (`intel_hasvk`) is a separate Vulkan driver for **Haswell only** (Gen7.5), and
//!    it reports **Vulkan 1.0**. `reims-vgpu` needs 1.2, so HASVK cannot host macOS either.
//!
//! The consequence is unavoidable and is stated plainly rather than papered over: no GPU older
//! than Broadwell can run the accelerated macOS path. Gen7/7.5 hardware is still usable as an
//! OEM-macOS host only in the software-rendering configuration, where the guest gets no GPU
//! acceleration at all.
//!
//! Broadwell and newer are listed as "supported" only in the sense that the driver reaches
//! Vulkan 1.2. Whether a specific Broadwell part reports a `deviceUUID` is a per-driver
//! question that [`super::select`] answers from the live probe, and the probe wins.

/// The Intel graphics family a PCI device ID belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IntelFamily {
    /// Gen5, Ironlake/Arrandale.
    Ironlake,
    /// Gen6, Sandy Bridge.
    SandyBridge,
    /// Gen7, Ivy Bridge.
    IvyBridge,
    /// Gen7.5, Haswell and Haswell-ULT.
    Haswell,
    /// Gen8, Broadwell.
    Broadwell,
    /// Gen9 and later.
    Gen9Plus,
}

impl IntelFamily {
    pub fn name(&self) -> &'static str {
        match self {
            IntelFamily::Ironlake => "Ironlake (Gen5)",
            IntelFamily::SandyBridge => "Sandy Bridge (Gen6)",
            IntelFamily::IvyBridge => "Ivy Bridge (Gen7)",
            IntelFamily::Haswell => "Haswell (Gen7.5)",
            IntelFamily::Broadwell => "Broadwell (Gen8)",
            IntelFamily::Gen9Plus => "Gen9 or later",
        }
    }

    /// The Mesa driver that drives this family.
    pub fn mesa_driver(&self) -> &'static str {
        match self {
            IntelFamily::Ironlake | IntelFamily::SandyBridge => "crocus (GL only)",
            IntelFamily::IvyBridge | IntelFamily::Haswell => "crocus / iris (GL only)",
            IntelFamily::Broadwell | IntelFamily::Gen9Plus => "iris (GL) + anv (Vulkan)",
        }
    }

    /// The Vulkan driver that can drive this family, if any, and the version it reaches.
    pub fn vulkan(&self) -> Option<(&'static str, (u32, u32))> {
        match self {
            // No Vulkan driver at any version.
            IntelFamily::Ironlake | IntelFamily::SandyBridge => None,
            // ANV starts at Gen8; HASVK covers Haswell alone and stops at Vulkan 1.0.
            IntelFamily::IvyBridge => None,
            IntelFamily::Haswell => Some(("intel_hasvk", (1, 0))),
            IntelFamily::Broadwell | IntelFamily::Gen9Plus => Some(("anv", (1, 3))),
        }
    }
}

/// What OEM-macOS can offer on a given GPU.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LegacySupport {
    /// Vulkan 1.2 or newer with a device UUID: the accelerated macOS path is possible.
    Supported,
    /// A Vulkan driver exists but cannot meet reims-vgpu's requirement. No acceleration.
    UnsupportedVulkan,
    /// No Vulkan driver at all; OpenGL through Mesa only.
    OpenGlOnly,
}

impl LegacySupport {
    pub fn label(&self) -> &'static str {
        match self {
            LegacySupport::Supported => "fully supported",
            LegacySupport::UnsupportedVulkan => "Vulkan present but below 1.2: not supported",
            LegacySupport::OpenGlOnly => "software rendering only",
        }
    }

    /// Whether the accelerated macOS path is reachable at all.
    pub fn accelerated(&self) -> bool {
        matches!(self, LegacySupport::Supported)
    }
}

/// A classified GPU.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Classification {
    pub family: Option<IntelFamily>,
    pub support: LegacySupport,
    /// One sentence a support engineer can act on.
    pub reason: String,
}

/// The family of an Intel graphics PCI device ID, from the documented device-ID ranges.
///
/// The ranges are the ones Mesa's `intel/common/intel_device_info` tables use, grouped by
/// generation. An ID outside every range is `None`: an unknown device is never guessed into a
/// family.
pub fn family_of(device_id: u16) -> Option<IntelFamily> {
    match device_id {
        // Ironlake: 0x0042-0x0046 desktop, 0x004a mobile.
        0x0042 | 0x0044 | 0x0046 | 0x004a => Some(IntelFamily::Ironlake),
        // Sandy Bridge: 0x0102, 0x0106, 0x010a, 0x0112, 0x0116, 0x011a, 0x0122, 0x0126, 0x0102.
        0x0102 | 0x0106 | 0x010a | 0x0112 | 0x0116 | 0x011a | 0x0122 | 0x0126 => {
            Some(IntelFamily::SandyBridge)
        }
        // Ivy Bridge: 0x0152, 0x0156, 0x015a, 0x0162, 0x0166, 0x016a.
        0x0152 | 0x0156 | 0x015a | 0x0162 | 0x0166 | 0x016a => Some(IntelFamily::IvyBridge),
        // Haswell: 0x0402, 0x0406, 0x040a, 0x040e, 0x0412, 0x0416, 0x041a, 0x041e, 0x0422,
        // 0x0426, 0x042a, 0x042e, and Haswell-ULT 0x0a02, 0x0a06, 0x0a0a, 0x0a0e, 0x0a12,
        // 0x0a16, 0x0a1a, 0x0a1e, 0x0a22, 0x0a26, 0x0a2a, 0x0a2e.
        0x0402 | 0x0406 | 0x040a | 0x040e | 0x0412 | 0x0416 | 0x041a | 0x041e | 0x0422 | 0x0426
        | 0x042a | 0x042e => Some(IntelFamily::Haswell),
        0x0a02 | 0x0a06 | 0x0a0a | 0x0a0e | 0x0a12 | 0x0a16 | 0x0a1a | 0x0a1e | 0x0a22 | 0x0a26
        | 0x0a2a | 0x0a2e => Some(IntelFamily::Haswell),
        // Broadwell: 0x1602, 0x1606, 0x160a, 0x160e, 0x1612, 0x1616, 0x161a, 0x161e, 0x1622,
        // 0x1626, 0x162a, 0x162e, and Broadwell-U 0x1606 and the 0x162b/0x162d variants.
        0x1602 | 0x1606 | 0x160a | 0x160e | 0x1612 | 0x1616 | 0x161a | 0x161e | 0x1622 | 0x1626
        | 0x162a | 0x162b | 0x162d | 0x162e => Some(IntelFamily::Broadwell),
        // Gen9 and later: Skylake 0x1902-0x1932, Kaby Lake 0x5902-0x5932, and newer families.
        0x1902 | 0x1906 | 0x190a | 0x190e | 0x1912 | 0x1916 | 0x191a | 0x191e | 0x1922 | 0x1926
        | 0x192a | 0x192d | 0x1932 | 0x193a | 0x193b | 0x193d => Some(IntelFamily::Gen9Plus),
        0x5902 | 0x5906 | 0x590a | 0x590e | 0x5912 | 0x5916 | 0x591a | 0x591e | 0x5922 | 0x5926
        | 0x592a | 0x592b | 0x592d | 0x592e | 0x5932 => Some(IntelFamily::Gen9Plus),
        // Coffee Lake, Comet Lake, Ice Lake, Tiger Lake, Alder Lake and later are all Gen9+.
        id if (0x3e90..=0x3e9b).contains(&id) => Some(IntelFamily::Gen9Plus),
        id if (0x9b00..=0x9bff).contains(&id) => Some(IntelFamily::Gen9Plus),
        id if (0x8a50..=0x8a7f).contains(&id) => Some(IntelFamily::Gen9Plus),
        id if (0x46a0..=0x46ff).contains(&id) => Some(IntelFamily::Gen9Plus),
        id if (0xa700..=0xa7ff).contains(&id) => Some(IntelFamily::Gen9Plus),
        _ => None,
    }
}

/// Classifies a PCI device. `measured` is what the Vulkan probe saw, if the device has a
/// driver loaded; it overrides the table wherever it disagrees, because a measurement outranks
/// a prediction.
pub fn classify(
    vendor_id: u16,
    device_id: u16,
    measured: Option<&super::VulkanCaps>,
) -> Classification {
    if vendor_id != 0x8086 {
        // The table is Intel's. Another vendor is classified from the measurement alone.
        let support = match measured {
            Some(c) if c.meets_reims() => LegacySupport::Supported,
            Some(_) => LegacySupport::UnsupportedVulkan,
            None => LegacySupport::OpenGlOnly,
        };
        return Classification {
            family: None,
            support,
            reason: format!(
                "vendor {vendor_id:04x} is outside the Intel legacy table; classified from the probe"
            ),
        };
    }
    let family = family_of(device_id);
    // A measurement is the authority.
    if let Some(c) = measured {
        let support = if c.meets_reims() {
            LegacySupport::Supported
        } else if c.api_version.0 > 0 {
            LegacySupport::UnsupportedVulkan
        } else {
            LegacySupport::OpenGlOnly
        };
        let reason = format!(
            "{} reports Vulkan {} via {} (needs 1.2 and a device UUID: {})",
            family.map(|f| f.name()).unwrap_or("unknown Intel device"),
            c.version_string(),
            c.driver_name,
            if c.meets_reims() { "met" } else { "not met" }
        );
        return Classification {
            family,
            support,
            reason,
        };
    }
    match family {
        Some(IntelFamily::Broadwell) | Some(IntelFamily::Gen9Plus) => Classification {
            family,
            support: LegacySupport::Supported,
            reason: format!(
                "{} uses {}; Vulkan 1.2+ is expected, but confirm with the probe",
                family.unwrap().name(),
                family.unwrap().mesa_driver()
            ),
        },
        Some(IntelFamily::Haswell) => Classification {
            family,
            support: LegacySupport::UnsupportedVulkan,
            reason: "Haswell reaches Vulkan 1.0 through intel_hasvk; reims-vgpu needs 1.2, so \
                     macOS gets no GPU acceleration on this machine"
                .into(),
        },
        Some(f) => Classification {
            family,
            support: LegacySupport::OpenGlOnly,
            reason: format!(
                "{} has no Vulkan driver; {} gives OpenGL only, so macOS runs with software \
                 rendering or not at all",
                f.name(),
                f.mesa_driver()
            ),
        },
        None => Classification {
            family: None,
            support: LegacySupport::OpenGlOnly,
            reason: format!(
                "Intel device {device_id:04x} is not in the legacy table and no probe answered; \
                 treating it as unaccelerated"
            ),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graphics::VulkanCaps;

    fn caps(v: (u32, u32, u32), uuid: bool, driver: &str) -> VulkanCaps {
        VulkanCaps {
            api_version: v,
            device_uuid: uuid.then_some([1; 16]),
            driver_name: driver.into(),
        }
    }

    /// The four GPUs the brief names, by their real device IDs.
    #[test]
    fn named_legacy_gpus_are_placed() {
        // HD 4000 is Ivy Bridge GT2.
        assert_eq!(family_of(0x0166), Some(IntelFamily::IvyBridge));
        // HD 4400 and HD 4600 are Haswell GT2.
        assert_eq!(family_of(0x041e), Some(IntelFamily::Haswell));
        assert_eq!(family_of(0x0412), Some(IntelFamily::Haswell));
        // HD 5000 is Haswell GT3.
        assert_eq!(family_of(0x0a26), Some(IntelFamily::Haswell));
        // HD 5500 is Broadwell GT2.
        assert_eq!(family_of(0x1616), Some(IntelFamily::Broadwell));
    }

    /// The brief's headline question, answered: Ivy Bridge has no Vulkan at any version.
    #[test]
    fn ivy_bridge_has_no_vulkan() {
        let c = classify(0x8086, 0x0166, None);
        assert_eq!(c.family, Some(IntelFamily::IvyBridge));
        assert_eq!(c.support, LegacySupport::OpenGlOnly);
        assert!(!c.support.accelerated());
        assert_eq!(IntelFamily::IvyBridge.vulkan(), None);
    }

    /// Haswell has Vulkan, but only 1.0 — which reims-vgpu refuses.
    #[test]
    fn haswell_vulkan_is_below_the_bar() {
        let c = classify(0x8086, 0x0412, None);
        assert_eq!(c.family, Some(IntelFamily::Haswell));
        assert_eq!(c.support, LegacySupport::UnsupportedVulkan);
        assert!(!c.support.accelerated());
        let (driver, version) = IntelFamily::Haswell.vulkan().unwrap();
        assert_eq!(driver, "intel_hasvk");
        assert!(
            version < (1, 2),
            "HASVK must be recorded below 1.2, got {version:?}"
        );
    }

    /// A live measurement overrides the table in both directions.
    #[test]
    fn measurement_outranks_the_table() {
        // A Broadwell that somehow reports only 1.0 is not usable, whatever the table says.
        let weak = classify(0x8086, 0x1616, Some(&caps((1, 0, 0), true, "anv")));
        assert_eq!(weak.support, LegacySupport::UnsupportedVulkan);
        // A Haswell that reports 1.2 with a UUID is usable; the probe is the authority.
        let strong = classify(0x8086, 0x0412, Some(&caps((1, 2, 0), true, "some-fork")));
        assert_eq!(strong.support, LegacySupport::Supported);
        assert!(strong.support.accelerated());
    }

    #[test]
    fn unknown_ids_are_not_guessed() {
        assert_eq!(family_of(0xdead), None);
        let c = classify(0x8086, 0xdead, None);
        assert_eq!(c.family, None);
        assert!(!c.support.accelerated());
    }

    #[test]
    fn non_intel_vendors_classify_from_probe() {
        let c = classify(0x10de, 0x2684, Some(&caps((1, 4, 0), true, "NVIDIA")));
        assert_eq!(c.support, LegacySupport::Supported);
        assert_eq!(c.family, None);
    }

    #[test]
    fn labels_are_distinct() {
        let all = [
            LegacySupport::Supported,
            LegacySupport::UnsupportedVulkan,
            LegacySupport::OpenGlOnly,
        ];
        for (i, a) in all.iter().enumerate() {
            for b in &all[i + 1..] {
                assert_ne!(a.label(), b.label());
            }
        }
    }
}
