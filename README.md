# OEM-macOS

A Linux host that boots straight into macOS and then stays out of the way.

This repository is the OEM layer around [reims-vgpu](https://github.com/steelbrain/reims-vgpu):
the boot orchestration, the graphics decision, the guest lifecycle, the installer, and the
recovery path. The virtual GPU itself is upstream and is not modified.

## Status, stated plainly

| Area | State |
|---|---|
| Upstream build (library, QEMU, `reims-ctl`) | **Built and verified** on this machine |
| OEM Core (detect, graphics, VM, power, recovery) | **Implemented, 82 tests passing** |
| Host package manifest | **51 packages, every name checked against the live Arch repos** |
| systemd boot orchestration | **Implemented, `systemd-analyze verify` clean** |
| Installer | **Implemented, `shellcheck` clean, syntax-verified** |
| Booted on real hardware | **Not done.** This environment has no KVM and no GPU |
| GPU performance on target hardware | **Not measured, and not claimed** |
| macOS guest actually booting | **Not demonstrated.** It needs a machine with `/dev/kvm` |

The distinction between "built" and "demonstrated" is the point of the table. Everything in
the first group was run; nothing in the second group is presented as if it were.

## The graphics question, answered

The brief asks for legacy Intel support, naming the HD 4000, 4400, 4600, 5000 and 5500. The
answer comes from what the drivers do, not from a compatibility table:

- **HD 4000 (Ivy Bridge, Gen7)** has **no Vulkan driver at all**. ANV starts at Gen8. OpenGL
  through Mesa is all there is.
- **HD 4400 / 4600 / 5000 (Haswell, Gen7.5)** have Vulkan, but only **1.0**, through
  `intel_hasvk`. reims-vgpu requires **1.2**.
- **HD 5500 (Broadwell, Gen8)** is the oldest family that reaches Vulkan 1.2 through ANV, and so
  the oldest that can host the accelerated macOS path.

So on the named legacy GPUs the accelerated path is not available, and `oem-core gpu` says so
with the reason. What OEM-macOS does offer there is an honest fallback: it installs, boots, and
reports that the guest has no acceleration, rather than silently producing a black window.

`oem-core` never decides this from the table alone. It asks the Vulkan loader, and the
measurement wins. The table exists only to explain the result.

## Quick start

```sh
make check          # format, lint, test, and validate the package manifest
cd oem && cargo run -- gpu        # what this machine can do
cd oem && cargo run -- report     # the full host and graphics report as JSON
```

On a machine being installed:

```sh
host/installer/oem-install --disk /dev/nvme0n1 --macos tahoe
```

## Layout

```
oem/                  OEM Core, in Rust
  crates/oem-core/
    src/detect/       host facts: vmx/svm, /dev/kvm, /dev/udmabuf, vm.max_map_count, PCI GPUs
    src/graphics/     the Vulkan probe, the legacy Intel table, the backend decision
    src/vm/           the one guest's config and lifecycle, through reims-ctl
    src/power/        host and guest power coordination
    src/recovery/     failed-boot counting and the maintenance path
host/
  packages/           the host package manifest, checked against the Arch repos
  systemd/            the boot units
  installer/          oem-install
  kernel/             sysctl and modules-load tuning
  mkinitcpio/         the initramfs configuration
docs/                 architecture, measurements, phase reports
tools/                check-manifest.py, measure-footprint.sh
```

## Documentation

- [`docs/architecture.md`](docs/architecture.md) — how the pieces fit, and why
- [`docs/measurements.md`](docs/measurements.md) — what was measured, and what was not
- [`docs/phases/`](docs/phases/) — the phase reports

## Licence

GPL-2.0-or-later, matching reims-vgpu. Upstream attribution is kept in `NOTICE`.
