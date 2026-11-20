//! Tests for host detection.
//!
//! The parsers take text and the probes take a [`HostRoot`], so every test here builds the real
//! file layout in a temporary directory and reads it back through the same code path the host
//! uses. Nothing is mocked: the temporary tree is a small but genuine `/sys` and `/proc`.

use super::*;
use std::fs;
use std::path::PathBuf;

/// A temporary directory removed when the guard drops.
struct TempTree(PathBuf);

impl TempTree {
    fn new(name: &str) -> Self {
        let base = std::env::temp_dir().join(format!(
            "oem-detect-{}-{}-{name}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(&base).expect("create temp tree");
        TempTree(base)
    }

    fn write(&self, rel: &str, contents: &str) {
        let p = self.0.join(rel);
        fs::create_dir_all(p.parent().unwrap()).expect("create parent");
        fs::write(&p, contents).expect("write");
    }

    fn root(&self) -> HostRoot {
        HostRoot::at(&self.0)
    }
}

impl Drop for TempTree {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

const CPUINFO_INTEL: &str = "processor\t: 0\nvendor_id\t: GenuineIntel\nflags\t\t: fpu vme de pse tsc msr pae mce cx8 apic sep mtrr vmx ssse3\n";

const CPUINFO_AMD: &str = "processor\t: 0\nvendor_id\t: AuthenticAMD\nflags\t\t: fpu vme de pse tsc msr pae mce cx8 apic sep svm ssse3\n";

const CPUINFO_NONE: &str = "processor\t: 0\nvendor_id\t: GenuineIntel\nflags\t\t: fpu vme de pse tsc msr pae mce cx8 apic sep\n";

#[test]
fn intel_vmx_is_recognised() {
    let c = cpu_virtualization(CPUINFO_INTEL);
    assert!(c.ok);
    assert!(c.detail.contains("vmx"));
}

#[test]
fn amd_svm_is_recognised() {
    let c = cpu_virtualization(CPUINFO_AMD);
    assert!(c.ok);
    assert!(c.detail.contains("svm"));
}

/// The failure has to name the fix, because the user sees only this line.
#[test]
fn missing_virtualization_says_what_to_enable() {
    let c = cpu_virtualization(CPUINFO_NONE);
    assert!(!c.ok);
    assert!(c.detail.contains("VT-x"), "{}", c.detail);
}

/// A "vmx" that appears in a model name, not in flags, must not count.
#[test]
fn vmx_outside_flags_is_ignored() {
    let info = "model name\t: Fake vmx CPU\nflags\t\t: fpu sse\n";
    assert!(!cpu_virtualization(info).ok);
}

#[test]
fn kvm_present_and_openable_passes() {
    let t = TempTree::new("kvm-ok");
    t.write("dev/kvm", "");
    let c = kvm(&t.root());
    assert!(c.ok, "{}", c.detail);
}

#[test]
fn kvm_absent_explains_itself() {
    let t = TempTree::new("kvm-missing");
    let c = kvm(&t.root());
    assert!(!c.ok);
    assert!(c.detail.contains("missing"), "{}", c.detail);
}

#[cfg(unix)]
#[test]
fn kvm_without_permission_is_its_own_failure() {
    use std::os::unix::fs::PermissionsExt;
    let t = TempTree::new("kvm-denied");
    t.write("dev/kvm", "");
    let p = t.0.join("dev/kvm");
    fs::set_permissions(&p, fs::Permissions::from_mode(0o000)).expect("chmod");
    let c = kvm(&t.root());
    // Root can open anything, so only assert the diagnosis when the open really failed.
    if !c.ok {
        assert!(c.detail.contains("kvm group"), "{}", c.detail);
    }
    let _ = fs::set_permissions(&p, fs::Permissions::from_mode(0o600));
}

#[test]
fn udmabuf_presence_and_absence() {
    let t = TempTree::new("udmabuf");
    assert!(!udmabuf(&t.root()).ok);
    t.write("dev/udmabuf", "");
    let c = udmabuf(&t.root());
    assert!(c.ok, "{}", c.detail);
    assert!(c.detail.contains("udmabuf"));
}

#[test]
fn max_map_count_threshold() {
    assert!(max_map_count("8388608").ok);
    assert!(max_map_count("1048576").ok);
    assert!(!max_map_count("65530").ok);
    // A file that cannot be parsed is a failure, not a pass.
    assert!(!max_map_count("").ok);
    assert!(!max_map_count("garbage").ok);
    // The failure tells the user the value to set.
    let c = max_map_count("65530");
    assert!(c.detail.contains("1048576"), "{}", c.detail);
}

/// A small but complete PCI tree: one Intel iGPU, one NVIDIA card, one non-display device.
#[test]
fn pci_scan_finds_only_display_controllers() {
    let t = TempTree::new("pci");
    t.write("sys/bus/pci/devices/0000:00:02.0/class", "0x030000\n");
    t.write("sys/bus/pci/devices/0000:00:02.0/vendor", "0x8086\n");
    t.write("sys/bus/pci/devices/0000:00:02.0/device", "0x0166\n");
    t.write("sys/bus/pci/devices/0000:01:00.0/class", "0x030200\n");
    t.write("sys/bus/pci/devices/0000:01:00.0/vendor", "0x10de\n");
    t.write("sys/bus/pci/devices/0000:01:00.0/device", "0x2684\n");
    // An NVMe controller, class 0x0108: not a GPU.
    t.write("sys/bus/pci/devices/0000:02:00.0/class", "0x010802\n");
    t.write("sys/bus/pci/devices/0000:02:00.0/vendor", "0x144d\n");
    t.write("sys/bus/pci/devices/0000:02:00.0/device", "0xa80a\n");

    let gpus = pci_gpus(&t.root());
    assert_eq!(gpus.len(), 2, "{gpus:?}");
    // Sorted by slot, so the report is stable.
    assert_eq!(gpus[0].slot, "0000:00:02.0");
    assert_eq!(gpus[0].vendor_id, 0x8086);
    assert_eq!(gpus[0].device_id, 0x0166);
    assert_eq!(gpus[0].id(), "8086:0166");
    assert_eq!(gpus[1].vendor_id, 0x10de);
}

#[test]
fn pci_scan_of_a_missing_tree_is_empty_not_an_error() {
    let t = TempTree::new("pci-empty");
    assert!(pci_gpus(&t.root()).is_empty());
}

/// A device whose id files are malformed is skipped, not panicked on.
#[test]
fn pci_scan_skips_malformed_entries() {
    let t = TempTree::new("pci-bad");
    t.write("sys/bus/pci/devices/0000:00:02.0/class", "0x030000\n");
    t.write("sys/bus/pci/devices/0000:00:02.0/vendor", "not-a-number\n");
    t.write("sys/bus/pci/devices/0000:00:02.0/device", "0x0166\n");
    assert!(pci_gpus(&t.root()).is_empty());
}

/// The exact JSON `reims-ctl gpus` writes, as `GpuInfo` serialises it.
#[test]
fn gpu_probe_parsing_matches_upstream_json() {
    let out = concat!(
        r#"{"name":"Intel HD Graphics 4600","vendor_id":32902,"device_id":1054,"kind":"integrated","driver_file":"/usr/share/vulkan/icd.d/intel_hasvk_icd.json","shares_driver":false,"shares_ids":false}"#,
        "\n",
        r#"{"name":"llvmpipe (LLVM 19.1.7, 256 bits)","vendor_id":65541,"device_id":0,"kind":"cpu","driver_file":"/usr/share/vulkan/icd.d/lvp_icd.json","shares_driver":false,"shares_ids":false}"#,
        "\n"
    );
    let gpus = parse_gpu_probe(out);
    assert_eq!(gpus.len(), 2);
    assert_eq!(gpus[0].0, "Intel HD Graphics 4600");
    assert_eq!(gpus[0].1, 0x8086);
    assert_eq!(gpus[0].2, 0x041e);
    assert_eq!(gpus[0].3, "integrated");
    assert_eq!(gpus[1].3, "cpu");
}

#[test]
fn gpu_probe_ignores_noise() {
    // Blank lines, a warning on stderr leaking into stdout, and a truncated object.
    let out = "\nno GPU found\n{\"name\":\"x\"\n{\"name\":\"ok\",\"vendor_id\":1,\"device_id\":2,\"kind\":\"other\"}\n";
    let gpus = parse_gpu_probe(out);
    assert_eq!(gpus.len(), 1);
    assert_eq!(gpus[0].0, "ok");
    assert_eq!(gpus[0].1, 1);
}

#[test]
fn report_readiness_follows_its_checks() {
    let t = TempTree::new("report");
    t.write("dev/kvm", "");
    t.write("dev/udmabuf", "");
    let report = HostReport {
        checks: vec![
            cpu_virtualization(CPUINFO_INTEL),
            kvm(&t.root()),
            udmabuf(&t.root()),
            max_map_count("8388608"),
        ],
        gpus: Vec::new(),
    };
    assert!(report.ready());
    assert!(report.failures().is_empty());

    let failing = HostReport {
        checks: vec![max_map_count("65530")],
        gpus: Vec::new(),
    };
    assert!(!failing.ready());
    assert_eq!(failing.failures().len(), 1);
    assert_eq!(failing.failures()[0].name, "vm.max_map_count");
}
