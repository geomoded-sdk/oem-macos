"""Leitura de um volume APFS selado (iOS 16 em diante), sem 7-Zip.

O leitor do HakiOS (`vendor/hakios/apfs.py`) para no nó raiz de um volume
selado: o bloco não tem `obj_phys`, portanto não tem Fletcher-64, e a leitura
com verificação falha com "bad Fletcher-64 checksum". O formato dos nós é o
mesmo; muda o que se confere:

- `APFS_BTREE_NOHEADER` (0x100): o nó não tem cabeçalho de objeto nem checksum;
- `APFS_BTREE_HASHED` (0x80): cada entrada de filho traz 8 bytes de oid (um
  deslocamento a partir do oid da raiz) e 32 bytes do SHA-256 do nó filho.

O comportamento abaixo segue `apfs_lib.c` do APFS da Apple (APSL 2.0), em
`xnu/files/bsd/miscfs/apfs/` no clone do HakiOS: `btree_open`, `node_at_h`,
`node_child_addr`, `node_parse`, `node_entry`.

O resto — inodes, nomes, atributos, extents e a descompressão decmpfs — é o
leitor do HakiOS que já existe; aqui só se troca a árvore.
"""

from __future__ import annotations

import hashlib
import os
import shutil
import struct

from .vendor.hakios import apfs

BTREE_HASHED = 0x00000080
BTREE_NOHEADER = 0x00000100
HASH_SIZE = 32


class SealedBtree(apfs.Btree):
    """A mesma B-tree do HakiOS, mas lendo nós sem cabeçalho e com hash."""

    def __init__(self, blocks, root_oid, resolve=None, name="fstree"):
        self.blocks = blocks
        self.name = name
        self.resolve = resolve
        self.root_oid = root_oid
        self.physical = False
        self.root_addr = resolve(root_oid) if resolve else root_oid
        rb = blocks.get(self.root_addr, verify=False)
        info = len(rb) - apfs.BTREE_INFO_SIZE
        bt_flags, node_size, key_size, val_size = struct.unpack_from("<IIII", rb, info)
        self.bt_flags = bt_flags
        self.node_size = node_size
        self.key_size = key_size
        self.val_size = val_size
        (self.longest_key, self.longest_val, self.key_count, self.node_count) = \
            struct.unpack_from("<IIQQ", rb, info + 16)
        self.physical = bool(bt_flags & apfs.BTREE_PHYSICAL)
        self.hashed = bool(bt_flags & BTREE_HASHED)
        self.noheader = bool(bt_flags & BTREE_NOHEADER)
        self.root = apfs.Node(self.root_addr, rb, blocks.block_size, key_size, val_size)
        self.nodes_read = 0
        self.hash_checked = 0
        if not self.noheader:
            # um volume selado sem NOHEADER não é o que esta classe sabe ler
            raise apfs.ApfsError("a árvore não tem APFS_BTREE_NOHEADER; use o leitor normal")

    def node(self, oid, hash8=None):
        addr = oid if (self.physical or self.resolve is None) else self.resolve(oid)
        blk = self.blocks.get(addr, verify=not self.noheader)
        self.nodes_read += 1
        if self.noheader and hash8:
            if hashlib.sha256(blk).digest()[:8] != hash8[:8]:
                raise apfs.ApfsError("bloco %d: não é o nó que o pai nomeia (SHA-256)" % addr)
            self.hash_checked += 1
        return apfs.Node(addr, blk, self.blocks.block_size, self.key_size, self.val_size)

    def child(self, node, i):
        if not self.hashed:
            return super().child(node, i)
        _key, value = node.entry(i)
        if len(value) != 8 + HASH_SIZE:
            raise apfs.ApfsError(
                "entrada de filho com %d bytes (esperado 40: oid + SHA-256)" % len(value))
        oid = struct.unpack_from("<Q", value, 0)[0] + self.root_oid
        return self.node(oid, value[8:8 + HASH_SIZE])


def open_sealed(path: str, index: int = 0):
    """Abre o container e troca a árvore do volume pelo leitor selado."""
    container = apfs.Container(path)
    volumes = container.volumes()
    if not volumes:
        raise apfs.ApfsError("%s: nenhum volume" % path)
    volume = volumes[index]
    if not volume.sealed:
        return container, volume
    volume._tree = SealedBtree(container.blocks, volume.root_tree_oid,
                               volume.omap.resolver(), "fstree")
    return container, volume


def open_any(path: str, index: int = 0, workdir: str = "build/raw"):
    """Abre um APFS cru ou um UDIF que o contenha, devolvendo (container, volume)."""
    from . import apfsx
    if not apfsx.is_apfs(path):
        raw = os.path.join(workdir, os.path.basename(path) + ".raw")
        path = apfsx.to_raw(path, raw)
    return open_sealed(path, index)


def _is_noheader(blk: bytes) -> bool:
    return (struct.unpack_from("<Q", blk, 0)[0] == 0
            and struct.unpack_from("<Q", blk, 8)[0] == 0
            and struct.unpack_from("<I", blk, 24)[0] == 0
            and (struct.unpack_from("<H", blk, 32)[0] & apfs.BTNODE_NOHEADER) != 0)


def _tree_for(blocks, oid, resolve=None, name="tree"):
    root = blocks.get(oid, verify=False)
    if _is_noheader(root):
        return SealedBtree(blocks, oid, resolve, name)
    return apfs.Btree(blocks, oid, resolve=resolve, name=name)


FEXT_TREE_OID = 0x408          # apfs_fext_tree_oid no apfs_superblock
FEXT_TREE_TYPE = 0x410


class FextReader:
    """Os extents de um volume selado: a árvore separada, não os registros do fs tree.

    A chave é (id do fluxo, deslocamento lógico), 16 bytes, sem o campo de tipo do
    `j_key`; o valor é o mesmo `j_file_extent_val`.
    """

    def __init__(self, container, volume):
        blk = container.blocks.get(volume.addr)
        oid = struct.unpack_from("<Q", blk, FEXT_TREE_OID)[0]
        if not oid:
            raise apfs.ApfsError("volume sem árvore de extents (fext_tree_oid 0)")
        self.oid = oid
        self.tree = _tree_for(container.blocks, oid, None, "fext")
        self.blocks = container.blocks

    def extents(self, stream_id: int):
        want = (stream_id, 0)

        def less(key):
            return (struct.unpack_from("<Q", key, 0)[0],
                    struct.unpack_from("<Q", key, 8)[0]) < want

        out = []
        for key, value in self.tree.iter_from(less):
            sid = struct.unpack_from("<Q", key, 0)[0]
            if sid != stream_id:
                return out
            logical = struct.unpack_from("<Q", key, 8)[0]
            length = struct.unpack_from("<Q", value, 0)[0] & apfs.J_FILE_EXTENT_LEN_MASK
            phys = struct.unpack_from("<Q", value, 8)[0]
            out.append((logical, length, phys))
        return out

    def read(self, stream_id: int, size: int) -> bytes:
        out = bytearray(size)
        block_size = self.blocks.block_size
        for logical, length, phys in self.extents(stream_id):
            if logical >= size:
                continue
            take = min(length, size - logical)
            if phys == 0:
                continue
            blocks = (length + block_size - 1) // block_size
            out[logical:logical + take] = self.blocks.raw(phys, blocks)[:take]
        return bytes(out)


def _stream_xattr(inode, name, fext):
    """O valor de um atributo que vive no seu próprio fluxo (XATTR_DATA_STREAM)."""
    entry = inode.xattrs.get(name)
    if entry is None:
        return None
    flags, data = entry
    if flags & apfs.XATTR_DATA_STREAM:
        obj_id, size = struct.unpack_from("<QQ", data, 0)
        if fext is not None:
            return fext.read(obj_id, size)
    return inode.vol.xattr_bytes(inode.obj_id, name, inode.xattrs)


def _contents(inode, decoder=None, fext=None):
    """Os bytes do arquivo, incluindo os tipos decmpfs que o leitor não cobre."""
    header = inode.decmpfs
    if header is not None and len(header) >= 16:
        ctype = struct.unpack_from("<I", header, 4)[0]
        if ctype in (7, 8, 14):
            if decoder is None:
                raise apfs.ApfsError("inode %d: decmpfs tipo %d sem decodificador"
                                     % (inode.obj_id, ctype))
            fork = _stream_xattr(inode, apfs.RSRC_FORK_XATTR_NAME, fext)
            if fork is None:
                raise apfs.ApfsError("inode %d: tipo %d sem resource fork"
                                     % (inode.obj_id, ctype))
            return decoder(header, fork)
    if inode.is_link:
        return inode.symlink_target_bytes() or b""
    if inode._dstream is not None and fext is not None:
        return fext.read(inode.private_id, inode._dstream[0])
    return inode.contents()


def _clear(path: str) -> None:
    """Remove o que já existir no destino, para a extração ser repetível."""
    if os.path.islink(path) or os.path.isfile(path):
        os.unlink(path)
    elif os.path.isdir(path):
        shutil.rmtree(path)


def extract(volume, outdir: str, decoder=None, max_files: int | None = None) -> dict:
    """Extrai a árvore do volume selado para OUTDIR. Devolve a contagem real."""
    stats = {"files": 0, "directories": 0, "symlinks": 0, "bytes": 0,
             "errors": [], "nodes": 0, "decmpfs_streams": 0}
    try:
        fext = FextReader(volume.c, volume)
        stats["fext_tree_oid"] = fext.oid
    except apfs.ApfsError:
        fext = None
    os.makedirs(outdir, exist_ok=True)
    stack = [(apfs.ROOT_DIR_INO_NUM, outdir)]
    seen = set()
    while stack:
        oid, path = stack.pop()
        if oid in seen:
            continue
        seen.add(oid)
        for name, fid, dtype in sorted(volume.dir_entries(oid)):
            target = os.path.join(path, name)
            if dtype == 4:
                if os.path.islink(target) or (os.path.exists(target)
                                              and not os.path.isdir(target)):
                    _clear(target)
                os.makedirs(target, exist_ok=True)
                stats["directories"] += 1
                stack.append((fid, target))
                continue
            inode = volume.inode(fid)
            if inode is None:
                stats["errors"].append({"path": target, "reason": "inode %d ausente" % fid})
                continue
            if inode.is_link:
                link = inode.symlink_target_bytes()
                if link is None:
                    stats["errors"].append({"path": target, "reason": "symlink sem alvo"})
                    continue
                _clear(target)
                os.symlink(link.decode("utf-8", "surrogateescape"), target)
                stats["symlinks"] += 1
                continue
            header = inode.decmpfs
            if header is not None and len(header) >= 16:
                if struct.unpack_from("<I", header, 4)[0] in (7, 8, 14):
                    stats["decmpfs_streams"] += 1
            try:
                data = _contents(inode, decoder, fext)
            except apfs.ApfsError as exc:
                stats["errors"].append({"path": target, "reason": str(exc)})
                continue
            os.makedirs(os.path.dirname(target), exist_ok=True)
            if os.path.islink(target) or os.path.isdir(target):
                _clear(target)
            with open(target, "wb") as handle:
                handle.write(data)
            try:
                os.chmod(target, inode.mode & 0o7777)
            except OSError:
                pass
            stats["files"] += 1
            stats["bytes"] += len(data)
            if max_files is not None and stats["files"] >= max_files:
                return stats
    stats["nodes"] = getattr(volume.tree, "nodes_read", 0)
    stats["hash_checked"] = getattr(volume.tree, "hash_checked", 0)
    if fext is not None:
        stats["fext_nodes"] = fext.tree.nodes_read
        stats["fext_hash_checked"] = getattr(fext.tree, "hash_checked", 0)
    return stats
