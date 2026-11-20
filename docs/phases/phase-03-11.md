# Phase report: 3 through 11

The brief asks for a report after every phase: what was implemented, which files changed, what
the build produced, which tests failed, what is unresolved, and what comes next. This covers
the phases done in this session, grouped because they share one codebase.

## Implemented

### Phase 3 — OEM architecture design

`docs/architecture.md`. The load-bearing finding: reims-vgpu is a **virtual GPU that renders
through Vulkan on the host**, not a passthrough layer. That decided two things downstream —
the display design (the library owns its window, so QEMU runs `-display none`) and the
graphics decision (a Vulkan device meeting 1.2 with a UUID is required).

### Phase 4 — Minimal Arch Linux environment

- `host/packages/packages.txt` — 51 packages, no desktop, no display manager, no compositor.
- `tools/check-manifest.py` — validates every name against the live Arch repositories.
  Two names were wrong and were caught: `iptables-nft` is a virtual name provided by
  `iptables`, and `libva-mesa-driver` is provided by `mesa`. Listing either would have failed
  the install on the user's machine.
- `host/kernel/oem-sysctl.conf`, `host/kernel/oem-modules-load.conf` — each value with its
  reason; `vm.max_map_count` is the one the guest will not start without.
- `host/mkinitcpio/` — a small initramfs: `ext4` plus the systemd hooks, no guest images.

### Phase 5 — OEM Core

`oem/crates/oem-core/`. Five modules, each with tests:

| Module | What it does |
|---|---|
| `detect` | vmx/svm, `/dev/kvm` (opened, not just stat'd), `/dev/udmabuf`, `vm.max_map_count`, PCI display controllers from `/sys` |
| `graphics` | the Vulkan probe, the legacy Intel table, the backend decision |
| `vm` | the one guest's config, validated before a start; the `reims-ctl` seam |
| `power` | what to do when the guest or host asks for a power change |
| `recovery` | failed-boot counting with a safe failure mode |

### Phase 6 — Reims integration

Integration is through the interface upstream actually ships: `reims-ctl`. OEM Core builds its
arguments, reads `list --json`, and owns only what upstream has no opinion about. Nothing
upstream was modified. Verified against the real `reims-ctl` binary built from the pinned
commit — it answered `no VM named mac (reims-ctl list shows them)`, which is the correct
response to a start of a guest that does not exist.

The Vulkan probe is OEM Core's own, because upstream's `__vulkan-probe` reports only vendor and
device IDs and cannot answer whether a device meets reims-vgpu's requirement.

### Phase 7 — OEM VM

`Config` validation happens before any start: name, CPUs, at least 4096 MiB of memory, and the
four paths the guest cannot start without. The failure messages name the missing thing.

### Phase 8 — Fullscreen macOS

`seatd.service` gives the session `/dev/dri/card0`; there is no desktop to compete for it. The
library's own window is borderless and full-screen by its own settings, so OEM Display does not
wrap anything — there is no QEMU window to wrap.

### Phase 9 — Automated installer

`host/installer/oem-install`. Three questions, then everything else. It refuses a BIOS-mode
machine, a CPU without virtualization, a disk under 64 GiB, and a disk holding data unless the
user types `ERASE`. It reads the same package manifest the build uses, so the two cannot drift.
It records the upstream commit and the installed binary hashes.

### Phase 10 — Power management

`oem/crates/oem-core/src/power/`. The decision is a pure function and is tested for every
combination. **ACPI S3 is disabled in this platform** — upstream sets `disable_s3` because a
guest in S3 never wakes with its display — so a guest sleep request deliberately does *not*
suspend the host. A test pins that behaviour so it cannot be quietly changed.

### Phase 11 — Recovery and diagnostics

`oem/crates/oem-core/src/recovery/`. The counter is written to disk *before* the start and
cleared once the guest is confirmed up, so a hard power loss between those points still counts.
A damaged counter reads as zero, so a corrupt state directory cannot lock the user out.
Verified end to end: four failed boots, and the fourth printed
`maintenance mode: the guest has failed to start 3 times` and exited 3.

## Build results

| Artifact | Result |
|---|---|
| `libreims_vgpu.a` | built, 1.1 GB |
| `libreims_vgpu.so` | built, 268 MB (41 MB stripped) |
| `qemu-system-x86_64` | built, 355 MB (77 MB stripped), 1981/1981 targets |
| `reims-ctl` | built, 0.1.0 |
| `oem-core` | built, 1.3 MB (0.95 MB stripped) |

## Tests

**83 passing, 0 failing.** They include:

- the real Vulkan probe against this machine's loader, which found a genuine bug: the probe
  created a Vulkan 1.0 instance, and lavapipe does not fill the 1.1 `VkPhysicalDeviceIDProperties`
  for one, so the UUID was missed and a CPU device looked like a failed GPU. Fixed by asking
  for 1.1.
- real temporary `/sys` trees read back through the same code the host uses
- real shell scripts run as the engine, including their exit statuses
- a test that pins "a sleep request never suspends the host"

## Unresolved

1. **Nothing has booted on real hardware.** No `/dev/kvm` and no GPU here. The boot path is
   tested up to `reims-ctl start`; the guest's own boot is not.
2. **The 500 MB host memory target is unverified.** `oem-core` uses 8–62 MiB, but the host's
   total includes systemd, the kernel and QEMU's own overhead, which needs a booted machine.
3. **The manifest carries three vendors' Vulkan drivers** (~75 MiB total). A per-vendor profile
   would save 30–60 MiB but needs the GPU known before `pacstrap`.
4. ~~`guest-exited` reads its event from an environment variable that no unit sets.~~
   **Resolved.** `reims-ctl start` detaches QEMU, so it never learns why the guest stopped; the
   only evidence is QEMU's QMP `SHUTDOWN` event, which exists only while the guest runs. OEM
   Core now watches the QMP socket in the background and records `guest: true` as `shutdown`
   and anything else as `crashed`. Verified on the real binary: a recorded `shutdown` gives
   `PowerOff`, a recorded `crashed` gives `RestartGuest`, and a missing file gives
   `RestartGuest` — the recoverable direction.

## Next

1. On a machine with KVM and a Broadwell-or-newer Intel GPU: run `oem-core doctor`, then
   `oem-core boot`, and record what actually happens — including boot time and host RSS.
2. Measure the HD 4000 and HD 4600 paths separately, as the brief requires, once hardware is
   available. The expected result is that neither gets acceleration; the measurement is what
   turns that from a prediction into a fact.
3. Confirm the QMP socket path on a real VM. `Config::qmp_socket` assumes `qmp.sock` beside the
   config, which is upstream's rule for a normal-length directory; a very long path moves the
   socket, and that fallback is not implemented.
