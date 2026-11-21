"""O relatório: o que há no IPSW, o que é legível, e o que isso permite concluir."""

from __future__ import annotations

import hashlib
import os
import re
import tempfile

from . import aea, apfsx, decmpfs, im4p, macho
from .manifest import BuildManifest
from .source import open_source
from .zipx import ZipArchive, human_size

BOOT_COMPONENTS = ("iBoot", "iBSS", "iBEC", "LLB", "DeviceTree", "SEP",
                   "RestoreSEP", "KernelCache", "RestoreKernelCache", "StaticTrustCache")
IMAGE_COMPONENTS = ("OS", "Cryptex1,SystemOS", "Cryptex1,AppOS",
                    "RestoreRamDisk", "RestoreDeviceRamDisk")
DIGEST_ALGORITHMS = {20: "sha1", 32: "sha256", 48: "sha384"}
SMALL_IMAGE = 512 << 20


class BytesSource:
    """Um `Source` sobre bytes em memória, para as sondagens de cabeçalho."""

    def __init__(self, data: bytes):
        self.data = data
        self.size = len(data)

    def read(self, offset: int, length: int) -> bytes:
        return self.data[offset:offset + length]


def sha256_file(path: str) -> str:
    digest = hashlib.sha256()
    with open(path, "rb") as handle:
        for block in iter(lambda: handle.read(1 << 20), b""):
            digest.update(block)
    return digest.hexdigest()


def _kind_from_bytes(data: bytes) -> str:
    return apfsx.image_kind_bytes(data)["kind"]


def _im4p_report(archive: ZipArchive, path: str, digest: bytes, decompress: bool) -> dict:
    data = archive.read(path)
    report: dict = {"path": path, "bytes": len(data)}
    algorithm = DIGEST_ALGORITHMS.get(len(digest))
    report["manifest_digest"] = digest.hex()
    report["manifest_digest_algorithm"] = algorithm
    report["manifest_digest_ok"] = (hashlib.new(algorithm, data).digest() == digest
                                    if algorithm else None)
    try:
        envelope = im4p.parse(data)
    except im4p.Im4pError as error:
        report["error"] = str(error)
        return report
    report.update({
        "prefix": envelope.prefix,
        "type": envelope.type,
        "description": envelope.description,
        "payload_bytes": len(envelope.payload),
        "payload_magic": envelope.payload_magic,
        "compression": envelope.compression,
        "trailer_bytes": len(envelope.trailer),
    })
    if decompress and envelope.compression == "lzfse":
        try:
            raw = envelope.decompress()
            report["decompressed_bytes"] = len(raw)
            report["sha256_decompressed"] = hashlib.sha256(raw).hexdigest()
            report["macho"] = im4p.macho_summary(raw)
            if envelope.type == "krnl":
                text = raw
                found = text.find(b"Darwin Kernel Version")
                if found >= 0:
                    chunk = text[found:found + 160].split(b"\0", 1)[0]
                    report["darwin_version"] = chunk.decode("utf-8", "replace")
                    match = re.search(r"root:xnu-([0-9.]+)~(\d+)/RELEASE_(\w+)",
                                      report["darwin_version"])
                    if match:
                        report["xnu"] = match.group(1)
                        report["xnu_build"] = match.group(2)
                        report["release"] = match.group(3)
                    match = re.search(r"Darwin Kernel Version (\d+)\.(\d+)\.(\d+)",
                                      report["darwin_version"])
                    if match:
                        report["darwin_major"] = int(match.group(1))
                found = text.find(b"RELEASE_ARM64")
                if found >= 0:
                    chunk = text[found:found + 80].split(b"\0", 1)[0]
                    report["release_tag"] = chunk.decode("utf-8", "replace")
        except im4p.Im4pError as error:
            report["decompress_error"] = str(error)
    return report


def _image_report(archive: ZipArchive, component: str, path: str, size: int,
                  extract_dir: str | None) -> dict:
    report = {"component": component, "path": path, "size": size,
              "compressed": archive.members[path].csize}
    if path.endswith(".aea"):
        head = archive.read(path, limit=4096)
        try:
            header = aea.probe(BytesSource(head))
            report.update({"kind": "aea", **header.as_dict()})
        except aea.AeaError as error:
            report.update({"kind": "aea", "error": str(error)})
        return report
    if size <= SMALL_IMAGE:
        data = archive.read(path)
        info = apfsx.image_kind_bytes(data)
        report["kind"] = info["kind"]
        report.update({k: v for k, v in info.items() if k not in ("kind", "size")})
        if extract_dir:
            target = os.path.join(extract_dir, os.path.basename(path))
            os.makedirs(extract_dir, exist_ok=True)
            with open(target, "wb") as out:
                out.write(data)
            report["extracted"] = target
            report["seven_zip"] = apfsx.seven_zip_info(target)
            report["apfs_probe"] = apfsx.probe_container(target)
        return report
    report["kind"] = "udif" if path.endswith(".dmg") else "unknown"
    report["note"] = "grande demais para sondar sem extrair; use o comando userland"
    return report


def analyze_ipsw(path: str, product_type: str = "iPad13,4",
                 variant: str = "Erase", do_hash: bool = False,
                 decompress: bool = True, extract_dir: str | None = None) -> dict:
    source = open_source(path)
    archive = ZipArchive(source)
    manifest = BuildManifest.load(archive.read("BuildManifest.plist"))
    identity = manifest.identity(product_type, variant)
    report: dict = {
        "project": "HkiPadOS",
        "source": path,
        "size": source.size,
        "zip": {"members": archive.n_total, "zip64": archive.zip64,
                "central_directory_bytes": archive.cd_size},
        "manifest": {
            "product_version": manifest.product_version,
            "product_build": manifest.product_build,
            "supported_products": manifest.supported_products,
            "identities": len(manifest.identities),
        },
        "target": {
            "device": product_type,
            "version": manifest.product_version,
            "build": manifest.product_build,
            "size": source.size,
            "sha256": sha256_file(path) if do_hash and not path.startswith("http") else None,
        },
        "upstream": {
            "repo": "oosexgang/hakios-xnu-kernel",
            "pin": "63fc2b4633bac3af8e57dadab9f5a8d4cb940438",
            "kernel": "xnu-8796.121.2",
            "arch": "arm64",
            "boot": "loader EFI + fastboot/UEFI (Qualcomm/MediaTek)",
            "identity": "iPhone10,3/10,6 (d22ap)",
            "userland": "iOS 16.5 20F66",
        },
    }
    if identity is None:
        report["error"] = f"{product_type} não está no BuildManifest"
        return report
    report["target"].update({
        "board": identity.device_class,
        "variant": identity.variant,
        "restore_behavior": identity.restore_behavior,
        "platform": _platform_from_identity(identity),
    })

    components = []
    for name, component in sorted(identity.components.items()):
        member = archive.members.get(component.path or "")
        components.append({
            "name": name,
            "path": component.path,
            "in_ipsw": member is not None,
            "size": member.usize if member else None,
            "compressed": member.csize if member else None,
            "digest": component.digest_hex,
            "digest_algorithm": component.algorithm,
            "personalized": component.personalized,
        })
    report["components"] = components
    report["component_count"] = len(components)

    kernel = None
    kernel_path = identity.path_of("KernelCache")
    if kernel_path and kernel_path in archive.members:
        kernel = _im4p_report(archive, kernel_path,
                              identity.components["KernelCache"].digest, decompress)
        report["kernel"] = {
            "path": kernel_path,
            "type": kernel.get("type"),
            "description": kernel.get("description"),
            "payload_bytes": kernel.get("payload_bytes"),
            "compression": kernel.get("compression"),
            "decompressed_bytes": kernel.get("decompressed_bytes"),
            "darwin_version": kernel.get("darwin_version"),
            "darwin_major": kernel.get("darwin_major"),
            "xnu": kernel.get("xnu"),
            "xnu_build": kernel.get("xnu_build"),
            "release": kernel.get("release"),
            "release_tag": kernel.get("release_tag"),
            "arch": (kernel.get("macho") or {}).get("arch"),
            "cpusubtype": (kernel.get("macho") or {}).get("cpusubtype"),
            "platform": (kernel.get("macho") or {}).get("platform"),
            "minos": (kernel.get("macho") or {}).get("minos"),
            "chained_fixups": (kernel.get("macho") or {}).get("chained_fixups"),
        }
        report["kernel_raw"] = kernel

    devicetree_path = identity.path_of("DeviceTree")
    if devicetree_path and devicetree_path in archive.members:
        report["devicetree"] = _im4p_report(
            archive, devicetree_path, identity.components["DeviceTree"].digest, decompress=False)

    images = []
    for component in IMAGE_COMPONENTS:
        member_path = identity.path_of(component)
        if not member_path or member_path not in archive.members:
            continue
        images.append(_image_report(archive, component, member_path,
                                    archive.members[member_path].usize, extract_dir))
    report["images"] = images

    report["findings"] = []
    source.close()
    return report


def _platform_from_identity(identity) -> str:
    board = identity.device_class or ""
    if board.startswith("j5"):
        return "t8103 (Apple M1)"
    return board


def inventory_tree(root: str, max_binaries: int = 4000) -> dict:
    """Inventário de uma árvore extraída: arquivos, launchd e arquiteturas Mach-O."""
    files = 0
    directories = 0
    total = 0
    arch_counts: dict[str, int] = {}
    platform_counts: dict[str, int] = {}
    minos_counts: dict[str, int] = {}
    chained = 0
    binaries = 0
    launchds = []
    decmpfs_types: dict[str, int] = {}
    decmpfs_containers: dict[str, int] = {}
    decmpfs_files = 0
    fat_headers = 0
    fat_binaries = 0
    empty_files = 0
    for base, dirs, names in os.walk(root):
        directories += len(dirs)
        for name in names:
            files += 1
            full = os.path.join(base, name)
            try:
                size = os.path.getsize(full)
            except OSError:
                continue
            total += size
            if size == 0:
                empty_files += 1
            if name.endswith(":com.apple.decmpfs"):
                decmpfs_files += 1
                try:
                    with open(full, "rb") as handle:
                        header = decmpfs.parse(handle.read(16))
                    decmpfs_types[header.compression] = decmpfs_types.get(header.compression, 0) + 1
                except (OSError, ValueError):
                    decmpfs_types["inválido"] = decmpfs_types.get("inválido", 0) + 1
                continue
            if name.endswith(":com.apple.ResourceFork"):
                try:
                    with open(full, "rb") as handle:
                        head = handle.read(4096)
                    container = decmpfs.resource_container(head)
                    if container:
                        decmpfs_containers[container] = decmpfs_containers.get(container, 0) + 1
                    if decmpfs.find_fat(head):
                        fat_headers += 1
                except OSError:
                    pass
                continue
            if name in ("launchd", "backboardd") or name.endswith("backboardd"):
                launchds.append(full)
            if binaries >= max_binaries or size < 32:
                continue
            try:
                with open(full, "rb") as handle:
                    head = handle.read(32)
                    magic = int.from_bytes(head[:4], "little")
                    magic_be = int.from_bytes(head[:4], "big")
                    is_fat = magic_be in (macho.FAT_MAGIC, macho.FAT_MAGIC_64)
                    if not is_fat and magic != macho.MH_MAGIC_64:
                        continue
                    handle.seek(0)
                    data = handle.read(min(size, 1 << 20))
                parsed = macho.parse(data)
            except (OSError, ValueError):
                continue
            binaries += 1
            fat_binaries += 1 if is_fat else 0
            for slice_ in (parsed.fat_slices or [parsed]):
                arch_counts[slice_.arch] = arch_counts.get(slice_.arch, 0) + 1
            if parsed.platform:
                platform_counts[parsed.platform] = platform_counts.get(parsed.platform, 0) + 1
            if parsed.minos:
                minos_counts[parsed.minos] = minos_counts.get(parsed.minos, 0) + 1
            chained += 1 if parsed.chained_fixups else 0
    return {
        "root": root,
        "files": files,
        "directories": directories,
        "bytes": total,
        "binaries_scanned": binaries,
        "architectures": arch_counts,
        "platforms": platform_counts,
        "minos": dict(sorted(minos_counts.items(), key=lambda kv: -kv[1])[:10]),
        "chained_fixups": chained,
        "fat_binaries": fat_binaries,
        "empty_files": empty_files,
        "launchd": sorted(launchds),
        "decmpfs": {
            "streams": decmpfs_files,
            "types": decmpfs_types,
            "containers": decmpfs_containers,
            "fat_headers_in_literal_prefix": fat_headers,
        },
    }


def describe(path: str) -> str:
    """Uma linha para o `ls --long`."""
    source = open_source(path)
    archive = ZipArchive(source)
    manifest = BuildManifest.load(archive.read("BuildManifest.plist"))
    lines = [f"{path}: {human_size(source.size)}, {archive.n_total} membros, "
             f"iPadOS {manifest.product_version} ({manifest.product_build})"]
    for name in archive.order:
        member = archive.members[name]
        lines.append(f"  {human_size(member.usize):>10}  {name}")
    source.close()
    return "\n".join(lines)


def extract_members(path: str, outdir: str, names: list[str]) -> dict:
    source = open_source(path)
    archive = ZipArchive(source)
    os.makedirs(outdir, exist_ok=True)
    written = {}
    for name in names:
        target = os.path.join(outdir, os.path.basename(name))
        written[name] = archive.extract(name, target)
    source.close()
    return written
