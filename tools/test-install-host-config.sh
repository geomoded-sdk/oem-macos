#!/bin/bash
# test-install-host-config: prove that install_host_config leaves a bootable tree behind.
#
# `verify_iso` checks the medium. It cannot check what the medium *writes to the target*, and
# that is where the failure mode lives: `oem-core.service` runs `/usr/bin/oem-core`, and
# `oem-install` finds its own data relative to itself. Both were wrong before, and neither shows
# up until a machine reboots into a disk with no guest and no diagnostic.
#
# So this calls install_host_config against an empty directory and asserts on the result. It
# stubs `arch-chroot`, which is the one call that needs a real target root, and nothing else.
#
# Runs as root in an Arch container:
#   tools/test-install-host-config.sh
set -uo pipefail

readonly REPO=${REPO:-$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)}
readonly STAGE=/usr/lib/oem-macos
readonly TARGET=${TARGET:-/tmp/target}

fail=0
ok()   { printf '  ok    %s\n' "$*"; }
bad()  { printf '  FAIL  %s\n' "$*"; fail=1; }

check_file() { [ -f "$1" ] && ok "$2" || bad "$2 ($1 is missing)"; }
check_exec() { [ -x "$1" ] && ok "$2" || bad "$2 ($1 is not executable)"; }

# --- stage the medium's layout, the way build-iso.sh does ---------------------
rm -rf "$STAGE" "$TARGET"
mkdir -p "$STAGE/payload" "$TARGET"
cp -a "$REPO/host" "$STAGE/host"
install -m644 "$REPO/VERSION" "$STAGE/VERSION"

# A real oem-core is not needed: install_host_config checks for an executable and copies it.
# Using the built binary when it exists keeps the test honest about the file it installs.
if [ -x "$REPO/oem/target/release/oem-core" ]; then
    install -m755 "$REPO/oem/target/release/oem-core" "$STAGE/payload/oem-core"
else
    printf '#!/bin/sh\nexit 0\n' > "$STAGE/payload/oem-core"
    chmod 755 "$STAGE/payload/oem-core"
fi

# --- call the function under test --------------------------------------------
# The dispatch line is neutralised in the staged copy so that sourcing the script does not run
# an install; every other line, including $here resolution, is exactly what the medium runs.
sed -i 's|^main "\$@"|:|' "$STAGE/host/installer/oem-install"
# The installer declares OEM_ROOT readonly at source time, so it has to come from the
# environment: assigning it afterwards fails.
export OEM_ROOT="$TARGET"
# shellcheck disable=SC1091
source "$STAGE/host/installer/oem-install"

arch-chroot() { echo "  (arch-chroot stubbed: $*)"; }

echo "running install_host_config against $TARGET"
if ! install_host_config; then
    echo "install_host_config returned non-zero"
    exit 1
fi

# --- assertions ---------------------------------------------------------------
echo "checking the installed tree"

# The unit's ExecStart is /usr/bin/oem-core. Without this file the unit fails as not-found, and
# a machine with no guest and no console gives no clue why.
check_exec "$TARGET/usr/bin/oem-core" "oem-core is installed and executable"

for unit in oem-core.service oem-maintenance.service oem-poweroff-on-guest-stop.service \
            oem-poweroff-on-guest-stop.path seatd.service; do
    check_file "$TARGET/usr/lib/systemd/system/$unit" "unit $unit"
done

check_file "$TARGET/etc/sysctl.d/90-oem.conf" "sysctl tunables"
check_file "$TARGET/etc/modules-load.d/oem.conf" "module list"
check_file "$TARGET/etc/mkinitcpio.conf" "initramfs configuration"

# oem-install resolves its data as $here/../..., so the installed copy must keep that shape and
# /usr/bin/oem-install must point into it rather than being a loose copy.
inst=$TARGET/usr/lib/oem-macos/host/installer/oem-install
check_exec "$inst" "the installer is installed as a tree, not a loose file"
check_file "$(dirname "$inst")/../packages/packages.txt" "the manifest is reachable from the installer"
check_file "$(dirname "$inst")/../bin/oem-veil" "the veil is reachable from the installer"
check_file "$TARGET/usr/lib/oem-macos/VERSION" "VERSION is where provenance looks for it"

if [ "$(readlink "$TARGET/usr/bin/oem-install")" = /usr/lib/oem-macos/host/installer/oem-install ]; then
    ok "/usr/bin/oem-install points at the tree"
else
    bad "/usr/bin/oem-install does not point at the tree"
fi

rm -rf "$STAGE" "$TARGET"
[ "$fail" -eq 0 ] || { echo "install_host_config leaves an unbootable target"; exit 1; }
echo "install_host_config is correct"
