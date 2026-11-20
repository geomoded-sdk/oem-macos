//! Legacy Intel integrated graphics: what each family can actually do, and why.
//!
//! Everything here was read out of Mesa's source (25.0.0) and out of the driver binaries on
//! this machine, not recalled. Three facts decide the answer, and the third one corrects an
//! earlier version of this file that was simply wrong:
//!
//! 1. **ANV**, Intel's Vulkan driver, requires **Gen8 (Broadwell) or newer**. Gen7 and Gen7.5
//!    are absent from `iris_pci_ids.h` entirely — `grep -c 0166 iris_pci_ids.h` is 0.
//! 2. **HASVK** (`intel_hasvk`) is the separate driver for the older generations, and it covers
//!    **Ivy Bridge (Gen7), Haswell (Gen7.5) and Bay Trail**, not Haswell alone. Its build lists
//!    `gfx7`, `gfx75` and `gfx8` (`src/intel/vulkan_hasvk/meson.build`), and
//!    `anv_physical_device.c` accepts a device when `intel_get_device_info_from_fd` matches
//!    Gen7 through Gen8, warning only that "Ivy Bridge Vulkan support is incomplete".
//! 3. **HASVK reports Vulkan 1.2, not 1.0.** `anv_device.c` sets
//!    `.apiVersion = use_softpin ? 1.3 : 1.2`, and `use_softpin` is `!use_relocations` where
//!    `use_relocations = info.ver < 8`, so Gen7 and Gen7.5 get **1.2**. It also computes a
//!    `deviceUUID` (`intel_uuid_compute_device_id`).
//!
//! So the three requirements `reims-vgpu` checks — `apiVersion >= 1.2`, a graphics+compute
//! queue, and a non-null `deviceUUID` — are **all met on Ivy Bridge and Haswell in principle**.
//! This file therefore does not rule them out. What it records is the *version*, and the
//! decision is left to the live probe in [`super::select`], which is the only thing that can
//! say what a particular machine actually reports.
//!
//! What is genuinely missing on Gen7/7.5, and what keeps this "experimental" rather than
//! "supported", is on the Mesa side:
//!
//! - `has_a64_buffer_access = info.ver >= 8 && ...`, so `bufferDeviceAddress` is **false** on
//!   Gen7/7.5. `reims-vgpu` reads that flag and enables it only where present, so this is a
//!   limitation rather than a refusal, but the runtime then has no 64-bit addresses.
//! - Mesa's own CI runs HASVK against **Haswell only** (`hasvk-hsw-fails.txt` and friends);
//!   there is no Ivy Bridge job, so Gen7 correctness is untested upstream.
//! - `use_relocations = info.ver < 8` means Gen7/7.5 run on the relocation path, which the
//!   softpin path exists to replace.
//!
//! The honest classification is therefore: **Haswell = experimentally supported**, Ivy Bridge =
//! **experimental and untested upstream**, and neither may be called "fully supported" until
//! someone runs it on the hardware and measures it. The classification in
//! [`IntelFamily::support`] says exactly that.

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
            IntelFamily::IvyBridge | IntelFamily::Haswell => {
                "crocus (GL) + hasvk (Vulkan, experimental)"
            }
            IntelFamily::Broadwell | IntelFamily::Gen9Plus => "iris (GL) + anv (Vulkan)",
        }
    }

    /// The Vulkan driver that can drive this family, if any, and the version it reaches.
    pub fn vulkan(&self) -> Option<(&'static str, (u32, u32))> {
        match self {
            // No Vulkan driver at any version: HASVK starts at Gen7.
            IntelFamily::Ironlake | IntelFamily::SandyBridge => None,
            // HASVK builds gfx7 and reports 1.2 where the device lacks softpin.
            IntelFamily::IvyBridge => Some(("intel_hasvk", (1, 2))),
            IntelFamily::Haswell => Some(("intel_hasvk", (1, 2))),
            IntelFamily::Broadwell | IntelFamily::Gen9Plus => Some(("anv", (1, 3))),
        }
    }
}

/// What OEM-macOS can offer on a given GPU.
///
/// The distinction between `Supported` and `Experimental` is not cosmetic: `Supported` means a
/// driver the vendor tests and that this project has measured; `Experimental` means the driver
/// reports the required version but the path is untested or under-tested upstream, so it may
/// fail at runtime and must be described that way to whoever is installing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LegacySupport {
    /// Vulkan 1.2+, a device UUID, and a driver the vendor tests: acceleration is expected.
    Supported,
    /// Vulkan 1.2+ and a UUID, but on a driver generation Mesa does not test: it may work, and
    /// it must not be promised.
    Experimental,
    /// A Vulkan driver exists but cannot meet reims-vgpu's requirement. No acceleration.
    UnsupportedVulkan,
    /// No Vulkan driver at all; OpenGL through Mesa only.
    OpenGlOnly,
}

impl LegacySupport {
    pub fn label(&self) -> &'static str {
        match self {
            LegacySupport::Supported => "fully supported",
            LegacySupport::Experimental => {
                "experimental: hasvk reports 1.2 but Mesa does not test this generation"
            }
            LegacySupport::UnsupportedVulkan => "Vulkan present but below 1.2: not supported",
            LegacySupport::OpenGlOnly => "software rendering only",
        }
    }

    /// Whether the accelerated macOS path is reachable at all.
    ///
    /// `Experimental` counts, because the requirements are met; the difference is how loudly
    /// the result must be qualified, not whether it is attempted.
    pub fn accelerated(&self) -> bool {
        matches!(self, LegacySupport::Supported | LegacySupport::Experimental)
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
        let tested_by_vendor = !matches!(
            family,
            Some(IntelFamily::IvyBridge) | Some(IntelFamily::Haswell)
        );
        let support = if !c.meets_reims() {
            if c.api_version.0 > 0 {
                LegacySupport::UnsupportedVulkan
            } else {
                LegacySupport::OpenGlOnly
            }
        } else if tested_by_vendor {
            LegacySupport::Supported
        } else {
            // The gate is met, but on a generation Mesa does not test. Saying "supported" here
            // would be a promise this project cannot keep.
            LegacySupport::Experimental
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
            support: LegacySupport::Experimental,
            reason: "Haswell reaches Vulkan 1.2 through intel_hasvk with a device UUID, so the \
                     accelerated path should work; Mesa tests HASVK on Haswell but this project \
                     has not measured it, so treat it as experimental"
                .into(),
        },
        Some(IntelFamily::IvyBridge) => Classification {
            family,
            support: LegacySupport::Experimental,
            reason: "Ivy Bridge reaches Vulkan 1.2 through intel_hasvk, but Mesa runs no CI job \
                     on Gen7 and the driver prints 'Ivy Bridge Vulkan support is incomplete'; \
                     the accelerated path may work and must be verified on the machine"
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

    /// Ivy Bridge does have a Vulkan driver, contrary to what this project first recorded:
    /// HASVK builds `gfx7` and reports 1.2. It is experimental because Mesa runs no Gen7 CI job
    /// and the driver itself warns that support is incomplete — not because it is absent.
    #[test]
    fn ivy_bridge_has_vulkan_through_hasvk() {
        let c = classify(0x8086, 0x0166, None);
        assert_eq!(c.family, Some(IntelFamily::IvyBridge));
        assert_eq!(c.support, LegacySupport::Experimental);
        assert!(
            c.support.accelerated(),
            "the gate is met, so it is attempted"
        );
        let (driver, version) = IntelFamily::IvyBridge.vulkan().unwrap();
        assert_eq!(driver, "intel_hasvk");
        assert_eq!(version, (1, 2));
    }

    /// Haswell reaches 1.2 through HASVK, so the version is not what stops it. It stays
    /// experimental because nobody has measured the path here.
    #[test]
    fn haswell_reaches_the_version_hasvk_reports() {
        let c = classify(0x8086, 0x0412, None);
        assert_eq!(c.family, Some(IntelFamily::Haswell));
        assert_eq!(c.support, LegacySupport::Experimental);
        let (driver, version) = IntelFamily::Haswell.vulkan().unwrap();
        assert_eq!(driver, "intel_hasvk");
        assert!(
            version >= (1, 2),
            "HASVK reports 1.2 on Gen7.5 (use_softpin is false, so ANV_API_VERSION_1_2), \
             got {version:?}"
        );
    }

    /// A live measurement overrides the table in both directions.
    #[test]
    fn measurement_outranks_the_table() {
        // A Broadwell that somehow reports only 1.0 is not usable, whatever the table says.
        let weak = classify(0x8086, 0x1616, Some(&caps((1, 0, 0), true, "anv")));
        assert_eq!(weak.support, LegacySupport::UnsupportedVulkan);
        // A Haswell reporting 1.2 with a UUID meets the gate, but stays experimental: the
        // generation is not vendor-tested, and the probe cannot tell us that it is stable.
        let strong = classify(0x8086, 0x0412, Some(&caps((1, 2, 0), true, "intel_hasvk")));
        assert_eq!(strong.support, LegacySupport::Experimental);
        assert!(strong.support.accelerated());
        // The same measurement on a Broadwell is a full endorsement, because Mesa tests Gen8.
        let bdw = classify(0x8086, 0x1616, Some(&caps((1, 3, 0), true, "anv")));
        assert_eq!(bdw.support, LegacySupport::Supported);
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
