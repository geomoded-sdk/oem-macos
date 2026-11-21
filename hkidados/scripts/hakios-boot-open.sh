#!/bin/sh
# Inicia o kernel aberto do HakiOS no QEMU e guarda o log.
#
# É o único boot que não depende de hardware da Apple: o kernel é o do HakiOS
# (xnu-8796.121.2), o loader é o `prebuilt/loader.efi` (sem dados da Apple) e a
# userland é o `hello`, um binário estático que só usa syscalls cruas.
#
# Requer: qemu-system-aarch64 e o firmware AAVMF (Debian: qemu-system-arm,
# qemu-efi-aarch64). Não requer o IPSW.
set -eu

ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
HAKIOS=${HAKIOS:-$ROOT/../vendor/hakios-xnu-kernel}
SECS=${SECS:-120}
MEM=${QEMU_MEM:-3G}
OUT=${OUT:-$ROOT/reports/qemu-open.log}

command -v qemu-system-aarch64 >/dev/null 2>&1 || {
    echo "falta qemu-system-aarch64 (apt install qemu-system-arm)" >&2
    exit 1
}
for firmware in /usr/share/edk2/aarch64/QEMU_EFI.fd /usr/share/AAVMF/AAVMF_CODE.fd; do
    [ -f "$firmware" ] && break
done
[ -f "${firmware:-}" ] || {
    echo "falta o firmware UEFI aarch64 (apt install qemu-efi-aarch64)" >&2
    exit 1
}
[ -d "$HAKIOS" ] || {
    echo "HakiOS não está em $HAKIOS; rode scripts/fetch-hakios.sh" >&2
    exit 1
}

cd "$HAKIOS"
ARGS=$(python3 tools/hakios-config.py args --defconfig defconfig)
python3 xnuloader/mkpayload.py -o qemu/payload.bin \
    kernel=prebuilt/hakios-kernel \
    ramdisk=prebuilt/userland/hello \
    --cmdline "$ARGS"

# A linha do QEMU é a do qemu/run.sh do HakiOS (mesmos -M, -cpu, pflash, ESP e
# loader device), com uma diferença: a serial vai para um arquivo. O run.sh usa
# `-serial stdio`, e quando o `timeout` mata o QEMU o que está no buffer de
# bloco (4 KiB) se perde — o log saía vazio mesmo com o kernel tendo impresso
# tudo. A fonte do firmware foi achada acima; o VARS é o irmão do CODE.
VARS=$(mktemp)
trap 'rm -f "$VARS"' EXIT INT TERM
cp "$(dirname "$firmware")/$(basename "$firmware" | sed 's/CODE/VARS/')" "$VARS"
rm -f "$OUT.serial"
( cd qemu && timeout "$SECS" qemu-system-aarch64 \
    -M virt,gic-version=3 -cpu cortex-a76 -smp "${QEMU_SMP:-2}" -m "$MEM" \
    -accel tcg,tb-size="${QEMU_TB_MB:-384}" \
    -drive if=pflash,format=raw,readonly=on,file="$firmware" \
    -drive if=pflash,format=raw,file="$VARS" \
    -drive file=fat:rw:esp,format=raw,if=virtio \
    -device loader,file=payload.bin,addr=0xF0000000,force-raw=on \
    -display none -serial "file:$OUT.serial" \
    -monitor none -no-reboot >/dev/null 2>&1 ) || true

# o log traz bytes binários do stackshot; guarda só as linhas legíveis
python3 - "$OUT.serial" "$OUT" <<'PY'
import sys
try:
    src = open(sys.argv[1], "rb").read().decode("latin-1").splitlines()
except OSError:
    sys.exit("o QEMU não escreveu %s: o kernel não chegou a imprimir" % sys.argv[1])
keep = []
for line in src:
    line = line.replace("\r", "")
    printable = sum(1 for ch in line if 32 <= ord(ch) < 127)
    if line and printable / len(line) > 0.85:
        keep.append(line)
open(sys.argv[2], "w").write("\n".join(keep) + "\n")
PY
rm -f "$OUT.serial"

echo "log em $OUT"
grep -E "XNULOADER|Darwin Kernel Version|bsd_init: done|BSD root|Hello from user space|load_init_program|pid 1 exited" "$OUT" || true
