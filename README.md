# OEM-macOS

A Linux host that boots straight into macOS and then stays out of the way.

This repository is the OEM layer around [reims-vgpu](https://github.com/steelbrain/reims-vgpu):
the boot orchestration, the graphics decision, the guest lifecycle, the installer, and the
recovery path. The virtual GPU itself is upstream and is not modified.

## Status, stated plainly

| Area | State |
|---|---|
| Upstream build (library, QEMU, `reims-ctl`) | **Built and verified** on this machine |
| OEM Core (detect, graphics, VM, power, recovery) | **Implemented, 83 tests passing** |
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
answer was read out of Mesa 25.0.0's source and out of the driver binaries, and it is better
than this project first recorded — that earlier version of this file was wrong:

- **ANV** needs **Gen8 (Broadwell) or newer**. Gen7/7.5 IDs are absent from `iris_pci_ids.h`.
- **HASVK** (`intel_hasvk`) covers **Ivy Bridge (Gen7), Haswell (Gen7.5) and Bay Trail**. Its
  build lists `gfx7`, `gfx75` and `gfx8`, and it accepts a Gen7 device while warning that
  "Ivy Bridge Vulkan support is incomplete".
- **HASVK reports Vulkan 1.2, not 1.0.** `anv_device.c` sets
  `.apiVersion = use_softpin ? 1.3 : 1.2`, and `use_softpin = !use_relocations` where
  `use_relocations = ver < 8` — so Gen7 and Gen7.5 get **1.2**, and it computes a `deviceUUID`.

So the three things reims-vgpu checks — Vulkan 1.2, a graphics+compute queue, a device UUID —
are **met in principle on HD 4000 and HD 4600 alike**. That does not make them "supported":

| GPU | Driver | Reported | Classification |
|---|---|---|---|
| HD 4000 (Ivy Bridge) | `intel_hasvk` | 1.2 | **experimental** — no Mesa CI job on Gen7 |
| HD 4400/4600/5000 (Haswell) | `intel_hasvk` | 1.2 | **experimental** — Mesa tests HASVK on Haswell, this project has not measured it |
| HD 5500 (Broadwell) | `anv` | 1.3 | **supported** — vendor-tested generation |

What is genuinely weaker on Gen7/7.5 is `bufferDeviceAddress`, which is `false` there because
`has_a64_buffer_access = ver >= 8`. reims-vgpu reads that flag and enables it only where
present, so it is a limitation rather than a refusal — but nobody has run it.

`oem-core` never decides this from the table alone. It asks the Vulkan loader, and the
measurement wins. The table explains the result; the probe decides it.

### Why not Zink

Zink is an OpenGL driver that runs *over* Vulkan. It cannot help here: reims-vgpu needs a
Vulkan physical device, and Zink is a consumer of Vulkan, not a provider. It never appears in
`vkEnumeratePhysicalDevices` — verified on this machine, where `zink_dri.so` is installed and
`vulkaninfo` still lists only llvmpipe. Also note that `mesa` and `vulkan-mesa-layers` install
no Vulkan ICD at all; the ICDs come from `vulkan-intel`, `vulkan-radeon` or `vulkan-swrast`.

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
