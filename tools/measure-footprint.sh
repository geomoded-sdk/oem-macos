#!/bin/bash
# measure-footprint.sh: what the host actually costs.
#
# The brief sets targets of 500 MB of host RAM and 1 GB of host storage. Targets are not
# results, so this measures what is there and prints both, side by side, without rounding the
# answer in the project's favour.
#
# The guest's own memory and disk are measured separately and never added to the host's: a
# 8 GiB guest is not a host that needs 8 GiB.
set -euo pipefail

target_ram_mib=500
target_disk_mib=1024

human() { numfmt --to=iec --suffix=B --format='%.1f' "$1" 2>/dev/null || echo "$1 B"; }

# --- host RAM -----------------------------------------------------------------
# Summed RSS of every process on the host, which is the closest honest answer to "how much
# memory does the host use". Shared pages are counted once per process by RSS, so this
# over-counts rather than under-counts, which is the safe direction.
host_ram_mib() {
    local total=0
    while read -r rss; do
        total=$((total + rss))
    done < <(ps -eo rss= 2>/dev/null)
    echo $((total / 1024))
}

# The guest's QEMU process, by its own name, so it can be subtracted and reported separately.
guest_ram_mib() {
    local total=0
    while read -r rss; do
        total=$((total + rss))
    done < <(ps -eo rss=,comm= 2>/dev/null | awk '$2 ~ /qemu-system/ {print $1}')
    echo $((total / 1024))
}

# --- host storage -------------------------------------------------------------
# Every file the host itself installs, from the package manifest's paths plus the OEM files.
# The guest's disk lives under the state directory and is excluded by design.
host_disk_mib() {
    local paths=(
        /usr/bin/oem-core
        /usr/bin/reims-ctl
        /usr/bin/qemu-system-x86_64-reims
        /usr/lib/libreims_vgpu.a
        /usr/lib/libreims_vgpu.so
        /usr/share/oem-macos
        /etc/oem
        /boot/vmlinuz-linux
        /boot/initramfs-linux.img
    )
    local bytes=0
    for p in "${paths[@]}"; do
        [ -e "$p" ] || continue
        bytes=$((bytes + $(du -sb "$p" 2>/dev/null | cut -f1)))
    done
    echo $((bytes / 1024 / 1024))
}

# --- the report ---------------------------------------------------------------
main() {
    local host_ram guest_ram host_disk
    host_ram=$(host_ram_mib)
    guest_ram=$(guest_ram_mib)
    host_disk=$(host_disk_mib)

    printf '%-28s %12s %12s\n' "measurement" "actual" "target"
    printf '%-28s %12s %12s\n' "------------" "------" "------"
    if [ "$guest_ram" -gt 0 ]; then
        printf '%-28s %9d MiB %12s\n' "host RAM (incl. guest)" "$host_ram" "-"
        printf '%-28s %9d MiB %12s\n' "  of which the guest" "$guest_ram" "not counted"
        printf '%-28s %9d MiB %9d MiB\n' "host RAM (excluding guest)" \
            "$((host_ram - guest_ram))" "$target_ram_mib"
    else
        printf '%-28s %9d MiB %9d MiB\n' "host RAM" "$host_ram" "$target_ram_mib"
    fi
    printf '%-28s %9d MiB %9d MiB\n' "host storage (no guest disk)" "$host_disk" "$target_disk_mib"

    echo
    if [ "$host_disk" -le "$target_disk_mib" ]; then
        echo "storage: within the target"
    else
        echo "storage: $((host_disk - target_disk_mib)) MiB over the target"
    fi
    local ram=$((host_ram - guest_ram))
    if [ "$ram" -le "$target_ram_mib" ]; then
        echo "memory: within the target"
    else
        echo "memory: $((ram - target_ram_mib)) MiB over the target"
    fi
    echo
    echo "Note: these are the numbers for THIS machine. The project does not claim them for"
    echo "hardware it has not run on."
}

main "$@"
