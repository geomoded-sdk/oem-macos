#!/bin/sh
# Monta o ramdisk de restauração do iPadOS como raiz do kernel do HakiOS.
#
# O payload do loader tem no máximo 63 MiB (PAYLOAD_MAX_SIZE, 64 MiB menos 1 MiB
# do log de pânico), então um ramdisk de 234 MB não cabe nele. O loader aceita,
# para isso, uma imagem que já esteja na memória: `rdres=<tamanho>` no payload e
# o QEMU põe o arquivo em `phys_base + RESIDENT_RD_OFFSET` (0x08000000). Sem
# entrada `ramdisk`, o md0 é essa imagem — e o md0 é a raiz.
#
# O que este script mede: o kernel do HakiOS monta o APFS do iPadOS 27 e tenta
# executar o `/sbin/launchd` de verdade; onde ele para está no log.
#
# Requer: qemu-system-aarch64, o firmware AAVMF, o clone do HakiOS com
# prebuilt/ (HAKIOS=...) e o IPSW em cache/ (IPSW=... para outro caminho).
set -eu

ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
HAKIOS=${HAKIOS:-$ROOT/../vendor/hakios-xnu-kernel}
IPSW=${IPSW:-$(ls "$ROOT"/cache/*.ipsw 2>/dev/null | head -1)}
OUT=${OUT:-$ROOT/reports/ipados-root.log}
SECS=${SECS:-240}
MEM=${QEMU_MEM:-4G}
# medido com -m 4G: "phys window 48000000 ..." — o loader imprime o endereço que
# usou e o script confere contra este.
PHYS_BASE=${PHYS_BASE:-0x48000000}
RD_ADDR=$(printf '0x%x' $((PHYS_BASE + 0x08000000)))

[ -n "$IPSW" ] && [ -f "$IPSW" ] || {
    echo "não achei o IPSW; passe IPSW=/caminho/arquivo.ipsw" >&2
    exit 1
}
command -v qemu-system-aarch64 >/dev/null 2>&1 || {
    echo "falta qemu-system-aarch64 (apt install qemu-system-arm qemu-system-ipxe)" >&2
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

WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT INT TERM
cp "$VARS" "$WORK/vars.fd"
IMAGE=$WORK/rd.apfs

echo "1/3 o ramdisk do IPSW"
PYTHONPATH=$ROOT python3 - "$IPSW" "$IMAGE" <<'PY'
import sys, zipfile
from hkipados import im4p
from hkipados.manifest import BuildManifest

ipsw, out = sys.argv[1], sys.argv[2]
archive = zipfile.ZipFile(ipsw)
# A mesma build traz três `.dmg` e cada um pertence a uma identidade (aparelho):
# pegar o primeiro `rdsk` do ZIP dá o ramdisk de outro iPad. O nome tem que vir
# do BuildManifest, na identidade do iPad13,4.
manifest = BuildManifest.load(archive.read("BuildManifest.plist"))
# A variante manda: "Customer Erase Install" e "Customer Upgrade Install" trazem
# ramdisks diferentes (1.260 e 1.519 arquivos). O alvo deste projeto é o Erase.
variants = ["Erase"] + [v for v in manifest.variants("iPad13,4") if v != "Erase"]
wanted = []
for variant in variants:
    identity = manifest.identity("iPad13,4", variant)
    path = identity.path_of("RestoreRamDisk") if identity else None
    if path and path not in wanted:
        wanted.append(path)
        print("  %s -> %s" % (variant, path))
if not wanted:
    sys.exit("o BuildManifest não tem RestoreRamDisk para iPad13,4")

chosen = next((info for name in wanted for info in archive.infolist() if info.filename == name), None)
if chosen is None:
    sys.exit("nenhum dos %s está no IPSW" % wanted)
for info in [chosen]:
    with archive.open(info) as handle:
        kind, description, offset, length = im4p.locate(handle.read(4096))
        if kind != "rdsk":
            sys.exit("%s não é IM4P rdsk (%s)" % (info.filename, kind))
        handle.seek(offset)
        left = length
        with open(out, "wb") as dst:
            while left:
                block = handle.read(min(1 << 22, left))
                if not block:
                    sys.exit("%s acabou antes dos %d bytes do payload" % (info.filename, length))
                dst.write(block)
                left -= len(block)
        print("  %s: IM4P %s (%s), payload em %d, %d bytes" % (info.filename, kind, description, offset, length))
        break
else:
    sys.exit("nenhum dos %s está no IPSW" % sorted(wanted))
data = open(out, "rb").read(64)
if data[32:36] != b"NXSB":
    sys.exit("%s não começa com NXSB no bloco 0; não é APFS" % out)
PY

SIZE=$(wc -c < "$IMAGE" | tr -d ' ')
RDRES=$(printf '%x' "$SIZE")
echo "  imagem APFS em $IMAGE, $SIZE bytes; rdres=$RDRES, vai para $RD_ADDR"

echo "2/3 o payload (só kernel + rdres)"
cd "$HAKIOS"
ARGS=$(python3 tools/hakios-config.py args --defconfig defconfig)
python3 xnuloader/mkpayload.py -o "$WORK/payload.bin" \
    kernel=prebuilt/hakios-kernel --rdres "$RDRES" --cmdline "$ARGS"

echo "3/3 boot"
rm -f "$OUT.serial"
( cd qemu && timeout "$SECS" qemu-system-aarch64 \
    -M virt,gic-version=3 -cpu cortex-a76 -smp 1 -m "$MEM" \
    -accel tcg,tb-size=512 \
    -drive if=pflash,format=raw,readonly=on,file="$EFI" \
    -drive if=pflash,format=raw,file="$WORK/vars.fd" \
    -drive file=fat:rw:esp,format=raw,if=virtio \
    -device loader,file="$WORK/payload.bin",addr=0xF0000000,force-raw=on \
    -device loader,file="$IMAGE",addr="$RD_ADDR",force-raw=on \
    -display none -serial "file:$OUT.serial" \
    -monitor none -no-reboot >/dev/null 2>&1 ) || true

[ -s "$OUT.serial" ] || {
    echo "o QEMU não escreveu nada: o kernel não chegou a imprimir" >&2
    exit 1
}

python3 - "$OUT.serial" "$OUT" <<'PY'
import sys
src = open(sys.argv[1], "rb").read().decode("latin-1").splitlines()
keep = []
for line in src:
    line = line.replace("\r", "")
    printable = sum(1 for ch in line if 32 <= ord(ch) < 127)
    if line and printable / len(line) > 0.85:
        keep.append(line)
open(sys.argv[2], "w").write("\n".join(keep) + "\n")
PY
rm -f "$OUT.serial"

echo
grep -E "phys window|resident ramdisk at pa|Added memory device|BSD root|apfs: volume|load_init_program|exec of|panic\(|Hello from user space" "$OUT" | head -12 || true

used=$(sed -n 's/.*resident ramdisk at pa \([0-9a-f]*\).*/\1/p' "$OUT" | head -1)
placed=$(printf '%x' "$RD_ADDR")
if [ -n "$used" ] && [ "$used" != "$placed" ]; then
    echo
    echo "ATENÇÃO: o loader usou a imagem em $used, não em $placed." >&2
    echo "A imagem do QEMU ficou em $placed, então o md0 é lixo. Ajuste PHYS_BASE" >&2
    echo "(o loader imprime 'phys window' com a base que escolheu)." >&2
    exit 1
fi

echo
echo "log em $OUT"
