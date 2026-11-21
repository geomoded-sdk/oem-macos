# OEM-macOS: notes for future work

## What this repo is

The OEM layer around [reims-vgpu](https://github.com/steelbrain/reims-vgpu). Upstream is built
and driven through `reims-ctl`; it is **not** modified. A reference clone lives at
`/workspace/vendor/reims-vgpu` with its QEMU fork in `vendor/qemu`.

Pinned upstream commit: `818868564e31f9023c074f73840de97542592c91`.

## Commands

```sh
make check                                  # fmt, clippy -D warnings, tests, manifest
cd oem && cargo test                        # 89 tests, incl. a live Vulkan probe
cd oem && cargo run -- gpu                  # the graphics decision on this machine
cd oem && cargo run -- report               # full report as JSON
tools/check-manifest.py                     # package names vs the live Arch repos
tools/measure-footprint.sh                  # host RAM/storage on this machine
systemd-analyze verify host/systemd/*.service host/systemd/*.path
shellcheck -S warning host/installer/oem-install host/bin/oem-veil tools/measure-footprint.sh
```

Rust is nightly (`rustc 1.101.0-nightly`); `cargo` is at `/home/openhands/.cargo/bin`.

## Invisible Linux: the three mechanisms

The product requirement is that a user never sees Linux. Three separate things make that true,
and each is installed by `host/installer/oem-install`, not by `make install-dist`:

1. **The kernel prints nothing.** The boot entry carries `quiet loglevel=0
   vt.global_cursor_default=0 systemd.show_status=no`.
2. **`oem-veil` blanks every VT** (`host/bin/oem-veil`, unit `oem-veil.service`, wanted by
   `sysinit.target`). It hides the cursor through `/sys/module/vt/parameters/cur_default`,
   clears tty1..tty12, and drops `kernel.printk` to 0 so nothing a module says later reappears.
3. **No getty exists.** `getty@.service`, `serial-getty@.service` and `autovt@.service` are
   masked with symlinks to `/dev/null`. `autovt@` is the one a VT switch asks for, so
   Ctrl+Alt+F2 gets a blank screen instead of a login prompt.

The veil cannot stop a VT switch — that is the kernel keyboard driver, not userspace. It
guarantees the target of a switch is black and has no shell. Recovery does not need a getty:
`oem-core boot` exits 3 and `oem-maintenance.service` starts a root shell on tty1.

## Silent installation

`oem-install --silent` asks nothing: it takes the only non-removable disk, the newest macOS,
and skips the confirmation. It refuses rather than guessing when more than one candidate disk
exists, because erasing the wrong disk is unrecoverable. `pick_only_disk` skips removable
media (the installer stick) and `loop`/`ram`/`zram`/`dm-`/`sr` devices.

The guest resolution is not a constant: `detect_resolution` reads the first connected
connector's preferred mode from `/sys/class/drm/*/modes`, so a 2560x1600 panel gets a
2560x1600 guest instead of a 1920x1080 window with black bars.

## The graphics gate, measured

The gate is `device_choice.rs` `Seen::candidate`: `api_version >= 1.2 && graphics_compute &&
uuid.is_some()`. Nothing else refuses a device.

The four dma-buf extensions (`VK_KHR_external_memory_fd`, `VK_EXT_external_memory_dma_buf`,
`VK_EXT_image_drm_format_modifier`, `VK_EXT_queue_family_foreign`) only choose between an
import and a copy. Do not treat a missing one as a refusal: that hides working hardware and
this project already made that mistake once.

Reproduce the proof without KVM, which is the only way to test this in a container:

```sh
# The QEMU fork needs libslirp, libusb and libpipewire at run time.
sudo apt-get install -y libslirp0 libusb-1.0-0 libpipewire-0.3-0 \
    mesa-vulkan-drivers vulkan-tools
cd /workspace/vendor/reims-vgpu
XDG_RUNTIME_DIR=/tmp/xdg ./build/qemu/qemu-system-x86_64 -accel tcg -machine q35 -m 2G \
    -display none -device reims-graphics-pci,resolution=1920x1080 -monitor none -serial none -S
```

It prints the degradations and stays up. `llvmpipe` lacks
`VK_EXT_image_drm_format_modifier`; the device still initialises, with "IOSurfaces are copied
for the GPU". `reims-ctl` always passes `-machine max-ram-below-4g=1G` (crates/reims-ctl/src/qemu.rs), so
drive QEMU with it too: without it the device starts but warns that a macOS guest's kext would
find no GPU region at all. Measured here: with the flag the warning is gone and the device still
initialises. The CI `vgpu-device` job passes the flag and fails if the warning reappears.

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

## The installer medium

`tools/build-iso.sh` builds it, `make iso` wraps that, and `.github/workflows/iso.yml` runs it on
a full VM (mkarchiso needs sysfs, devtmpfs and loop devices; a container on a shared runner has
neither of the first two).

Two traps cost real time and are worth remembering:

1. **`mkarchiso` copies `airootfs` with `cp -af --no-preserve=ownership,mode`.** Everything lands
   as 0644, and only paths listed in `profiledef.sh`'s `file_permissions` get an executable bit.
   An unlisted script is a data file, and the medium boots to a black screen. The permission
   entries are load-bearing, not tidiness.
2. **`oem-install` finds its data relative to itself** (`$here/../packages/packages.txt`,
   `$here/../../VERSION`). Anything installing it must preserve that tree shape and put a symlink
   on PATH; a loose copy in `/usr/bin` finds nothing. This was a real bug in both `install-dist`
   and `install_host_config`, fixed by installing `host/` under `/usr/lib/oem-macos/`.

`verify_iso` (inside `build-iso.sh`) checks the finished image, not the source tree: executables,
the units, the getty mask, the loader entry, the kernel and the El Torito record. The kernel and
initramfs are at `/arch/boot/x86_64/` in the ISO, *outside* the squashfs, so looking for them in
the unpacked root finds nothing on a good medium.

`tools/test-install-host-config.sh` covers the other half: what the medium writes to the target.
It calls `install_host_config` against an empty directory with only `arch-chroot` stubbed.

## Not done

- Nothing has booted on real hardware. No `/dev/kvm`, no GPU in this environment.
- The 500 MB host-memory target is unverified; `oem-core` itself is 8–62 MiB.
- `Config::qmp_socket` assumes `qmp.sock` beside the config (upstream's rule for a normal-length
  directory); the long-path fallbacks are not implemented.
- The manifest installs three vendors' Vulkan drivers (~75 MiB); a per-vendor profile would
  save 30–60 MiB.

## `hkidados/` is a separate project

`hkidados/` (HkiPadOS) is **not** part of the OEM layer: its own MIT licence, its own
README, no shared code. It reuses the community HakiOS XNU
(`oosexgang/hakios-xnu-kernel`, pinned `63fc2b4`) and analyses the iPadOS IPSW for
iPad13,4. Do not fold it into the `make check` pipeline or the installer.

Facts that cost time, so they are not re-derived:

- The iPadOS 27 `OS` (8.3 GB) and `Cryptex1,SystemOS` (2.4 GB) images are **AEA**; the key
  comes from `wkms.sd.apple.com` for an attested device. Only `Cryptex1,AppOS` (16 MiB) and
  `RestoreRamDisk` (245 MB) are readable.
- The HakiOS `tools/apfs/apfs.py` cannot open a **sealed** APFS tree (no checksum on the
  node, `BTNODE_NOHEADER`, children ordered by hash). 7-Zip 23+ can, so `hkipados/apfsx.py`
  uses it there and says so.
- 7-Zip extracts every App-cryptex file as **0 bytes** plus `name:com.apple.decmpfs` and a
  resource fork: all 869 are decmpfs type 14 = **LZBITMAP** (`ZBM\x09`). Nothing in 7-Zip or
  HakiOS decodes it.
- `RestoreRamDisk` is APFS at **offset 27**, not 0; 383 Mach-O, all arm64e, `launchd` is
  iOS 27.2.0 with 32 entitlements (`pivot-root`, `graftdmg`, `load-trust-cache`).
- Target kernel is `xnu-13432.42.3` (`MH_FILESET`, arm64e); HakiOS is `xnu-8796.121.2`
  arm64 (`DEVELOPMENT_ARM64_BCM2837`); the newest open-source XNU tag is `12377.121.6`.
- The open kernel **does** boot under QEMU: AAVMF + `prebuilt/loader.efi` +
  `xnuloader/mkpayload.py` with `prebuilt/hakios-kernel` and `prebuilt/userland/hello`
  reaches `bsd_init: done` and runs pid 1 (`Hello from user space on Darwin!`). Needs
  `qemu-system-arm` and `qemu-efi-aarch64`; `make qemu-open` would rebuild and is not needed.
- The DeviceTree decompresses with `lzfse` to 260 KB and names the hardware: `aic,1`,
  `dart,t8020`, `ans`, `gpu,t8103`, `dcp`, `sep`, `ane,t8020`, `wlan-pcie,bcm4387`.
