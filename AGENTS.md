# OEM-macOS: notes for future work

## What this repo is

The OEM layer around [reims-vgpu](https://github.com/steelbrain/reims-vgpu). Upstream is built
and driven through `reims-ctl`; it is **not** modified. A reference clone lives at
`/workspace/vendor/reims-vgpu` with its QEMU fork in `vendor/qemu`.

Pinned upstream commit: `818868564e31f9023c074f73840de97542592c91`.

## Commands

```sh
make check                                  # fmt, clippy -D warnings, tests, manifest
cd oem && cargo test                        # 83 tests, incl. a live Vulkan probe
cd oem && cargo run -- gpu                  # the graphics decision on this machine
cd oem && cargo run -- report               # full report as JSON
tools/check-manifest.py                     # package names vs the live Arch repos
tools/measure-footprint.sh                  # host RAM/storage on this machine
systemd-analyze verify host/systemd/*.service host/systemd/*.path
shellcheck -S warning host/installer/oem-install tools/measure-footprint.sh
```

Rust is nightly (`rustc 1.101.0-nightly`); `cargo` is at `/home/openhands/.cargo/bin`.

## Facts that were expensive to establish

Read these before changing graphics or power code.

1. **reims-vgpu is a virtual GPU, not passthrough.** No VFIO, no mediated devices, no SR-IOV.
   `reims-graphics-pci` (`1234:5247`) executes the guest's Metal on the host GPU through
   Vulkan. The device requires `apiVersion >= 1.2`, a graphics+compute queue, and a
   `deviceUUID` — `crates/reims-vgpu-vulkan/src/device_choice.rs`.
2. **The library owns its own SDL3 window**, so QEMU runs `-display none`
   (`qemu_window = !c.graphics().library() && ctx.screen`). Never try to wrap QEMU's window;
   there isn't one.
3. **ACPI S3 is disabled** upstream (`disable_s3` on `ICH9-LPC`, `docs/qemu.md`). A guest in S3
   never wakes with its display. So a guest sleep request must not suspend the host, and
   `power::policy` has a test pinning that.
4. **`reims-ctl start` detaches QEMU** and returns once QMP answers. It never learns why the
   guest later stopped. The only evidence is QEMU's QMP `SHUTDOWN` event, which exists only
   while the guest runs — hence `vm::qmp`, which watches the socket and records the reason.
5. **Legacy Intel — corrected once already, so verify before changing.** Ivy Bridge (HD 4000)
   and Haswell (HD 4400/4600/5000) both have Vulkan **1.2** via `intel_hasvk`, with a computed
   `deviceUUID`. The evidence, from Mesa 25.0.0 source: `vulkan_hasvk/meson.build` builds
   `gfx7`, `gfx75`, `gfx8`; `anv_device.c` sets `.apiVersion = use_softpin ? 1.3 : 1.2` where
   `use_softpin = !(ver < 8)`, so Gen7/7.5 get 1.2; `intel_uuid_compute_device_id` fills the
   UUID. ANV is the one that starts at Gen8 (`iris_pci_ids.h` has no Gen7 IDs).
   They are still classified **Experimental**, not Supported: Mesa's CI only runs HASVK on
   Haswell (`hasvk-hsw-*.txt`), the driver prints "Ivy Bridge Vulkan support is incomplete",
   and `has_a64_buffer_access = ver >= 8` means no `bufferDeviceAddress` on Gen7/7.5.
   An earlier version of this file said "Ivy Bridge has no Vulkan; Haswell is 1.0" — that was
   wrong and was corrected. Do not reintroduce it.

9. **Zink is not a Vulkan provider.** It is an OpenGL driver running over Vulkan, so it never
   appears in `vkEnumeratePhysicalDevices` and cannot satisfy reims-vgpu. `mesa` and
   `vulkan-mesa-layers` install **no ICD**; the ICDs come from `vulkan-intel`,
   `vulkan-radeon`, `vulkan-swrast`.
6. **lavapipe reports Vulkan 1.4 *and* a deviceUUID**, so it passes reims-vgpu's technical gate
   while rendering on the CPU. `Backend::VulkanOnCpu` exists so it is never reported as
   acceleration.
7. **`VkPhysicalDeviceIDProperties` needs a 1.1 instance.** At 1.0 lavapipe leaves the UUID
   zeroed, which silently made the probe think a CPU device had no UUID. `probe.rs` creates a
   1.1 instance; do not lower it.
8. **Arch package-name traps:** `iptables-nft` and `libva-mesa-driver` are *virtual* names
   (provided by `iptables` and `mesa`) and fail `pacman -S`. `reims-ctl` is not in the Arch
   repos at all; it is built from source.

## Conventions

- Tests use real temporary files and real subprocesses, not mocks. A `/sys` tree is built in
  `std::env::temp_dir()` and read back through `HostRoot`.
- Logging is one JSON object per line (`log::Log`), hand-escaped so it needs no serialiser.
- Every claim in docs distinguishes *built and run* from *not demonstrated*. Do not blur that.
- `docs/measurements.md` holds the numbers; regenerate them rather than editing them by hand.

## Not done

- Nothing has booted on real hardware. No `/dev/kvm`, no GPU in this environment.
- The 500 MB host-memory target is unverified; `oem-core` itself is 8–62 MiB.
- `Config::qmp_socket` assumes `qmp.sock` beside the config (upstream's rule for a normal-length
  directory); the long-path fallbacks are not implemented.
- The manifest installs three vendors' Vulkan drivers (~75 MiB); a per-vendor profile would
  save 30–60 MiB.
