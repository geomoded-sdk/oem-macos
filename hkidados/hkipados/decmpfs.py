"""decmpfs: como o APFS guarda um arquivo comprimido, e por que ele vem vazio.

No cryptex de App do iPadOS 27 todos os arquivos são comprimidos com decmpfs. O
atributo `com.apple.decmpfs` tem 16 bytes:

    66 70 6d 63   "cmpf" (o magic, gravado invertido)
    0e 00 00 00   tipo de compressão (14 neste IPSW)
    a0 92 02 00 00 00 00 00   tamanho descomprimido (168864)

e o conteúdo comprimido vai para `com.apple.ResourceFork`. O 7-Zip 25.01 extrai
o atributo como um arquivo `nome:com.apple.decmpfs` e deixa o arquivo principal
com 0 byte; ele não remonta o arquivo. O tipo 14 começa com `ZBM\\x09`, o
contêiner do LZBITMAP — e o prefixo literal já mostra um Mach-O universal
(`ca fe ba be`, duas fatias, `arm64e`).

Reconstruir exige o decodificador LZBITMAP (bitmap + LZVN), que não está no 7-Zip
nem no leitor do HakiOS — mas está no HakiOS: `vendor/hakios/lzbitmap` traz o
`libzbitmap.c` e o `unzbm.c`, e é isso que este módulo compila e chama.
"""

from __future__ import annotations

import os
import shutil
import struct
import subprocess
from dataclasses import dataclass

MAGIC = 0x636D7066          # "cmpf" em little-endian
HEADER_SIZE = 16
ZBM_MAGIC = b"ZBM"

TYPE_LZBITMAP = 14

TYPES = {
    1: "zlib",
    3: "LZVN",
    4: "LZFSE",
    7: "LZBITMAP",
    8: "LZBITMAP16",
    14: "LZBITMAP",
}


@dataclass
class Header:
    compression_type: int
    uncompressed_size: int

    @property
    def compression(self) -> str:
        return TYPES.get(self.compression_type, f"tipo {self.compression_type}")


def parse(data: bytes) -> Header:
    if len(data) < HEADER_SIZE:
        raise ValueError("cabeçalho decmpfs truncado")
    magic, compression_type, uncompressed_size = struct.unpack_from("<IIQ", data, 0)
    if magic != MAGIC:
        raise ValueError(f"magic decmpfs inválido: 0x{magic:08x}")
    return Header(compression_type, uncompressed_size)


def resource_container(data: bytes) -> str | None:
    """O que o resource fork anuncia: `ZBM` (LZBITMAP), `bvx2` (LZFSE), etc.

    O fluxo começa depois de um cabeçalho de recurso (16 bytes neste IPSW), então
    procurar no prefixo é mais seguro do que exigir que esteja no byte 0.
    """
    head = data[:64]
    if ZBM_MAGIC in head:
        return "ZBM"
    if b"bvx2" in head or b"bvx-" in head or b"bvx$" in head:
        return "LZFSE"
    if b"complzss" in head:
        return "LZSS"
    return None


def fat_header(data: bytes, offset: int = 0) -> dict | None:
    """Lê um cabeçalho de Mach-O universal, se houver um no deslocamento dado."""
    if offset + 8 > len(data):
        return None
    magic = struct.unpack_from(">I", data, offset)[0]
    if magic not in (0xCAFEBABE, 0xCAFEBABF):
        return None
    count = struct.unpack_from(">I", data, offset + 4)[0]
    if count > 16:
        return None
    return {"magic": f"0x{magic:08x}", "slices": count}


def find_fat(data: bytes, limit: int = 4096) -> dict | None:
    """Procura um cabeçalho universal no prefixo literal de um fluxo comprimido."""
    for offset in range(0, max(0, min(len(data), limit) - 7)):
        found = fat_header(data, offset)
        if found:
            found["offset"] = offset
            return found
    return None


# --------------------------------------------------------------------- LZBITMAP
#
# O decodificador não é nosso: o HakiOS traz libzbitmap.c (Ernesto A. Fernández,
# Corellium, MIT/GPL-2.0+) e unzbm.c, que junta os 64K blocos do resource fork.
# Copiamos os dois verbatim em vendor/hakios/lzbitmap e só os compilamos.

UNZBM_SOURCES = ("libzbitmap.c", "unzbm.c")
BLOCK = 0x10000


def vendor_dir() -> str:
    return os.path.join(os.path.dirname(os.path.abspath(__file__)), "vendor", "hakios", "lzbitmap")


def build_unzbm(dest: str = "build/bin/unzbm", cc: str | None = None) -> tuple[str | None, str | None]:
    """Compila o unzbm do HakiOS. Devolve (caminho, None) ou (None, motivo)."""
    cc = cc or shutil.which("cc") or shutil.which("gcc") or shutil.which("clang")
    if not cc:
        return None, "nenhum compilador C (cc/gcc/clang) no PATH"
    sources = [os.path.join(vendor_dir(), name) for name in UNZBM_SOURCES]
    missing = [path for path in sources if not os.path.exists(path)]
    if missing:
        return None, "fontes ausentes: " + ", ".join(missing)
    if os.path.exists(dest) and all(os.path.getmtime(dest) >= os.path.getmtime(path) for path in sources):
        return dest, None
    os.makedirs(os.path.dirname(dest) or ".", exist_ok=True)
    proc = subprocess.run([cc, "-O2", "-Wall", "-o", dest, *sources],
                          capture_output=True, text=True)
    if proc.returncode != 0:
        tail = (proc.stderr.strip().splitlines() or ["cc falhou"])[-1]
        return None, f"cc falhou: {tail}"
    return dest, None


def decode_file(name: str, unzbm: str) -> tuple[int | None, str | None]:
    """Descomprime NAME a partir de NAME:com.apple.decmpfs/ResourceFork.

    Devolve (bytes, None) ou (None, motivo). O tamanho é conferido contra o
    cabeçalho decmpfs e cada bloco é conferido pelo unzbm.
    """
    header_path = name + ":com.apple.decmpfs"
    if not os.path.exists(header_path):
        return None, "sem atributo decmpfs"
    try:
        with open(header_path, "rb") as handle:
            header = parse(handle.read(16))
    except (OSError, ValueError) as exc:
        return None, f"cabeçalho inválido: {exc}"
    if header.compression_type != TYPE_LZBITMAP:
        return None, f"tipo {header.compression_type} ({header.compression}) não é LZBITMAP"
    proc = subprocess.run([unzbm, name], capture_output=True, text=True)
    if proc.returncode != 0:
        tail = (proc.stderr.strip().splitlines() or ["unzbm falhou"])[-1]
        return None, tail.replace("unzbm: ", "")
    try:
        size = os.path.getsize(name)
    except OSError as exc:
        return None, str(exc)
    if size != header.uncompressed_size:
        return None, f"tamanho {size} != {header.uncompressed_size} do cabeçalho"
    return size, None


def decode_tree(root: str, unzbm: str, limit: int | None = None) -> dict:
    """Descomprime toda a árvore extraída pelo 7-Zip e confere o resultado."""
    names = []
    for base, _dirs, files in os.walk(root):
        for name in files:
            if name.endswith(":com.apple.decmpfs"):
                names.append(os.path.join(base, name[:-len(":com.apple.decmpfs")]))
    names.sort()
    if limit is not None:
        names = names[:limit]
    decoded = 0
    failed = 0
    total = 0
    failures = []
    for name in names:
        size, error = decode_file(name, unzbm)
        if error:
            failed += 1
            if len(failures) < 20:
                failures.append({"file": os.path.relpath(name, root), "reason": error})
        else:
            decoded += 1
            total += size
    return {"streams": len(names), "decoded": decoded, "failed": failed,
            "bytes": total, "failures": failures}


def build_libzbitmap(dest: str = "build/lib/libzbitmap.so", cc: str | None = None) -> tuple[str | None, str | None]:
    """Compila o libzbitmap.c do HakiOS como biblioteca compartilhada (para ctypes)."""
    cc = cc or shutil.which("cc") or shutil.which("gcc") or shutil.which("clang")
    if not cc:
        return None, "nenhum compilador C (cc/gcc/clang) no PATH"
    source = os.path.join(vendor_dir(), "libzbitmap.c")
    if not os.path.exists(source):
        return None, "fonte ausente: " + source
    if os.path.exists(dest) and os.path.getmtime(dest) >= os.path.getmtime(source):
        return dest, None
    os.makedirs(os.path.dirname(dest) or ".", exist_ok=True)
    proc = subprocess.run([cc, "-O2", "-shared", "-fPIC", "-o", dest, source],
                          capture_output=True, text=True)
    if proc.returncode != 0:
        tail = (proc.stderr.strip().splitlines() or ["cc falhou"])[-1]
        return None, f"cc falhou: {tail}"
    return dest, None


_lib_cache: dict[str, object] = {}


def _zbm_library(path: str):
    import ctypes
    lib = _lib_cache.get(path)
    if lib is None:
        lib = ctypes.CDLL(path)
        lib.zbm_decompress.restype = ctypes.c_int
        lib.zbm_decompress.argtypes = [ctypes.c_void_p, ctypes.c_size_t,
                                       ctypes.c_void_p, ctypes.c_size_t,
                                       ctypes.POINTER(ctypes.c_size_t)]
        _lib_cache[path] = lib
    return lib


def _zbm_block(lib, chunk: bytes, want: int) -> bytes:
    import ctypes
    out = ctypes.create_string_buffer(want)
    got = ctypes.c_size_t(0)
    src = ctypes.create_string_buffer(chunk, len(chunk))
    rc = lib.zbm_decompress(out, want, src, len(chunk), ctypes.byref(got))
    if rc != 0:
        raise ValueError("zbm_decompress devolveu %d" % rc)
    if got.value != want:
        raise ValueError("bloco com %d bytes, esperado %d" % (got.value, want))
    return out.raw[:want]


def decompress(header_bytes: bytes, fork: bytes, lib_path: str = "build/lib/libzbitmap.so") -> bytes:
    """Os bytes de um arquivo decmpfs tipo 14 a partir do cabeçalho e do resource fork.

    O laço é o do `unzbm.c` do HakiOS: o resource fork começa com uma tabela de
    deslocamentos, um por bloco de 64 KB; um bloco que não encolheu vem como 0xff
    seguido dos bytes.
    """
    header = parse(header_bytes)
    if header.compression_type != TYPE_LZBITMAP:
        raise ValueError("tipo %d (%s) não é LZBITMAP" % (header.compression_type, header.compression))
    size = header.uncompressed_size
    blocks = (size + BLOCK - 1) // BLOCK
    if (blocks + 1) * 4 > len(fork):
        raise ValueError("tabela de blocos não cabe no resource fork")
    lib_path, error = build_libzbitmap(lib_path)
    if lib_path is None:
        raise ValueError(error)
    lib = _zbm_library(lib_path)
    out = bytearray()
    for index in range(blocks):
        start = int.from_bytes(fork[4 * index:4 * index + 4], "little")
        end = int.from_bytes(fork[4 * index + 4:4 * index + 8], "little")
        want = min(BLOCK, size - index * BLOCK)
        if end > len(fork) or start > end:
            raise ValueError("bloco %d fora do resource fork" % index)
        chunk = fork[start:end]
        if len(chunk) == want + 1 and chunk[:1] == b"\xff":
            out += chunk[1:]
        else:
            out += _zbm_block(lib, chunk, want)
    return bytes(out)
