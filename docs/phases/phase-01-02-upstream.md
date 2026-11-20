# Phase 1–2: reims-vgpu repository inspection and build

## Upstream pins (exact)

| Item | Value |
|---|---|
| reims-vgpu commit | `818868564e31f9023c074f73840de97542592c91` (master) |
| vendor/qemu submodule | `010bf82714f00d0f2758671d4a81402a6d94767f` (branch `steelbrain/reims-vgpu-rebase-2`) |
| QEMU version string | `QEMU emulator version 11.1.90` |
| rust-toolchain | `nightly-2026-11-03` |
| metal2vulkan rev | `911692b55de90b69448b1cd84804a4d2718b7f15` |
| License | `GPL-2.0-or-later OR LGPL-3.0-or-later` |

## What the repository actually is

`reims-vgpu` is a **virtual GPU for x86-64 macOS guests running in QEMU on Linux**.
It is not a general-purpose VMM and it is not a GPU passthrough project.

Four parts:

1. **`libreims_vgpu`** (`crates/`) — the host side of a PCI device, `reims-graphics-pci`
   (PCI ID `1234:5247`). It receives a Metal command stream from the guest and executes it on
   the host GPU through **`metal2vulkan`**, a Metal-shaped API implemented over Vulkan.
2. **`vendor/qemu`** — QEMU carrying `hw/display/reims-graphics-pci.c`, which links the library
   statically through `pkg-config` (`--enable-reims-vgpu`).
3. **`guest/`** — macOS kexts (`ReimsGraphics.kext`, `Graft.kext`, `RgfxGate.kext`) plus a Metal
   bundle, shipped prebuilt and injected into the VM's OpenCore image by `reims-ctl`.
4. **`reims-ctl`** — the CLI/TUI that creates, boots and manages VMs; it also drives
   `reims-util`, which builds the OpenCore image and downloads macOS from Apple.

`firmware/ReimsGraphics.rom` is the device's UEFI option ROM; the device has no default ROM,
so the `romfile` property must be set.

## Hard requirements the library imposes (read from source and docs)

- Host **x86-64 Linux** (or Windows/WHPX). A Mac host runs no `reims-ctl` VMs.
- **KVM** (`/dev/kvm`) and **Vulkan** with a physical device that:
  - reports `apiVersion >= 1.2`,
  - has a queue family supporting graphics + compute,
  - exposes a `deviceUUID`
  (`crates/reims-vgpu-vulkan/src/device_choice.rs`, `device_choice::choose`).
- **`/dev/udmabuf`** — guest memory reaches the GPU through it; without it the guest can crash
  at higher resolutions. Group ownership is usually `kvm`.
- Guest RAM must be a **shared memfd** (`memory-backend-memfd`), allocated whole at start
  (`prealloc=on`).
- `vm.max_map_count` matters: the library's views of guest memory take kernel mappings.
  Default 65,530 is often too low; the docs recommend 8,388,608 for a 16 GiB guest.
- ACPI S3 is **disabled** (`ICH9-LPC.disable_s3`): a guest in S3 never wakes with its display.
  macOS sleep therefore stops at a dark wake. This is a real constraint on Phase 10.
- Guest kexts load on **macOS 26.4 and later** only (upstream README).
- The display is drawn by the library into **an SDL3 window it owns**, not QEMU's window.
  SDL3 is built from pinned source and linked statically into the archive.

## Build results (measured in this environment)

Environment: Debian 13 (trixie) container, 4 vCPU, 16 GiB RAM, **no `/dev/kvm`, no GPU, no
Vulkan ICD, no display**. Container is not the target hardware; these are build results only.

| Artifact | Command | Result | Size |
|---|---|---|---|
| `libreims_vgpu.a` | `cargo xtask dist` | **success**, 3 m 15 s clean | 1.1 GiB (unstripped, includes Rust std + SDL3) |
| `libreims_vgpu.so` | `cargo xtask dist` | **success** | 257 MiB |
| `reims_vgpu.h`, `reims-vgpu.pc`, licenses | `cargo xtask dist` | **success** | — |
| `reims-ctl` | `cargo build --release` | **success**, 47 s | 27 MiB |
| `qemu-system-x86_64` | `make qemu` | **success**, 1981 ninja steps | 355 MiB (with debug info) |
| `reims-graphics-pci` device | `qemu-system-x86_64 -device help` | **present**, `desc "reims-graphics GPU"` | — |
| Accelerators in binary | `-accel help` | `tcg mshv nitro kvm` | — |

Build dependencies that had to be installed (Debian): `cmake pkg-config clang lld
build-essential meson ninja-build libglib2.0-dev libpixman-1-dev zlib1g-dev libslirp-dev
libusb-1.0-0-dev`, and for SDL3: `libx11-dev libxext-dev libxrandr-dev libxinerama-dev
libxcursor-dev libxi-dev libxss-dev libxtst-dev libwayland-dev libxkbcommon-dev
libxkbcommon-x11-dev libdecor-0-dev libdrm-dev libgbm-dev libegl1-mesa-dev libudev-dev
libpulse-dev libasound2-dev libpipewire-0.3-dev`.

### Build failures encountered (real, not hypothetical)

1. `sdl3-sys` build script aborted: *"SDL could not find X11 or Wayland development libraries"*.
   Fixed by installing the X11/Wayland dev packages above.
2. Second abort: *"Couldn't find dependency package for XTEST"*. Fixed with `libxtst-dev`.
3. A killed build left two `cargo rustc` processes holding the build-directory lock; the retry
   printed `Blocking waiting for file lock on build directory` and eventually failed. Fixed by
   killing the stale PIDs. **Lesson for the installer: never leave an orphaned cargo running.**

## Feature status as documented upstream

| Feature | Status |
|---|---|
| Vulkan GPU acceleration for x86-64 macOS guest | implemented, beta |
| Display in a library-owned SDL3 window (Wayland/X11) | implemented |
| Borderless window filling the screen | implemented (`REIMS_VGPU_WINDOW_BORDERLESS`) |
| Host video decode/encode (VA-API, NVDEC/NVENC) | implemented (`reims-vgpu-video`) |
| Audio (two virtio-sound devices + HDA for boot chime) | implemented |
| USB passthrough | implemented (`reims-ctl usb`) |
| macOS installer download from Apple + chunklist verification | implemented via `reims-util` |
| OpenCore image generation | implemented via `reims-util` |
| GPU passthrough / VFIO / mediated devices | **not implemented, not planned** |
| ACPI S3 suspend with working display resume | **not implemented** (S3 disabled by design) |
| Live migration | **not implemented** |
| arm64 guest via `reims-ctl` | **not implemented** (Ambrosia path exists separately) |
| Intel Gen7/Gen7.5 iGPU acceleration | **not supported** (below Vulkan 1.2 — see Phase 3 note) |
| Windows host | experimental, "nothing ships for them yet" |

## Consequence for OEM-macOS

Because passthrough is not implemented upstream, OEM-macOS must **not** promise direct GPU
passthrough. The acceleration path available is exactly one: the `reims-graphics-pci` device on
the host's Vulkan driver. Everything else in this project is the operating-system infrastructure
around that device — which is precisely what the brief asks OEM-macOS to provide.

The library owns its own window, so OEM Display must not try to wrap QEMU's window. The correct
integration is: give the library a screen it can present to (a KMS/DRM seat with no desktop), and
make its window borderless and full-screen.
