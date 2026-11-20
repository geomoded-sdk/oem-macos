# Measurements

Every number here was produced on the machine that ran this build, by a command in this
repository. Nothing is estimated from a datasheet, and nothing is carried over from another
platform.

## What this machine is, and what that means

The build environment is a container: no `/dev/kvm`, no GPU, no display. So:

- **Build results are real.** The library, QEMU, `reims-ctl` and `oem-core` all compiled here.
- **Runtime results on target hardware are not claimed.** There is no Intel HD 4000 or HD 4600
  in this container, so their performance is not measured and is not reported.

## Host storage

| Component | Size | How |
|---|---|---|
| 51 packages from `host/packages/packages.txt` | 449.6 MiB | `installed_size` summed from the live Arch package API |
| `qemu-system-x86_64`, stripped | 77 MiB | `strip` on the built binary |
| `libreims_vgpu.so`, stripped | 41 MiB | `strip --strip-unneeded` |
| `oem-core`, stripped | 0.95 MiB | `strip` on the release build |
| **Total** | **~568 MiB** | |

That is inside the 1 GB target with about 450 MiB to spare, and the guest's disk is not in the
figure — it is tens of GB and is counted separately, as it must be.

Two things are worth saying plainly rather than leaving in a table:

1. **The manifest carries drivers for three GPU vendors.** `vulkan-intel` (43.7 MiB),
   `vulkan-radeon` (17.6 MiB) and `vulkan-swrast` (13.7 MiB) are all installed, but a given
   machine uses one. A per-vendor profile chosen at install time would save 30–60 MiB. It is not
   done yet because the installer would then need to know the GPU before `pacstrap`, and a
   manifest that is wrong for the hardware is worse than one that is 60 MiB too large.
2. **`linux` is 151.5 MiB and `mesa` is 54.6 MiB.** These are the two largest items and both are
   irreducible without a custom kernel or a trimmed Mesa, which the brief allows only where a
   measured improvement justifies it. No such measurement exists here, so neither was done.

## Host memory

Measured with `os.wait4` peak RSS, which is the kernel's own accounting of the process:

| Run | Peak RSS | Time |
|---|---|---|
| `oem-core doctor` (host checks, no Vulkan) | 8.0 MiB | 25 ms |
| `oem-core gpu` (loads the Vulkan loader) | 62.4 MiB | 30 ms |
| `oem-core report` | 62.4 MiB | 52 ms |

`oem-core` is not the host's memory. The host's memory is systemd plus the kernel plus QEMU's
own overhead; QEMU's guest RAM is the guest's, and is excluded by design. **The 500 MB host
figure is a target that this project has not verified**, because verifying it needs a booted
OEM-macOS machine and this is a container. Saying otherwise would be the fabrication the brief
forbids.

## What the graphics probe found here

`oem-core gpu` on this machine:

```
not accelerated: reims-graphics-pci on llvmpipe (LLVM 19.1.7, 256 bits)
                 (Vulkan 1.4.305, CPU rendering)
```

This is the correct answer, and it is the one that took a bug fix to get right. llvmpipe
reports Vulkan 1.4 *and* a `deviceUUID`, so it passes reims-vgpu's technical gate. It renders
on the CPU. The probe originally missed the UUID because the Vulkan instance was created as
1.0, and `VkPhysicalDeviceIDProperties` is a 1.1 structure that a driver need not fill for a
1.0 instance. With the instance at 1.1 the UUID is reported, and the backend is classified as
`VulkanOnCpu` rather than `Accelerated`.

## Reproducing

```sh
tools/check-manifest.py          # every package name against the live Arch repos
tools/measure-footprint.sh       # host RAM and storage on the machine running it
cd oem && cargo test             # the unit tests, including the live Vulkan probe
cd oem && cargo run -- gpu       # the graphics decision on this machine
```
