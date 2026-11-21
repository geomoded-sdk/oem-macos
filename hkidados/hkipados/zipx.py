"""ZIP64 sobre uma fonte com acesso aleatório.

Um IPSW é um ZIP64 com 185 membros e 11,6 GB. O `zipfile` da biblioteca padrão
funciona muito bem num arquivo local, mas não numa fonte HTTP; e aqui a mesma
implementação precisa servir aos dois casos. Ela lê o diretório central (17 KB),
que é a única parte que descreve todos os membros, e só então busca os bytes de
cada membro, inflando com `zlib` e conferindo o CRC32 do diretório central.
"""

from __future__ import annotations

import bz2
import struct
import zlib
from dataclasses import dataclass

from .source import SourceError

EOCD_SIG = b"PK\x05\x06"
ZIP64_LOCATOR_SIG = b"PK\x06\x07"
ZIP64_EOCD_SIG = b"PK\x06\x06"
CENTRAL_SIG = b"PK\x01\x02"
LOCAL_SIG = b"PK\x03\x04"

STORED = 0
DEFLATED = 8
BZIP2 = 12


class ZipError(Exception):
    pass


@dataclass
class Member:
    name: str
    usize: int
    csize: int
    method: int
    crc: int
    local_offset: int
    mtime: tuple[int, int, int, int, int, int] | None = None

    @property
    def ratio(self) -> float:
        return (self.csize / self.usize) if self.usize else 1.0


class ZipArchive:
    def __init__(self, source):
        self.source = source
        self.size = source.size
        self._read_end()
        self._read_central()

    # ------------------------------------------------------------------ estrutura

    def _read_end(self) -> None:
        tail_len = min(self.size, 65557 + 20)
        tail = self.source.read(self.size - tail_len, tail_len)
        at = tail.rfind(EOCD_SIG)
        if at < 0:
            raise ZipError("não achei o EOCD; isto não é um ZIP (ou falta o fim do arquivo)")
        (_, _, _, _, n_total, cd_size, cd_off, _) = struct.unpack_from("<4sHHHHIIH", tail, at)
        self.eocd_offset = self.size - tail_len + at
        self.zip64 = False
        if n_total == 0xFFFF or cd_size == 0xFFFFFFFF or cd_off == 0xFFFFFFFF:
            locator_at = at - 20
            if locator_at < 0 or tail[locator_at:locator_at + 4] != ZIP64_LOCATOR_SIG:
                raise ZipError("ZIP64 anunciado mas sem localizador")
            _, _, z64_off, _ = struct.unpack_from("<IIQI", tail, locator_at)
            record = self.source.read(z64_off, 56)
            if record[:4] != ZIP64_EOCD_SIG:
                raise ZipError("registro ZIP64 inválido")
            (_, _, _, _, _, _, _, n_total, cd_size, cd_off) = struct.unpack_from(
                "<4sQHHIIQQQQ", record, 0)
            self.zip64 = True
        self.n_total = n_total
        self.cd_offset = cd_off
        self.cd_size = cd_size

    def _read_central(self) -> None:
        data = self.source.read(self.cd_offset, self.cd_size)
        if len(data) != self.cd_size:
            raise ZipError("diretório central truncado")
        self.members: dict[str, Member] = {}
        self.order: list[str] = []
        at = 0
        for _ in range(self.n_total):
            if data[at:at + 4] != CENTRAL_SIG:
                raise ZipError(f"entrada central inválida no byte {at}")
            (_sig, _made_by, _needed, _flags, method, mtime, mdate, crc, csize, usize,
             nlen, elen, clen, _disk, _iattr, _eattr, lho) = struct.unpack_from(
                "<4sHHHHHHIIIHHHHHII", data, at)
            name = data[at + 46:at + 46 + nlen].decode("utf-8", "replace")
            extra = data[at + 46 + nlen:at + 46 + nlen + elen]
            usize, csize, lho = _zip64_extra(extra, usize, csize, lho)
            at += 46 + nlen + elen + clen
            member = Member(name, usize, csize, method, crc, lho, _dos_time(mdate, mtime))
            self.members[name] = member
            self.order.append(name)
        self.cd_end = at

    # ------------------------------------------------------------------ leitura

    def _data_offset(self, member: Member) -> int:
        header = self.source.read(member.local_offset, 30)
        if len(header) != 30 or header[:4] != LOCAL_SIG:
            raise ZipError(f"{member.name}: cabeçalho local inválido")
        nlen, elen = struct.unpack_from("<HH", header, 26)
        return member.local_offset + 30 + nlen + elen

    def iter_chunks(self, member: Member, block: int = 1 << 20):
        """Os bytes descomprimidos de um membro, em blocos, com CRC verificado no fim."""
        if member.usize == 0 and member.method == STORED:
            return
        offset = self._data_offset(member)
        remaining = member.csize
        crc = 0
        produced = 0
        inflater = None
        if member.method == DEFLATED:
            inflater = zlib.decompressobj(-15)
        elif member.method == BZIP2:
            inflater = bz2.BZ2Decompressor()
        elif member.method != STORED:
            raise ZipError(f"{member.name}: método de compressão {member.method} não suportado")
        while remaining > 0:
            chunk = self.source.read(offset, min(block, remaining))
            if not chunk:
                raise ZipError(f"{member.name}: fim inesperado dos dados")
            offset += len(chunk)
            remaining -= len(chunk)
            if inflater is None:
                out = chunk
            else:
                out = inflater.decompress(chunk)
            if out:
                crc = zlib.crc32(out, crc)
                produced += len(out)
                yield out
        if inflater is not None and member.method == DEFLATED:
            out = inflater.flush()
            if out:
                crc = zlib.crc32(out, crc)
                produced += len(out)
                yield out
        if produced != member.usize:
            raise ZipError(f"{member.name}: {produced} bytes descomprimidos, esperava {member.usize}")
        if (crc & 0xFFFFFFFF) != member.crc:
            raise ZipError(f"{member.name}: CRC32 não confere")

    def read(self, name: str, limit: int | None = None) -> bytes:
        member = self.members.get(name)
        if member is None:
            raise ZipError(f"{name}: não está no IPSW")
        out = bytearray()
        for chunk in self.iter_chunks(member):
            out += chunk
            if limit is not None and len(out) >= limit:
                return bytes(out[:limit])
        return bytes(out)

    def extract(self, name: str, path: str) -> int:
        member = self.members.get(name)
        if member is None:
            raise ZipError(f"{name}: não está no IPSW")
        written = 0
        with open(path, "wb") as out:
            for chunk in self.iter_chunks(member):
                out.write(chunk)
                written += len(chunk)
        return written

    def crc_check(self, name: str) -> None:
        for _ in self.iter_chunks(self.members[name]):
            pass


def _zip64_extra(extra: bytes, usize: int, csize: int, lho: int):
    at = 0
    while at + 4 <= len(extra):
        tag, size = struct.unpack_from("<HH", extra, at)
        body = extra[at + 4:at + 4 + size]
        at += 4 + size
        if tag != 0x0001:
            continue
        pos = 0
        if usize == 0xFFFFFFFF and pos + 8 <= len(body):
            usize = struct.unpack_from("<Q", body, pos)[0]
            pos += 8
        if csize == 0xFFFFFFFF and pos + 8 <= len(body):
            csize = struct.unpack_from("<Q", body, pos)[0]
            pos += 8
        if lho == 0xFFFFFFFF and pos + 8 <= len(body):
            lho = struct.unpack_from("<Q", body, pos)[0]
    return usize, csize, lho


def _dos_time(mdate: int, mtime: int):
    if not mdate:
        return None
    return (1980 + (mdate >> 9), (mdate >> 5) & 0xF, mdate & 0x1F,
            mtime >> 11, (mtime >> 5) & 0x3F, (mtime & 0x1F) * 2)


def human_size(value: int) -> str:
    for unit in ("B", "KB", "MB", "GB", "TB"):
        if value < 1024 or unit == "TB":
            return f"{value:.0f} {unit}" if unit == "B" else f"{value:.1f} {unit}"
        value /= 1024.0
    return f"{value:.1f} TB"
