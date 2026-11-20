#!/bin/bash
# build-iso: assemble the installer medium.
#
# This script runs inside an Arch container (see .github/workflows/iso.yml), because archiso's
# mkarchiso needs pacstrap, a loop device and the Arch repositories. It does three things:
#
#   1. builds `oem-core` for the host, so the medium carries the binary the installed disk needs
#   2. stages the profile from host/iso plus the repository files the installer reads at
#      install time (the package manifest, the units, the veil, the kernel tunables)
#   3. runs mkarchiso, which is the only part that cannot be reproduced by hand
#
# The staging is not cosmetic. `oem-install` locates its own data relative to itself
# (`$here/../packages/packages.txt`), so the medium must reproduce the repository's layout
# under a fixed prefix or the installer dies at `install_base` with "the package manifest is
# missing".
set -euo pipefail

readonly REPO=${REPO:-/work}
readonly OUT=${OUT:-$REPO/out}
readonly BUILD=${BUILD:-/var/tmp/iso-build}
readonly PROFILE_SRC=$REPO/host/iso
readonly PROFILE=$BUILD/profile
readonly PAYLOAD=/usr/lib/oem-macos/payload

log()  { printf '\033[1;34m==>\033[0m %s\n' "$*"; }
die()  { printf '\033[1;31mERR\033[0m %s\n' "$*" >&2; exit 1; }

# --- 1. the host binary -------------------------------------------------------
# Built for the host's own architecture with the `vulkan` feature, because oem-core queries the
# GPU's Vulkan capabilities at boot and a build without it reports nothing.
build_core() {
    log "building oem-core"
    command -v cargo >/dev/null || die "cargo is missing"
    ( cd "$REPO/oem" && cargo build --release --locked )
    local core=$REPO/oem/target/release/oem-core
    [ -x "$core" ] || die "oem-core did not build"
    install -Dm755 "$core" "$PAYLOAD/oem-core"
    # Recorded so the medium can be traced to a build, and so a mismatch is visible.
    sha256sum "$PAYLOAD/oem-core" > "$PAYLOAD/oem-core.sha256"
}

# --- 2. the profile -----------------------------------------------------------
# `oem-install` finds its data relative to itself, with paths that assume the repository's
# layout: `$here/../packages/packages.txt`, `$here/../bin/oem-veil`, and `$here/../../VERSION`
# for provenance. So the medium carries a copy of the repository's `host/` tree beside VERSION,
# under one prefix. Reproducing that shape is what makes the relative paths resolve; a flatter
# layout would die at `install_base` with "the package manifest is missing".
#
#   /usr/lib/oem-macos/VERSION                        what provenance reads
#   /usr/lib/oem-macos/host/installer/oem-install     the installer
#   /usr/lib/oem-macos/host/packages/packages.txt     what install_base reads
#   /usr/lib/oem-macos/host/systemd/*.service         what install_host_config copies
#   /usr/lib/oem-macos/host/bin/oem-veil              what install_invisibility copies
#   /usr/lib/oem-macos/payload/oem-core               what install_host_config installs
#
# `/usr/local/bin/oem-install` is a symlink to the first, so the medium's autostart and the
# installed system both call it by one name.
stage_profile() {
    log "staging the profile"
    rm -rf "$BUILD"
    mkdir -p "$BUILD"
    cp -a "$PROFILE_SRC" "$PROFILE"

    local root=$PROFILE/airootfs
    local oem=$root/usr/lib/oem-macos
    install -d "$root/usr/local/bin" "$oem/payload"

    # The repository's host/ tree, verbatim, so every `$here/..` in the installer resolves.
    cp -a "$REPO/host" "$oem/host"
    install -m644 "$REPO/VERSION" "$oem/VERSION"
    install -Dm755 "$PAYLOAD/oem-core" "$oem/payload/oem-core"

    ln -sf /usr/lib/oem-macos/host/installer/oem-install "$root/usr/local/bin/oem-install"

    # Fail here, not at install time, if the staged layout cannot satisfy the installer's own
    # relative paths. Each of these is a path the installer actually opens.
    local inst=$oem/host/installer/oem-install
    local here
    here=$(dirname "$inst")
    [ -f "$inst" ]                                        || die "the installer is not staged"
    [ -f "$here/../packages/packages.txt" ]               || die "the manifest path does not resolve"
    [ -f "$here/../bin/oem-veil" ]                        || die "the veil path does not resolve"
    [ -f "$here/../systemd/oem-core.service" ]            || die "the unit path does not resolve"
    [ -f "$here/../../VERSION" ]                          || die "the VERSION path does not resolve"
    log "profile staged at $PROFILE"
}

# --- 3. the medium ------------------------------------------------------------
build_iso() {
    log "running mkarchiso (this takes several minutes)"
    mkdir -p "$OUT"
    # `-v` so the log says which package failed if one does; `-w` keeps the work directory
    # outside the profile so a failed build can be inspected.
    mkarchiso -v -w "$BUILD/work" -o "$OUT" "$PROFILE" || die "mkarchiso failed"

    local iso
    iso=$(find "$OUT" -maxdepth 1 -name '*.iso' -print -quit)
    [ -n "$iso" ] || die "mkarchiso produced no .iso in $OUT"
    log "built $iso"
    ls -lh "$iso"
    sha256sum "$iso" | tee "$iso.sha256"
}

# --- 4. the check that the build itself cannot make ---------------------------
# mkarchiso reports success even when everything it staged is mode 0644, and a medium whose
# autostart is not executable boots to a black screen with no way to tell why. So the finished
# image is opened and the files that must run are checked inside the squashfs, not in the source
# tree, because it is the copy inside the image that boots.
verify_iso() {
    local iso=$1
    local tmp=$BUILD/verify
    rm -rf "$tmp"
    mkdir -p "$tmp"

    log "verifying $iso"
    xorriso -osirrox on -indev "$iso" \
        -extract "/arch/x86_64/airootfs.sfs" "$tmp/airootfs.sfs" >/dev/null 2>&1 \
        || die "could not extract the squashfs from $iso"
    unsquashfs -q -d "$tmp/root" "$tmp/airootfs.sfs" >/dev/null 2>&1 \
        || die "could not unpack the squashfs"

    local failed=0 f
    # Executables: the whole point of the medium is that these run.
    for f in usr/local/bin/oem-installer-autostart \
             usr/lib/oem-macos/payload/oem-core \
             usr/lib/oem-macos/host/installer/oem-install \
             usr/lib/oem-macos/host/bin/oem-veil; do
        if [ -x "$tmp/root/$f" ]; then
            printf '  executable  %s\n' "$f"
        else
            printf '  NOT EXECUTABLE  %s\n' "$f"
            failed=1
        fi
    done

    # Present but not necessarily executable: what the installer reads at install time.
    for f in usr/lib/oem-macos/host/packages/packages.txt \
             usr/lib/oem-macos/host/systemd/oem-core.service \
             usr/lib/oem-macos/VERSION \
             etc/systemd/system/oem-installer.service; do
        if [ -e "$tmp/root/$f" ]; then
            printf '  present     %s\n' "$f"
        else
            printf '  MISSING     %s\n' "$f"
            failed=1
        fi
    done

    # The invisible boot depends on these three: the installer enabled, getty masked, and the
    # one command the medium offers pointing at the installer.
    [ -L "$tmp/root/etc/systemd/system/multi-user.target.wants/oem-installer.service" ] \
        || { printf '  MISSING     oem-installer.service is not enabled\n'; failed=1; }
    [ "$(readlink "$tmp/root/etc/systemd/system/getty@tty1.service")" = /dev/null ] \
        || { printf '  WRONG       getty@tty1 is not masked\n'; failed=1; }
    [ "$(readlink "$tmp/root/usr/local/bin/oem-install")" \
        = /usr/lib/oem-macos/host/installer/oem-install ] \
        || { printf '  WRONG       /usr/local/bin/oem-install does not point at the installer\n'; failed=1; }

    # The kernel and the initramfs live outside the squashfs, at the path the loader entry names
    # (`/arch/boot/x86_64/...`). archiso moves them there so the bootloader can read them without
    # unpacking the image, so checking the squashfs for them finds nothing on a good medium.
    local listing=$tmp/listing
    xorriso -indev "$iso" -find / -type f -exec lsdl 2>/dev/null > "$listing" || true
    local want
    for want in arch/boot/x86_64/vmlinuz-linux arch/boot/x86_64/initramfs-linux.img \
                loader/loader.conf loader/entries/oem-macos.conf; do
        # xorriso quotes the path, so the line ends with '/path/to/file''
        if grep -q "/$want'$" "$listing"; then
            printf '  present     %s\n' "$want"
        else
            printf '  MISSING     %s\n' "$want"
            failed=1
        fi
    done

    # The El Torito record is what makes the image bootable at all.
    xorriso -indev "$iso" -report_el_torito plain 2>/dev/null | grep -q 'El Torito' \
        || { printf '  MISSING     no El Torito boot record\n'; failed=1; }

    rm -rf "$tmp"
    [ "$failed" -eq 0 ] || die "the built medium failed verification"
    log "the medium verifies"
}

main() {
    [ "$(id -u)" -eq 0 ] || die "build-iso must run as root: mkarchiso needs loop devices"
    build_core
    stage_profile
    build_iso
    local iso
    iso=$(find "$OUT" -maxdepth 1 -name '*.iso' -print -quit)
    verify_iso "$iso"
}

main "$@"
