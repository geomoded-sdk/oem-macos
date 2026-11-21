"""APFS: até onde o leitor comunitário do HakiOS vai, e onde ele para.

O `apfs.py` do HakiOS lê contêineres APFS, volumes, árvores comuns e arquivos
comprimidos. Ele foi escrito contra as imagens que o próprio `mkapfs.py` gera e
contra o volume do iOS 16.5. No iPadOS 27 o volume do cryptex de App é **selado**
(`SEALED_VOLUME`), e a árvore do sistema de arquivos passa a usar nós sem
cabeçalho (`BTNODE_NOHEADER`), ordenados por hash e com ponteiros para filhos que
carregam 32 bytes de hash além do endereço físico.

Este módulo não reescreve o leitor: ele usa o do HakiOS como está, detecta o caso
selado e diz exatamente o que falhou. Para extrair arquivos, usa o 7-Zip, que é o
mesmo caminho que o `sysimage/unpack.sh` do HakiOS adota.
"""

from __future__ import annotations

import os
import re
import shutil
import struct
import subprocess
import sys

BTNODE_ROOT = 0x0001
BTNODE_LEAF = 0x0002
BTNODE_HASHED = 0x0008
BTNODE_NOHEADER = 0x0010

NX_MAGIC = b"NXSB"
APFS_MAGIC = b"APSB"
UDIF_TRAILER = b"koly"


def _upstream():
    from .vendor.hakios import apfs  # importado tarde: só quem precisa paga o custo
    return apfs


def image_kind(path: str) -> dict:
    """Diz o que é o arquivo: UDIF, APFS cru, AEA, ou outra coisa."""
    size = os.path.getsize(path)
    with open(path, "rb") as handle:
        head = handle.read(4096)
        tail = b""
        if size > 512:
            handle.seek(-512, os.SEEK_END)
            tail = handle.read(512)
    kind = "unknown"
    details = {}
    if head.startswith(b"AEA1"):
        kind = "aea"
    elif tail[:4] == UDIF_TRAILER:
        kind = "udif"
    elif (at := head.find(NX_MAGIC)) >= 4:
        kind = "apfs"
        details["apfs_offset"] = at - 32
        details["block_size"] = struct.unpack_from("<I", head, at + 4)[0]
        details["block_count"] = struct.unpack_from("<Q", head, at + 8)[0]
    elif head.startswith((b"\x1f\x8b", b"BZh", b"\xfd7zXZ")):
        kind = "compressed"
    return {"kind": kind, "size": size, **details}


def image_kind_bytes(data: bytes) -> dict:
    """O mesmo que `image_kind`, para uma imagem já em memória."""
    details: dict = {"kind": "unknown", "size": len(data)}
    if data.startswith(b"AEA1"):
        details["kind"] = "aea"
    elif len(data) >= 512 and data[-512:-508] == UDIF_TRAILER:
        details["kind"] = "udif"
    elif (at := data.find(NX_MAGIC, 0, 65536)) >= 4:
        details["kind"] = "apfs"
        details["apfs_offset"] = at - 32
        details["block_size"] = struct.unpack_from("<I", data, at + 4)[0]
        details["block_count"] = struct.unpack_from("<Q", data, at + 8)[0]
    return details


def node_flags(block: bytes) -> dict:
    """Os campos de um nó de B-tree, com ou sem cabeçalho de objeto."""
    if len(block) < 40:
        return {}
    flags, level, nkeys = struct.unpack_from("<HHI", block, 32)
    return {"flags": flags, "level": level, "nkeys": nkeys,
            "root": bool(flags & BTNODE_ROOT), "leaf": bool(flags & BTNODE_LEAF),
            "hashed": bool(flags & BTNODE_HASHED),
            "noheader": bool(flags & BTNODE_NOHEADER),
            "zero_header": block[:24] == bytes(24)}


def probe_container(path: str) -> dict:
    """Lê o contêiner com o leitor do HakiOS e relata onde ele consegue chegar."""
    apfs = _upstream()
    result = {"path": path, "kind": image_kind(path), "volumes": [],
              "container_readable": False, "fstree_readable": False}
    try:
        container = apfs.Container(path)
    except Exception as error:  # noqa: BLE001 - queremos a mensagem, não o tipo
        result["error"] = f"{type(error).__name__}: {error}"
        return result
    result["container_readable"] = True
    result["block_size"] = container.block_size
    result["xid"] = container.xid
    try:
        volumes = container.volumes()
    except Exception as error:  # noqa: BLE001
        result["error"] = f"volumes: {type(error).__name__}: {error}"
        volumes = []
    for volume in volumes:
        entry = {
            "name": volume.volname,
            "role": volume.role,
            "sealed": volume.sealed,
            "incompat": f"0x{volume.incompat:x}",
            "num_files": volume.num_files,
            "num_directories": volume.num_directories,
            "num_symlinks": volume.num_symlinks,
            "root_tree_oid": volume.root_tree_oid,
            "omap_oid": volume.omap_oid,
        }
        try:
            tree = volume.tree
            root = tree.root
            entry["fstree"] = {
                "physical": tree.physical,
                "level": root.level,
                "nkeys": root.nkeys,
                "noheader": node_flags(root.blk).get("noheader", False),
            }
            entry["root_entries"] = len(volume.dir_entries(apfs.ROOT_DIR_INO_NUM))
            result["fstree_readable"] = True
        except Exception as error:  # noqa: BLE001
            entry["fstree"] = {"error": f"{type(error).__name__}: {error}"}
        result["volumes"].append(entry)
    container.close()
    if result["volumes"] and not result["fstree_readable"]:
        sealed = any(v["sealed"] for v in result["volumes"])
        result["explanation"] = (
            "volume selado: a árvore usa nós BTNODE_NOHEADER (cabeçalho zerado, sem "
            "checksum Fletcher-64) e ponteiros de filho de 40 bytes (endereço físico + "
            "hash de 32 bytes). O leitor do HakiOS verifica checksum em todo bloco e "
            "ordena por object-id; nenhuma das duas coisas vale nessa árvore."
            if sealed else
            "a árvore do sistema de arquivos não pôde ser aberta; veja o erro do volume"
        )
    return result


def is_apfs(path: str) -> bool:
    """A imagem já é um container APFS cru (NXSB em 32)? Um .dmg é UDIF, não é."""
    with open(path, "rb") as handle:
        head = handle.read(4096)
    return len(head) >= 36 and head[32:36] == b"NXSB"


def to_raw(path: str, out: str | None = None) -> str:
    """A imagem APFS crua, tirando o embrulho IM4P ou UDIF que houver."""
    if is_apfs(path):
        return path
    if out is not None:
        os.makedirs(os.path.dirname(os.path.abspath(out)), exist_ok=True)
    from . import im4p
    with open(path, "rb") as handle:
        head = handle.read(4096)
    if b"IM4P" in head[:64]:
        kind, _desc, offset, length = im4p.locate(head)
        if out is None:
            out = path + ".raw"
        if not (os.path.exists(out) and os.path.getsize(out) == length):
            with open(path, "rb") as src, open(out, "wb") as dst:
                src.seek(offset)
                left = length
                while left:
                    chunk = src.read(min(1 << 24, left))
                    if not chunk:
                        raise ValueError("%s: IM4P %s truncado" % (path, kind))
                    dst.write(chunk)
                    left -= len(chunk)
        with open(out, "rb") as check:
            if check.read(4096)[32:36] != b"NXSB":
                raise ValueError("%s: o payload IM4P %s não é APFS" % (path, kind))
        return out
    from .vendor.hakios import dmg
    if out is None:
        out = path + ".raw"
    if os.path.exists(out) and os.path.getsize(out) > 0:
        return out
    image = dmg.Dmg(path)
    os.makedirs(os.path.dirname(os.path.abspath(out)), exist_ok=True)
    image.extract(out)
    return out


def seven_zip() -> str | None:
    for name in ("7z", "7zz", "7za"):
        path = shutil.which(name)
        if path:
            return path
    return None


def seven_zip_info(path: str) -> dict:
    """O que o 7-Zip 23+ entende da imagem (ele lê APFS, inclusive selado)."""
    tool = seven_zip()
    if not tool:
        return {"available": False}
    completed = subprocess.run([tool, "l", path], capture_output=True, text=True,
                               errors="replace", timeout=300)
    text = completed.stdout
    info: dict = {"available": True, "tool": tool, "returncode": completed.returncode}
    match = re.search(r"Type = (\S+)", text)
    if match:
        info["type"] = match.group(1)
    match = re.search(r"Physical Size = (\d+)", text)
    if match:
        info["physical_size"] = int(match.group(1))
    for key in ("volume_name", "num_files", "num_directories", "num_symlinks",
                "incompatible_features", "block_size", "fs_alloc_count"):
        match = re.search(rf"^\s*{key}: (.+)$", text, re.MULTILINE)
        if match:
            info[key] = match.group(1).strip()
    info["entries"] = len(re.findall(r"^[-d].{10} ", text, re.MULTILINE))
    return info


def extract_with_7z(path: str, outdir: str, timeout: int = 1800) -> dict:
    """Extrai uma imagem APFS/UDIF para um diretório. Devolve a contagem real."""
    tool = seven_zip()
    if not tool:
        raise RuntimeError("7-Zip não está instalado (apt install 7zip)")
    os.makedirs(outdir, exist_ok=True)
    completed = subprocess.run([tool, "x", "-y", f"-o{outdir}", path],
                               capture_output=True, text=True, errors="replace",
                               timeout=timeout)
    files = 0
    total = 0
    for root, _dirs, names in os.walk(outdir):
        for name in names:
            files += 1
            try:
                total += os.path.getsize(os.path.join(root, name))
            except OSError:
                pass
    return {"files": files, "bytes": total, "returncode": completed.returncode,
            "stderr_tail": completed.stderr.strip().splitlines()[-3:]}


def find_launchd(root: str) -> list[str]:
    """Onde estão os launchd de uma árvore extraída (prova de que a userland veio)."""
    hits = []
    for base, _dirs, names in os.walk(root):
        for name in names:
            if name in ("launchd", "launchd_sim") or name.endswith("backboardd"):
                hits.append(os.path.join(base, name))
    return sorted(hits)
