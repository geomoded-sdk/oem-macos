"""Apple Encrypted Archive (AEA): o que dá para saber sem a chave.

O volume do sistema do iPadOS 27 (`141-10293-225.dmg.aea`, 8,3 GB) e o cryptex do
sistema (`141-09054-235.dmg.aea`, 2,4 GB) são AEA. O cabeçalho é legível:

    41 45 41 31          "AEA1"
    01 00 00 00          versão 1
    c0 08 00 00          tamanho do cabeçalho (0x8c0 = 2240)
    30 00 00 00          tamanho do primeiro campo
    "com.apple.wkms.url\0https://wkms.sd.apple.com"
    81 03 00 00          tamanho do segundo campo
    "com.apple.wkms.auth-data\0<base64>"

`wkms` é o serviço de gestão de chaves da Apple. A chave que abre o arquivo é
entregue por esse serviço, autenticada pelo dispositivo. Este projeto não tenta
obtê-la: isso seria contornar a proteção, e está fora do escopo declarado.
"""

from __future__ import annotations

import struct
from dataclasses import dataclass, field

MAGIC = b"AEA1"


class AeaError(Exception):
    pass


@dataclass
class AeaHeader:
    version: int
    header_size: int
    fields: dict[str, str] = field(default_factory=dict)
    undecodable: int = 0

    @property
    def key_service(self) -> str | None:
        return self.fields.get("com.apple.wkms.url")

    @property
    def has_auth_data(self) -> bool:
        return "com.apple.wkms.auth-data" in self.fields

    def as_dict(self) -> dict:
        return {
            "format": "AEA",
            "version": self.version,
            "header_size": self.header_size,
            "fields": {k: (v[:64] + "…" if len(v) > 64 else v) for k, v in self.fields.items()},
            "key_service": self.key_service,
            "has_auth_data": self.has_auth_data,
            "decryptable_here": False,
            "reason": "chave entregue pelo wkms da Apple, autenticada pelo dispositivo",
        }


def probe(source, limit: int = 4096) -> AeaHeader:
    """Lê e decodifica o cabeçalho de um AEA sem tocar no conteúdo cifrado."""
    data = source.read(0, min(limit, source.size))
    if not data.startswith(MAGIC):
        raise AeaError("não é um AEA (falta a assinatura AEA1)")
    if len(data) < 12:
        raise AeaError("cabeçalho AEA truncado")
    version, header_size = struct.unpack_from("<II", data, 4)
    header = AeaHeader(version=version, header_size=header_size)
    # O cabeçalho é uma sequência de campos: um comprimento de 32 bits, e dentro dele
    # uma chave terminada em NUL seguida do valor. Ler por NUL é mais robusto do que
    # confiar no comprimento, que inclui alinhamento.
    region = data[12:min(len(data), 12 + header_size)]
    parts = region.split(b"\0")
    for index, part in enumerate(parts):
        if not part.startswith(b"com.apple."):
            continue
        try:
            key = part.decode("ascii")
        except UnicodeDecodeError:
            header.undecodable += 1
            continue
        value = ""
        if index + 1 < len(parts):
            raw = parts[index + 1]
            value = "".join(ch for ch in raw.decode("latin-1") if 0x20 <= ord(ch) < 0x7F)
        header.fields[key] = value
    return header