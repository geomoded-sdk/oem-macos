"""Quais bibliotecas cada binário extraído pede, e quais dessas existem na árvore.

O `dyld` do iPadOS não lê `/usr/lib/libSystem.B.dylib` do disco: desde o iOS 13
essas bibliotecas vivem no *dyld shared cache*, um arquivo só, fora de qualquer
volume que este projeto possa extrair (o cache é gerado na instalação, no
aparelho). Então a conta que interessa não é "quantas dependências estão
faltando", é "quantas estão no volume e quantas só existiriam no cache".

O módulo não adivinha: ele resolve cada caminho dentro da árvore extraída e
classifica o que não achou.
"""

from __future__ import annotations

import os
import struct

from . import macho

# caminhos que, no iPadOS, são servidos pelo dyld shared cache e por isso não
# aparecem em nenhum volume extraível
SHARED_CACHE_DIRS = ("/usr/lib/", "/System/Library/PrivateFrameworks/")


def _is_macho(head: bytes) -> bool:
    if len(head) < 8:
        return False
    magic_le = int.from_bytes(head[:4], "little")
    magic_be = int.from_bytes(head[:4], "big")
    return (magic_le == macho.MH_MAGIC_64
            or magic_be in (macho.FAT_MAGIC, macho.FAT_MAGIC_64))


def _read_thin_slice(path: str) -> bytes | None:
    """O começo de uma fatia: um universal pode ter a primeira bem longe do início."""
    with open(path, "rb") as handle:
        head = handle.read(32)
        if not _is_macho(head):
            return None
        magic_be = int.from_bytes(head[:4], "big")
        if magic_be not in (macho.FAT_MAGIC, macho.FAT_MAGIC_64):
            handle.seek(0)
            return handle.read(1 << 20)
        count = struct.unpack_from(">I", head, 4)[0]
        if magic_be == macho.FAT_MAGIC_64:
            entry = 8 + 32
            offset_at, offset_size = entry + 8, ">Q"
        else:
            entry = 8
            offset_at, offset_size = entry + 8, ">I"
        for index in range(min(count, 8)):
            at = offset_at + index * entry
            handle.seek(at)
            raw = handle.read(8 if offset_size == ">Q" else 4)
            if len(raw) < (8 if offset_size == ">Q" else 4):
                break
            offset = struct.unpack(offset_size, raw)[0]
            if offset == 0 or offset >= os.path.getsize(path):
                continue
            handle.seek(offset)
            return handle.read(1 << 20)
    return None


def _walk_binaries(root: str, limit: int):
    seen = 0
    for base, _dirs, names in os.walk(root):
        for name in sorted(names):
            path = os.path.join(base, name)
            if os.path.islink(path):
                continue
            try:
                data = _read_thin_slice(path)
            except OSError:
                continue
            if data is None:
                continue
            parsed = macho.parse(data)
            if parsed.error:
                continue
            yield path, parsed
            seen += 1
            if seen >= limit:
                return


def analyze_tree(root: str, limit: int = 4000) -> dict:
    """Resolve as dependências de cada Mach-O contra a própria árvore."""
    binaries = []
    present: dict[str, int] = {}
    missing: dict[str, int] = {}
    for path, parsed in _walk_binaries(root, limit):
        rows = []
        for kind, dep in parsed.dylibs:
            if kind == "id":
                continue
            full = os.path.join(root, dep.lstrip("/"))
            if os.path.exists(full):
                present[dep] = present.get(dep, 0) + 1
                rows.append({"path": dep, "kind": kind, "present": True})
            else:
                missing[dep] = missing.get(dep, 0) + 1
                rows.append({"path": dep, "kind": kind, "present": False})
        binaries.append({"path": os.path.relpath(path, root),
                         "arch": parsed.arch,
                         "dylibs": rows,
                         "rpaths": parsed.rpaths})
    unique = set(present) | set(missing)
    cached = [dep for dep in missing if dep.startswith(SHARED_CACHE_DIRS)]
    absent = [dep for dep in missing if dep not in cached]
    return {
        "root": root,
        "binaries": binaries,
        "binaries_scanned": len(binaries),
        "dylibs_unique": len(unique),
        "dylibs_present": len(present),
        "dylibs_missing": len(missing),
        "missing_in_shared_cache": sorted(cached),
        "missing_elsewhere": sorted(absent),
        "present": dict(sorted(present.items())),
        "missing": dict(sorted(missing.items())),
    }


def summary(report: dict) -> str:
    lines = [
        f"{report['binaries_scanned']} binários, {report['dylibs_unique']} "
        f"bibliotecas distintas: {report['dylibs_present']} estão na árvore, "
        f"{report['dylibs_missing']} não",
        f"  das que faltam, {len(report['missing_in_shared_cache'])} são do "
        f"dyld shared cache (não estão em nenhuma imagem extraível)",
    ]
    if report["missing_elsewhere"]:
        lines.append("  e estas faltam por outro motivo:")
        for dep in report["missing_elsewhere"][:20]:
            lines.append(f"    {dep} (pedida por {report['missing'][dep]} binários)")
    return "\n".join(lines)
