#!/bin/sh
# Tenta iniciar binários do iPadOS como processo 1 do kernel do HakiOS, no QEMU.
#
# O kernel do HakiOS é xnu-8796.121.2 (Darwin 22.5, arm64). A userland do iPadOS
# 27 é arm64e. Este script mede, um caso por vez, onde o exec para — e guarda o
# log de cada caso em reports/init-attempts/.
#
# Os quatro casos, na ordem em que interessam:
#   hello        o binário estático do HakiOS, que já se sabe que roda (controle)
#   hello-arm64e o mesmo binário com o cpusubtype trocado para arm64e
#   launchd      o /sbin/launchd do RestoreRamDisk, arm64e e com 10 dylibs
#   webpushd     uma fatia arm64 (cpusubtype zerado) de um binário do cryptex
#
# Requer: qemu-system-aarch64, o firmware AAVMF, e o clone do HakiOS com
# prebuilt/ (HAKIOS=...). Requer `make userland` para os binários do iPadOS.
set -eu

ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
HAKIOS=${HAKIOS:-$ROOT/../vendor/hakios-xnu-kernel}
RAMDISK=${RAMDISK:-$ROOT/build/userland/restore-ramdisk}
CRYPTEX=${CRYPTEX:-$ROOT/build/userland/app-cryptex}
OUT=${OUT:-$ROOT/reports/init-attempts}
SECS=${SECS:-90}
MEM=${QEMU_MEM:-3G}

command -v qemu-system-aarch64 >/dev/null 2>&1 || {
    echo "falta qemu-system-aarch64 (apt install qemu-system-arm qemu-system-data ipxe-qemu)" >&2
    exit 1
}
EFI=
for f in /usr/share/edk2/aarch64/QEMU_EFI.fd /usr/share/AAVMF/AAVMF_CODE.fd; do
    [ -f "$f" ] && { EFI=$f; break; }
done
VARS=
for f in /usr/share/edk2/aarch64/QEMU_VARS.fd /usr/share/AAVMF/AAVMF_VARS.fd; do
    [ -f "$f" ] && { VARS=$f; break; }
done
[ -n "$EFI" ] && [ -n "$VARS" ] || {
    echo "falta o firmware UEFI aarch64 (apt install qemu-efi-aarch64)" >&2
    exit 1
}
[ -f "$HAKIOS/xnuloader/mkpayload.py" ] || {
    echo "o HakiOS não está em $HAKIOS; passe HAKIOS=/caminho" >&2
    exit 1
}
[ -d "$RAMDISK" ] || {
    echo "falta $RAMDISK; rode: make userland" >&2
    exit 1
}

WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT INT TERM
mkdir -p "$OUT"
cp "$VARS" "$WORK/vars.fd"

# fatia um Mach-O universal e, se pedido, reescreve o cpusubtype (offset 8)
slice() {
    python3 - "$@" <<'PY'
import struct, sys
src, dst = sys.argv[1], sys.argv[2]
want = sys.argv[3] if len(sys.argv) > 3 else ""
subtype = sys.argv[4] if len(sys.argv) > 4 else None
data = open(src, "rb").read()
magic = int.from_bytes(data[:4], "big")
if magic not in (0xCAFEBABE, 0xCAFEBABF):
    out = bytearray(data)
else:
    count = struct.unpack_from(">I", data, 4)[0]
    out = None
    for index in range(count):
        if magic == 0xCAFEBABF:
            cpu, sub, off, size, _align, _res = struct.unpack_from(">iiQQII", data, 8 + index * 32)
        else:
            cpu, sub, off, size, _align = struct.unpack_from(">iiIII", data, 8 + index * 20)
        name = "arm64e" if (sub & 0x00FFFFFF) == 2 else "arm64"
        if name == want:
            out = bytearray(data[off:off + size])
            break
    if out is None:
        sys.exit("%s não tem fatia %s" % (src, want))
if subtype is not None:
    struct.pack_into("<I", out, 8, int(subtype, 0) & 0xFFFFFFFF)
open(dst, "wb").write(bytes(out))
PY
}

# boot <nome> <binário>: monta o payload e inicia o kernel com esse init
boot() {
    name=$1
    binary=$2
    python3 "$HAKIOS/xnuloader/mkpayload.py" -o "$WORK/$name.bin" \
        kernel="$HAKIOS/prebuilt/hakios-kernel" ramdisk="$binary" \
        --cmdline cpus=1 >/dev/null
    rm -f "$WORK/$name.serial"
    ( cd "$HAKIOS/qemu" && timeout "$SECS" qemu-system-aarch64 \
        -M virt,gic-version=3 -cpu cortex-a76 -smp 1 -m "$MEM" \
        -accel tcg,tb-size=384 \
        -drive if=pflash,format=raw,readonly=on,file="$EFI" \
        -drive if=pflash,format=raw,file="$WORK/vars.fd" \
        -drive file=fat:rw:esp,format=raw,if=virtio \
        -device loader,file="$WORK/$name.bin",addr=0xF0000000,force-raw=on \
        -display none -serial file:"$WORK/$name.serial" \
        -monitor none -no-reboot >/dev/null 2>&1 ) || true
    [ -s "$WORK/$name.serial" ] || {
        echo "$name: o QEMU não escreveu nada (o kernel nem chegou a imprimir)" >&2
        return 1
    }
    grep -aE "init_program|exec of|panic\(|Hello from user space" \
        "$WORK/$name.serial" > "$OUT/$name.log" || true
}

echo "caso 1/4: hello (controle, arm64 estático)"
boot hello "$HAKIOS/prebuilt/userland/hello"

echo "caso 2/4: hello com cpusubtype arm64e"
slice "$HAKIOS/prebuilt/userland/hello" "$WORK/hello-arm64e" "" 0x80000002
boot hello-arm64e "$WORK/hello-arm64e"

echo "caso 3/4: launchd do RestoreRamDisk (arm64e, 10 dylibs)"
boot launchd "$RAMDISK/sbin/launchd"

echo "caso 4/4: fatia arm64 de um binário do cryptex, cpusubtype zerado"
slice "$CRYPTEX/usr/libexec/webpushd" "$WORK/webpushd-arm64" arm64 0
boot webpushd "$WORK/webpushd-arm64"

echo
echo "o que cada caso disse:"
for name in hello hello-arm64e launchd webpushd; do
    line=$(grep -aE "exec of|panic\(|Hello from user space" "$OUT/$name.log" 2>/dev/null | head -1)
    printf '  %-12s %s\n' "$name" "${line:-sem linha decisiva}"
done
echo
echo "logs em $OUT"
