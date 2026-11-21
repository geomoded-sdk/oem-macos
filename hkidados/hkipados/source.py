"""Fontes de bytes com acesso aleatório: um arquivo local ou uma URL com HTTP Range.

O IPSW tem 11 GB. Ler o `BuildManifest.plist` (185 KB comprimidos) não pode custar
11 GB de download, então a fonte HTTP pede só os intervalos necessários. O cache é
por blocos alinhados, porque a leitura de um membro do ZIP é sequencial e o ZIP
guarda os tamanhos no diretório central, não no cabeçalho local.
"""

from __future__ import annotations

import os
import time
import urllib.error
import urllib.request

DEFAULT_CHUNK = 1 << 20
DEFAULT_CACHE = 64


class SourceError(Exception):
    pass


class LocalSource:
    """Um arquivo no disco. `read` não altera a posição de ninguém."""

    def __init__(self, path: str):
        self.path = path
        self.fd = os.open(path, os.O_RDONLY)
        self.size = os.fstat(self.fd).st_size

    def read(self, offset: int, length: int) -> bytes:
        if offset < 0 or length < 0:
            raise SourceError("offset e length precisam ser >= 0")
        return os.pread(self.fd, length, offset)

    def close(self) -> None:
        os.close(self.fd)

    def __enter__(self):
        return self

    def __exit__(self, *exc):
        self.close()


class HttpRangeSource:
    """Uma URL lida por intervalos, com cache de blocos alinhados.

    `Accept-Ranges: bytes` é exigido. Se o servidor ignorar o `Range` e devolver 200
    com o arquivo inteiro, a leitura falha em vez de baixar 11 GB por acidente.
    """

    def __init__(self, url: str, chunk: int = DEFAULT_CHUNK, cache_blocks: int = DEFAULT_CACHE,
                 timeout: float = 60.0, retries: int = 3):
        self.url = url
        self.chunk = chunk
        self.cache_blocks = cache_blocks
        self.timeout = timeout
        self.retries = retries
        self._cache: dict[int, bytes] = {}
        self._order: list[int] = []
        self.requests = 0
        self.bytes_read = 0
        self.size = self._probe_size()

    def _request(self, first: int, last: int) -> bytes:
        last_error: Exception | None = None
        for attempt in range(self.retries):
            request = urllib.request.Request(
                self.url, headers={"Range": f"bytes={first}-{last}", "User-Agent": "HkiPadOS/0.1"})
            try:
                with urllib.request.urlopen(request, timeout=self.timeout) as response:
                    if response.status == 206:
                        body = response.read()
                    elif response.status == 200:
                        raise SourceError(
                            "o servidor ignorou o Range e devolveu o arquivo inteiro; "
                            "recusando baixar %d bytes" % self.size)
                    else:
                        raise SourceError("HTTP %d" % response.status)
                self.requests += 1
                self.bytes_read += len(body)
                return body
            except (urllib.error.URLError, TimeoutError, SourceError) as error:
                last_error = error
                if isinstance(error, SourceError):
                    raise
                time.sleep(0.5 * (attempt + 1))
        raise SourceError(f"{self.url}: {last_error}")

    def _probe_size(self) -> int:
        request = urllib.request.Request(self.url, method="HEAD",
                                         headers={"User-Agent": "HkiPadOS/0.1"})
        try:
            with urllib.request.urlopen(request, timeout=self.timeout) as response:
                length = response.headers.get("Content-Length")
                ranges = (response.headers.get("Accept-Ranges") or "").lower()
        except urllib.error.URLError as error:
            raise SourceError(f"{self.url}: {error}") from error
        if not length:
            raise SourceError(f"{self.url}: resposta sem Content-Length")
        if ranges and "bytes" not in ranges:
            raise SourceError(f"{self.url}: o servidor não anuncia Accept-Ranges: bytes")
        return int(length)

    def _block(self, index: int) -> bytes:
        data = self._cache.get(index)
        if data is not None:
            return data
        first = index * self.chunk
        last = min(first + self.chunk, self.size) - 1
        data = self._request(first, last)
        self._cache[index] = data
        self._order.append(index)
        while len(self._order) > self.cache_blocks:
            self._cache.pop(self._order.pop(0), None)
        return data

    def read(self, offset: int, length: int) -> bytes:
        if offset < 0 or length < 0:
            raise SourceError("offset e length precisam ser >= 0")
        if offset >= self.size:
            return b""
        length = min(length, self.size - offset)
        out = bytearray()
        while length > 0:
            index, within = divmod(offset, self.chunk)
            block = self._block(index)
            take = min(length, len(block) - within)
            if take <= 0:
                break
            out += block[within:within + take]
            offset += take
            length -= take
        return bytes(out)

    def close(self) -> None:
        self._cache.clear()
        self._order.clear()

    def __enter__(self):
        return self

    def __exit__(self, *exc):
        self.close()


def open_source(path_or_url: str) -> LocalSource | HttpRangeSource:
    if path_or_url.startswith(("http://", "https://")):
        return HttpRangeSource(path_or_url)
    return LocalSource(path_or_url)
