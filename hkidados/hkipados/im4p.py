"""IM4P: o envelope dos componentes de firmware da Apple.

O arquivo é ASN.1/DER com uma sequência externa:

    30 84 <len>                  SEQUENCE
      16 04 "IM4P"               IA5String: o tipo do contêiner
      16 04 "krnl"               IA5String: o tipo do conteúdo
      16 1e "KernelManagement…"  IA5String: a descrição
      04 84 <len> <payload>      OCTET STRING: o conteúdo (às vezes comprimido)
      16 04 "IM4M" ...           o manifesto de assinatura, quando presente

O payload do kernelcache do iPadOS 27 começa com `bvx2` (LZFSE). O do DeviceTree
também. O IM4P em si não é cifrado; quem cifra é o volume do sistema (AEA).
"""

from __future__ import annotations

import struct
from dataclasses import dataclass

TAG_IA5 = 0x16
TAG_OCTET = 0x04

COMPRESSION_MAGIC = {
    b"bvx2": "lzfse",
    b"bvx-": "lzfse",
    b"bvx$": "lzfse",
    b"complzss": "lzss",
    b"\x1f\x8b": "gzip",
}


class Im4pError(Exception):
    pass


@dataclass
class Im4p:
    prefix: int                 # bytes antes de "IM4P" (0 no formato clássico)
    type: str
    description: str
    payload: bytes
    payload_offset: int         # posição do payload dentro do arquivo
    trailer: bytes              # o que vem depois do payload (IM4M, lixo de alinhamento)
    total: int

    @property
    def compression(self) -> str | None:
        for magic, name in COMPRESSION_MAGIC.items():
            if self.payload.startswith(magic):
                return name
        return None

    @property
    def payload_magic(self) -> str:
        head = self.payload[:4]
        return head.decode("latin-1") if all(32 <= b < 127 for b in head) else head.hex()

    def decompress(self) -> bytes:
        """O conteúdo descomprimido. LZFSE precisa do módulo `lzfse`."""
        kind = self.compression
        if kind is None:
            return self.payload
        if kind == "lzfse":
            try:
                import lzfse
            except ImportError as error:
                raise Im4pError(
                    "payload em LZFSE: instale o módulo Python 'lzfse' "
                    "(python3 -m pip install lzfse) ou use --no-decompress") from error
            return lzfse.decompress(self.payload)
        if kind == "lzss":
            raise Im4pError("payload em LZSS ('complzss'): descompressão não implementada")
        if kind == "gzip":
            import gzip
            return gzip.decompress(self.payload)
        raise Im4pError(f"compressão desconhecida: {kind}")

    def sha(self, algorithm: str = "sha1") -> str:
        import hashlib
        return hashlib.new(algorithm, self.payload).hexdigest()


def _der_length(data: bytes, at: int) -> tuple[int, int]:
    if at >= len(data):
        raise Im4pError("fim do arquivo lendo um comprimento DER")
    first = data[at]
    at += 1
    if not first & 0x80:
        return first, at
    count = first & 0x7F
    if count == 0 or at + count > len(data):
        raise Im4pError("comprimento DER indefinido ou truncado")
    return int.from_bytes(data[at:at + count], "big"), at + count


def _field(data: bytes, at: int, tag: int) -> tuple[bytes, int]:
    if at >= len(data):
        raise Im4pError("fim do arquivo lendo um campo")
    if data[at] != tag:
        raise Im4pError(f"esperava a tag 0x{tag:02x}, achei 0x{data[at]:02x} no byte {at}")
    length, at = _der_length(data, at + 1)
    end = at + length
    if end > len(data):
        raise Im4pError("campo truncado")
    return data[at:end], end


def parse(data: bytes) -> Im4p:
    """Lê um IM4P, inclusive o formato embrulhado numa SEQUENCE (iPadOS 27)."""
    prefix = data.find(b"IM4P")
    if prefix < 0:
        raise Im4pError("não achei a assinatura IM4P")
    at = prefix + 4
    try:
        raw_type, at = _field(data, at, TAG_IA5)
        raw_description, at = _field(data, at, TAG_IA5)
        payload, at = _field(data, at, TAG_OCTET)
    except Im4pError:
        # formato antigo: tipo cru de 4 bytes, sem a tag IA5
        raw_type = data[at:at + 4]
        at += 4
        raw_description, at = _field(data, at, TAG_IA5)
        payload, at = _field(data, at, TAG_OCTET)
    return Im4p(
        prefix=prefix,
        type=raw_type.decode("latin-1", "replace"),
        description=raw_description.decode("latin-1", "replace"),
        payload=payload,
        payload_offset=at - len(payload),
        trailer=data[at:],
        total=len(data),
    )


def locate(data: bytes) -> tuple[str, str, int, int]:
    """(tipo, descrição, deslocamento, comprimento) do payload, sem copiá-lo.

    Serve para um IM4P grande (o ramdisk de restauração tem 234 MB): só os
    primeiros bytes são necessários para saber onde o payload começa e acaba.
    """
    prefix = data.find(b"IM4P")
    if prefix < 0:
        raise Im4pError("não achei a assinatura IM4P")
    at = prefix + 4

    def field(at: int, tag: int) -> tuple[int, int]:
        if at >= len(data):
            raise Im4pError("fim do arquivo lendo um campo")
        if data[at] != tag:
            raise Im4pError(f"esperava a tag 0x{tag:02x}, achei 0x{data[at]:02x} no byte {at}")
        return _der_length(data, at + 1)

    length, start = field(at, TAG_IA5)
    raw_type = data[start:start + length].decode("latin-1", "replace")
    at = start + length
    length, start = field(at, TAG_IA5)
    description = data[start:start + length].decode("latin-1", "replace")
    at = start + length
    length, start = field(at, TAG_OCTET)
    return raw_type, description, start, length


def is_im4p(data: bytes) -> bool:
    return b"IM4P" in data[:64]


def macho_summary(data: bytes) -> dict | None:
    """Resumo do Mach-O contido num payload descomprimido, se for um."""
    if len(data) < 32:
        return None
    magic = struct.unpack_from("<I", data, 0)[0]
    if magic not in (0xFEEDFACF, 0xFEEDFACE, 0xCAFEBABE):
        return None
    from .macho import parse
    try:
        return parse(data).as_dict()
    except Exception as error:  # noqa: BLE001 - o relatório não pode morrer por um binário estranho
        return {"error": str(error)}
