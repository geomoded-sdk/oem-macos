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
nem no leitor do HakiOS. Este módulo lê o cabeçalho e diz o que há, sem fingir
que descomprimiu.
"""

from __future__ import annotations

import struct
from dataclasses import dataclass

MAGIC = 0x636D7066          # "cmpf" em little-endian
HEADER_SIZE = 16
ZBM_MAGIC = b"ZBM"

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
