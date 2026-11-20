# OEM-macOS architecture

## What this is

A Linux host that exists to run one macOS guest, and then gets out of the way. The user sees
macOS. The host is not a desktop, has no display manager, and exposes no VM interface.

## The load-bearing decision

`reims-vgpu` is the only path to GPU acceleration for an x86-64 macOS guest on this platform.
It is **not** a passthrough project and does not implement VFIO, mediated devices or SR-IOV.
Read from the source and its docs, not assumed:

- The guest gets a PCI device, `reims-graphics-pci` (`1234:5247`), whose host side executes the
  guest's Metal command stream on the host GPU through Vulkan (`metal2vulkan`).
- The device requires a physical Vulkan device with `apiVersion >= 1.2`, a graphics+compute
  queue, and a `deviceUUID`.
- The library draws the display into **its own SDL3 window**, so QEMU runs `-display none`.

That last point decides the whole display design: OEM Display must not wrap QEMU's window,
because there is no QEMU window. It must give the library a screen to present to.

## Subsystems

| Subsystem | Responsibility | Where |
|---|---|---|
| OEM Boot | Order the boot; stop it with a readable reason | `host/systemd/`, `oem-core boot` |
| OEM Core | Detection, graphics decision, guest lifecycle, logging | `oem/crates/oem-core/` |
| Reims vGPU | The virtual GPU: Metal over Vulkan | upstream, pinned |
| OEM VM | The one guest's config and lifecycle | `oem/crates/oem-core/src/vm/` |
| OEM EFI | OpenCore + OVMF, made by upstream `reims-util` | upstream, driven by `reims-ctl` |
| OEM Installer | Guided install, Apple download, integrity | `host/installer/` |
| OEM Display | A KMS/DRM seat, no desktop, fullscreen | `host/systemd/seatd.service` |
| OEM Power | Host/guest power coordination | `oem/crates/oem-core/src/power/` |
| OEM Recovery | Failure counting, maintenance console | `oem/crates/oem-core/src/recovery/` |

## Why OEM Core wraps `reims-ctl` instead of reimplementing it

The OpenCore image, the Apple download with its chunklist verification, and the QEMU command
line must agree with upstream exactly. Reimplementing them would drift and break silently at
the next upstream change. So OEM Core builds `reims-ctl`'s arguments, reads its
machine-readable output (`list --json`), and owns only what upstream has no opinion about:
which guest, when, in what mode, and what the user sees on failure.

## Boot sequence

```
power on
  → systemd (no display manager, no getty)
  → seatd hands the session /dev/dri/card0
  → oem-core boot
      → host checks (vmx/svm, /dev/kvm, /dev/udmabuf, vm.max_map_count)
      → graphics decision (Vulkan probe → backend)
      → recovery counter (refuse after 3 failures)
      → reims-ctl start
  → macOS
```

Each arrow is a real unit or a real function; none is aspirational. `oem-core boot` returns 0
when the guest is up, 3 when it has failed too often, and 1 otherwise.

## The honest limits

These are not bugs to be fixed later; they are properties of the platform as it exists today.

1. **No GPU passthrough.** Not implemented upstream. The accelerated path is the virtual GPU
   only. Anything older than Broadwell cannot use it at all (see `docs/phases/`).
2. **ACPI S3 is disabled.** Upstream sets `disable_s3` because a guest in S3 never wakes with
   its display. So "macOS sleeps, host suspends, macOS resumes" is not implementable, and
   `oem_core::power` deliberately refuses to suspend the host on a guest sleep request.
3. **The host cannot be 1 GB with the guest's disk.** The 1 GB target is the *host's* footprint
   with the guest's storage excluded. The guest's disk is tens of GB and is counted separately,
   as the brief requires.
4. **No measured GPU performance on target hardware.** This environment has no `/dev/kvm` and
   no GPU. Build results are real; performance numbers are not claimed.
