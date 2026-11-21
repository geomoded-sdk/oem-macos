"""Mach-O: arquitetura, plataforma, versão mínima, assinatura e entitlements.

O que importa para a compatibilidade é pouco e cabe aqui:

- `arm64` (cpusubtype 0/1) ou `arm64e` (cpusubtype 2, com PAC);
- a plataforma e a versão mínima que o binário exige;
- se ele usa *chained fixups* (o formato de ligação moderno);
- os entitlements, que dizem o que o binário espera do kernel e do SEP.
"""

from __future__ import annotations

import struct
from dataclasses import dataclass, field

MH_MAGIC = 0xFEEDFACE
MH_MAGIC_64 = 0xFEEDFACF
FAT_MAGIC = 0xCAFEBABE
FAT_MAGIC_64 = 0xCAFEBABF

CPU_ARCH_ABI64 = 0x01000000
CPU_TYPE_ARM = 12
CPU_TYPE_ARM64 = CPU_ARCH_ABI64 | CPU_TYPE_ARM
CPU_SUBTYPE_MASK = 0xFF000000
CPU_SUBTYPE_ARM64_ALL = 0
CPU_SUBTYPE_ARM64_V8 = 1
CPU_SUBTYPE_ARM64E = 2

LC_SEGMENT_64 = 0x19
LC_CODE_SIGNATURE = 0x1D
LC_VERSION_MIN_IPHONEOS = 0x25
LC_ENCRYPTION_INFO_64 = 0x2C
LC_BUILD_VERSION = 0x32
LC_DYLD_CHAINED_FIXUPS = 0x80000034
LC_DYLD_INFO_ONLY = 0x80000022

PLATFORMS = {
    1: "macOS", 2: "iOS", 3: "tvOS", 4: "watchOS", 5: "bridgeOS",
    6: "macCatalyst", 7: "iOSSimulator", 8: "tvOSSimulator",
    9: "watchOSSimulator", 10: "driverKit", 11: "visionOS", 12: "visionOSSimulator",
}

FILETYPES = {
    1: "object", 2: "execute", 3: "fvmlib", 4: "core", 5: "preload",
    6: "dylib", 7: "dylinker", 8: "bundle", 9: "dylib_stub", 10: "dsym",
    11: "kext_bundle", 12: "fileset",
}

CSMAGIC_EMBEDDED_SIGNATURE = 0xFADE0CC0
CSMAGIC_EMBEDDED_ENTITLEMENTS = 0xFADE7171
CSMAGIC_EMBEDDED_DER_ENTITLEMENTS = 0xFADE7172
CSSLOT_ENTITLEMENTS = 5
CSSLOT_DER_ENTITLEMENTS = 6


def decode_version(value: int) -> str:
    return f"{value >> 16}.{(value >> 8) & 0xFF}.{value & 0xFF}"


@dataclass
class MachO:
    cputype: int
    cpusubtype: int
    filetype: int
    ncmds: int
    flags: int
    platform: str | None = None
    minos: str | None = None
    sdk: str | None = None
    chained_fixups: bool = False
    encrypted: bool = False
    entitlements_xml: str | None = None
    entitlements_der: bytes | None = None
    fat_slices: list["MachO"] = field(default_factory=list)
    error: str | None = None

    @property
    def arch(self) -> str:
        if self.cputype != CPU_TYPE_ARM64:
            return f"cpu{self.cputype:#x}"
        subtype = self.cpusubtype & 0x00FFFFFF
        if subtype == CPU_SUBTYPE_ARM64E:
            return "arm64e"
        if subtype == CPU_SUBTYPE_ARM64_V8:
            return "arm64v8"
        return "arm64"

    @property
    def filetype_name(self) -> str:
        return FILETYPES.get(self.filetype, f"filetype{self.filetype}")

    @property
    def ptrauth_abi(self) -> bool:
        return bool(self.cpusubtype & 0x80000000)

    def as_dict(self) -> dict:
        out = {
            "arch": self.arch,
            "cputype": f"0x{self.cputype:08x}",
            "cpusubtype": f"0x{self.cpusubtype & 0xFFFFFFFF:08x}",
            "ptrauth_abi": self.ptrauth_abi,
            "filetype": self.filetype_name,
            "platform": self.platform,
            "minos": self.minos,
            "sdk": self.sdk,
            "chained_fixups": self.chained_fixups,
            "encrypted": self.encrypted,
            "entitlements": _entitlement_keys(self.entitlements_xml),
        }
        if self.fat_slices:
            out["fat_slices"] = [s.as_dict() for s in self.fat_slices]
        if self.error:
            out["error"] = self.error
        return out


def _entitlement_keys(xml: str | None) -> list[str]:
    if not xml:
        return []
    import plistlib
    try:
        value = plistlib.loads(xml.encode())
    except Exception:  # noqa: BLE001
        return []
    if isinstance(value, dict):
        return sorted(value)
    return []


def parse(data: bytes) -> MachO:
    if len(data) < 28:
        raise ValueError("arquivo pequeno demais para um Mach-O")
    magic_le = struct.unpack_from("<I", data, 0)[0]
    magic_be = struct.unpack_from(">I", data, 0)[0]
    if magic_be in (FAT_MAGIC, FAT_MAGIC_64):
        return _parse_fat(data, magic_be)
    if magic_le == MH_MAGIC_64:
        return _parse_thin(data, 64)
    if magic_le == MH_MAGIC:
        return _parse_thin(data, 32)
    raise ValueError(f"não é Mach-O (magic 0x{magic_le:08x})")


def _parse_fat(data: bytes, magic: int) -> MachO:
    count = struct.unpack_from(">I", data, 4)[0]
    slices = []
    at = 8
    entry = 20 if magic == FAT_MAGIC else 32
    for _ in range(min(count, 16)):
        if entry == 20:
            _cpu, _sub, offset, size, _align = struct.unpack_from(">IIIII", data, at)
        else:
            _cpu, _sub, offset, size, _align, _res = struct.unpack_from(">IIQQII", data, at)
        at += entry
        if offset + size <= len(data):
            try:
                slices.append(_parse_thin(data[offset:offset + size], 64))
            except Exception as error:  # noqa: BLE001
                broken = MachO(0, 0, 0, 0, 0)
                broken.error = str(error)
                slices.append(broken)
    if not slices:
        raise ValueError("Mach-O universal sem fatias legíveis")
    chosen = next((s for s in slices if s.arch.startswith("arm64")), slices[0])
    chosen.fat_slices = slices
    return chosen


def _parse_thin(data: bytes, bits: int) -> MachO:
    if bits == 64:
        (magic, cputype, cpusubtype, filetype, ncmds, _sizeofcmds,
         flags, _reserved) = struct.unpack_from("<IiiIIIII", data, 0)
        at = 32
    else:
        (magic, cputype, cpusubtype, filetype, ncmds, _sizeofcmds,
         flags) = struct.unpack_from("<IiiIIII", data, 0)
        at = 28
    result = MachO(cputype, cpusubtype, filetype, ncmds, flags)
    code_signature = None
    for _ in range(ncmds):
        if at + 8 > len(data):
            result.error = "load commands truncados"
            break
        cmd, cmdsize = struct.unpack_from("<II", data, at)
        if cmdsize < 8 or at + cmdsize > len(data):
            result.error = f"load command inválido em {at}"
            break
        body = at + 8
        if cmd == LC_BUILD_VERSION and cmdsize >= 24:
            platform, minos, sdk, _ntools = struct.unpack_from("<IIII", data, body)
            # um kernelcache (MH_FILESET) costuma trazer estes campos zerados
            if platform:
                result.platform = PLATFORMS.get(platform, f"platform{platform}")
            if minos:
                result.minos = decode_version(minos)
            if sdk:
                result.sdk = decode_version(sdk)
        elif cmd == LC_VERSION_MIN_IPHONEOS and cmdsize >= 16:
            version, sdk = struct.unpack_from("<II", data, body)
            result.platform = "iOS"
            if version:
                result.minos = decode_version(version)
            if sdk:
                result.sdk = decode_version(sdk)
        elif cmd == LC_ENCRYPTION_INFO_64 and cmdsize >= 20:
            _off, _size, cryptid, _pad = struct.unpack_from("<IIII", data, body)
            result.encrypted = cryptid != 0
        elif cmd == LC_DYLD_CHAINED_FIXUPS:
            result.chained_fixups = True
        elif cmd == LC_CODE_SIGNATURE and cmdsize >= 16:
            code_signature = struct.unpack_from("<II", data, body)
        at += cmdsize
    if code_signature:
        result.entitlements_xml, result.entitlements_der = _entitlements(
            data, code_signature[0], code_signature[1])
    return result


def _entitlements(data: bytes, offset: int, size: int):
    blob = data[offset:offset + size]
    if len(blob) < 12:
        return None, None
    magic, _length, count = struct.unpack_from(">III", blob, 0)
    if magic != CSMAGIC_EMBEDDED_SIGNATURE:
        return None, None
    xml = None
    der = None
    for index in range(min(count, 64)):
        entry = 12 + index * 8
        if entry + 8 > len(blob):
            break
        slot, rel = struct.unpack_from(">II", blob, entry)
        if rel + 8 > len(blob):
            continue
        blob_magic, blob_len = struct.unpack_from(">II", blob, rel)
        content = blob[rel + 8:rel + blob_len]
        if slot == CSSLOT_ENTITLEMENTS and blob_magic == CSMAGIC_EMBEDDED_ENTITLEMENTS:
            xml = content.decode("utf-8", "replace")
        elif slot == CSSLOT_DER_ENTITLEMENTS and blob_magic == CSMAGIC_EMBEDDED_DER_ENTITLEMENTS:
            der = content
    return xml, der
