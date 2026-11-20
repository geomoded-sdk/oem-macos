//! Host detection: what the machine is, before anything is started.
//!
//! Everything here reads the kernel's own view — `/sys`, `/proc`, `/dev` — so the answers are
//! facts about the running host, not a guess from a model name. Each probe returns an
//! `Option`/`Vec` rather than an error, because a missing file is a legitimate answer
//! ("this host has no such thing"), and the report distinguishes that from a real failure.

use std::path::{Path, PathBuf};

use crate::graphics::PciGpu;

/// Where the kernel exposes what this module reads. Production uses [`HostRoot::system`]; the
/// tests point it at a directory they built, so every parser is exercised against real file
/// content without needing the hardware.
#[derive(Debug, Clone)]
pub struct HostRoot {
    pub sys: PathBuf,
    pub proc: PathBuf,
    pub dev: PathBuf,
}

impl HostRoot {
    pub fn system() -> Self {
        HostRoot {
            sys: PathBuf::from("/sys"),
            proc: PathBuf::from("/proc"),
            dev: PathBuf::from("/dev"),
        }
    }

    /// A root built from `base`, for tests.
    pub fn at(base: &Path) -> Self {
        HostRoot {
            sys: base.join("sys"),
            proc: base.join("proc"),
            dev: base.join("dev"),
        }
    }
}

/// The result of one check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Check {
    pub name: &'static str,
    pub ok: bool,
    /// What was found, or why the check failed. Always populated.
    pub detail: String,
}

impl Check {
    fn ok(name: &'static str, detail: impl Into<String>) -> Self {
        Check {
            name,
            ok: true,
            detail: detail.into(),
        }
    }

    fn fail(name: &'static str, detail: impl Into<String>) -> Self {
        Check {
            name,
            ok: false,
            detail: detail.into(),
        }
    }
}

/// Whether the CPU exposes the virtualization extensions QEMU needs for a usable guest.
///
/// The flags are read from `/proc/cpuinfo`, which is the same source QEMU's own check uses.
pub fn cpu_virtualization(proc_cpuinfo: &str) -> Check {
    let mut has_vmx = false;
    let mut has_svm = false;
    for line in proc_cpuinfo.lines() {
        if let Some(rest) = line.strip_prefix("flags") {
            for flag in rest.split_whitespace() {
                if flag == "vmx" {
                    has_vmx = true;
                }
                if flag == "svm" {
                    has_svm = true;
                }
            }
        }
    }
    if has_vmx {
        Check::ok("cpu virtualization", "Intel VT-x (vmx) is present")
    } else if has_svm {
        Check::ok("cpu virtualization", "AMD-V (svm) is present")
    } else {
        Check::fail(
            "cpu virtualization",
            "neither vmx nor svm is in /proc/cpuinfo: enable VT-x/AMD-V in the firmware",
        )
    }
}

/// Whether `/dev/kvm` exists and is openable. Existence alone is not enough: the device node
/// can be present with permissions that deny the user, which is the common failure.
pub fn kvm(root: &HostRoot) -> Check {
    let path = root.dev.join("kvm");
    match std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(&path)
    {
        Ok(_) => Check::ok(
            "kvm",
            format!("{} opens for reading and writing", path.display()),
        ),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Check::fail(
            "kvm",
            format!(
                "{} is missing: load the kvm module and enable virtualization",
                path.display()
            ),
        ),
        Err(e) => Check::fail(
            "kvm",
            format!(
                "{} cannot be opened ({e}): add the user to the kvm group (usermod -aG kvm) \
                 and log in again",
                path.display()
            ),
        ),
    }
}

/// Whether `/dev/udmabuf` exists. reims-vgpu hands guest memory to the GPU through it; without
/// it the guest can crash at higher resolutions (upstream `docs/architecture.md`).
pub fn udmabuf(root: &HostRoot) -> Check {
    let path = root.dev.join("udmabuf");
    if path.exists() {
        Check::ok("udmabuf", format!("{} is present", path.display()))
    } else {
        Check::fail(
            "udmabuf",
            format!(
                "{} is missing: load the udmabuf module (modprobe udmabuf), or the guest may \
                 crash above a low resolution",
                path.display()
            ),
        )
    }
}

/// `vm.max_map_count`, which the library's views of guest memory depend on.
pub fn max_map_count(sysctl_value: &str) -> Check {
    let value: u64 = sysctl_value.trim().parse().unwrap_or(0);
    // The upstream docs recommend 8,388,608 for a 16 GiB guest; 1,048,576 is enough for
    // smaller guests and is what this check asks for at minimum.
    const WANTED: u64 = 1_048_576;
    if value >= WANTED {
        Check::ok(
            "vm.max_map_count",
            format!("{value}, at least the {WANTED} needed"),
        )
    } else {
        Check::fail(
            "vm.max_map_count",
            format!(
                "{value} is below {WANTED}: raise it (sysctl -w vm.max_map_count={WANTED}) or \
                 the guest may fail to start"
            ),
        )
    }
}

/// Every GPU on the PCI bus, from `/sys/bus/pci/devices`.
///
/// A display controller is class `0x03`; the class is read from the `class` attribute, so the
/// scan does not depend on a device-ID table.
pub fn pci_gpus(root: &HostRoot) -> Vec<PciGpu> {
    let dir = root.sys.join("bus/pci/devices");
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut gpus = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        let read = |name: &str| std::fs::read_to_string(path.join(name)).ok();
        let Some(class) = read("class") else { continue };
        // "0x030000" is a VGA controller; "0x030200" a 3D controller; "0x038000" other display.
        let class = class.trim();
        let is_display = class.starts_with("0x03");
        if !is_display {
            continue;
        }
        let Some(vendor) = read("vendor") else {
            continue;
        };
        let Some(device) = read("device") else {
            continue;
        };
        let parse_id = |s: &str| u16::from_str_radix(s.trim().trim_start_matches("0x"), 16).ok();
        let (Some(vendor_id), Some(device_id)) = (parse_id(&vendor), parse_id(&device)) else {
            continue;
        };
        // The driver is a symlink named after the module; its file name is the module name.
        let driver = std::fs::read_link(path.join("driver"))
            .ok()
            .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()));
        let slot = entry.file_name().to_string_lossy().into_owned();
        gpus.push(PciGpu {
            vendor_id,
            device_id,
            driver,
            slot,
        });
    }
    gpus.sort_by(|a, b| a.slot.cmp(&b.slot));
    gpus
}

/// The GPUs that `reims-ctl gpus` reports, from the shape `GpuInfo` serialises
/// (`crates/reims-ctl/src/gpu.rs`): `name`, `vendor_id`, `device_id`, `kind`, `driver_file`.
///
/// This is upstream's own probe, and it deliberately reports only identity, not capability.
/// Capability comes from [`crate::graphics::probe`], which asks the loader directly. Both are
/// used: this one says which driver file serves which GPU, which the library needs to be told.
pub fn parse_gpu_probe(stdout: &str) -> Vec<(String, u32, u32, String)> {
    let mut out = Vec::new();
    for line in stdout.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        let Some(name) = v["name"].as_str() else {
            continue;
        };
        let vendor = v["vendor_id"].as_u64().unwrap_or(0) as u32;
        let device = v["device_id"].as_u64().unwrap_or(0) as u32;
        let kind = v["kind"].as_str().unwrap_or("other").to_owned();
        out.push((name.to_owned(), vendor, device, kind));
    }
    out
}

/// A full host report, as OEM Core writes it at boot and the installer shows before it starts.
#[derive(Debug, Clone)]
pub struct HostReport {
    pub checks: Vec<Check>,
    pub gpus: Vec<PciGpu>,
}

impl HostReport {
    /// Whether every check that must pass for a boot did pass.
    pub fn ready(&self) -> bool {
        self.checks.iter().all(|c| c.ok)
    }

    /// The failed checks, for the recovery screen.
    pub fn failures(&self) -> Vec<&Check> {
        self.checks.iter().filter(|c| !c.ok).collect()
    }
}

#[cfg(test)]
mod tests;
