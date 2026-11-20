# OEM-macOS

A Linux host that boots straight into macOS and then stays out of the way.

This repository is the OEM layer around [reims-vgpu](https://github.com/steelbrain/reims-vgpu):
the boot orchestration, the graphics decision, the guest lifecycle, the installer, and the
recovery path. The virtual GPU itself is upstream and is not modified.

## Status, stated plainly

| Area | State |
|---|---|
| Upstream build (library, QEMU, `reims-ctl`) | **Built and verified** on this machine |
| OEM Core (detect, graphics, VM, power, recovery) | **Implemented, 89 tests passing** |
| Host package manifest | **51 packages, every name checked against the live Arch repos** |
| systemd boot orchestration | **Implemented, `systemd-analyze verify` clean** |
| Installer | **Implemented, `shellcheck` clean, syntax-verified; `--silent` installs with no prompt** |
| Invisible Linux (no console, no getty, no cursor) | **Implemented**; a VT switch still reaches a *blank* terminal |
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
are **met in principle on HD 4000 and HD 4600 alike**. That is the whole gate: `Seen::candidate`
in `device_choice.rs` accepts a device on `api_version >= 1.2 && graphics_compute && uuid.is_some()`
and on nothing else.

There are four dma-buf extensions — `VK_KHR_external_memory_fd`, `VK_EXT_external_memory_dma_buf`,
`VK_EXT_image_drm_format_modifier`, `VK_EXT_queue_family_foreign` — that decide whether guest
RAM reaches the GPU **without a copy**. They do not decide whether the guest starts. A device
without them still runs; `reims-vgpu` copies the surfaces ("IOSurfaces are copied for the
GPU") and pays for it in bandwidth. An earlier revision of this project treated them as a
refusal and would have hidden working hardware. That was wrong, and this was the experiment
that showed it:

```
$ qemu-system-x86_64 -accel tcg -machine q35 -m 2G -display none \
    -device reims-graphics-pci,resolution=1920x1080 -S
warning: reims-graphics-pci: no guest RAM is a shared memfd in pages of the host's size:
         IOSurfaces are copied for the GPU
metal2vulkan: the device llvmpipe (LLVM 19.1.7, 256 bits): ...
[ran for the full 40 s until the timeout killed it]
```

`llvmpipe` lacks `VK_EXT_image_drm_format_modifier` and the device initialised anyway. So the
extension list is reported by `oem-core gpu` as a **degradation**, not as a failure.

That does not make HD 4000 and HD 4600 "supported":

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
