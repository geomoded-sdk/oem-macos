#!/usr/bin/env bash
# The installer medium's archiso profile.
#
# Derived from archiso's own `releng` profile, with three changes that matter:
#
#   * `iso_label` is OEMISO, not ARCH_<date>. The live environment's initramfs finds its root by
#     this label, and a medium whose label changes every build cannot be recognised by a script.
#   * UEFI only. The installer refuses a BIOS boot anyway (`check_uefi`), so shipping a syslinux
#     boot mode would offer a path that dies one step later.
#   * The package list is the installer's own, not releng's 200-package rescue toolkit.
#
# shellcheck disable=SC2034
iso_name="oem-macos"
iso_label="OEMISO"
iso_publisher="OEM-macOS"
iso_application="OEM-macOS installer"
iso_version="${OEM_VERSION:-$(date --date="@${SOURCE_DATE_EPOCH:-$(date +%s)}" +%Y.%m.%d)}"
install_dir="arch"
buildmodes=('iso')
bootmodes=('uefi.systemd-boot')
arch="x86_64"
pacman_conf="pacman.conf"
airootfs_image_type="squashfs"
airootfs_image_tool_options=('-comp' 'xz' '-Xbcj' 'x86' '-b' '1M' '-Xdict-size' '1M')
# mkarchiso copies airootfs with `cp -af --no-preserve=ownership,mode`, so every staged file
# lands as 0644 and only the entries here get an executable bit. Anything the medium runs must
# therefore be listed: without these, the installer and oem-core are data files, and the boot
# fails with "Permission denied" on the very first script.
#
# No /etc/shadow entry: the live medium has no password to protect. `root` logs in without one
# because the console never offers a login at all, and the only thing that runs there is
# oem-installer.service.
file_permissions=(
  ["/usr/local/bin/oem-installer-autostart"]="0:0:755"
  ["/usr/lib/oem-macos/payload/oem-core"]="0:0:755"
  ["/usr/lib/oem-macos/host/installer/oem-install"]="0:0:755"
  ["/usr/lib/oem-macos/host/bin/oem-veil"]="0:0:755"
)
