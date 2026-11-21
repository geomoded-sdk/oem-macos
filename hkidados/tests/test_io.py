"""Fontes e ZIP: arquivo local, HTTP com Range, e o ZIP64 que o IPSW usa."""

import http.server
import os
import struct
import tempfile
import threading
import unittest
import zipfile
import zlib

from hkipados.source import HttpRangeSource, LocalSource, SourceError
from hkipados.zipx import ZipArchive, _zip64_extra


class RangeHandler(http.server.BaseHTTPRequestHandler):
    """Um servidor HTTP real que entende Range, como a CDN da Apple."""

    path_to_file = None

    def log_message(self, *args):
        pass

    def do_HEAD(self):
        size = os.path.getsize(self.path_to_file)
        self.send_response(200)
        self.send_header("Content-Length", str(size))
        self.send_header("Accept-Ranges", "bytes")
        self.end_headers()

    def do_GET(self):
        size = os.path.getsize(self.path_to_file)
        header = self.headers.get("Range")
        if not header:
            with open(self.path_to_file, "rb") as handle:
                body = handle.read()
            self.send_response(200)
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)
            return
        first, last = header.split("=", 1)[1].split("-", 1)
        first = int(first)
        last = int(last) if last else size - 1
        with open(self.path_to_file, "rb") as handle:
            handle.seek(first)
            body = handle.read(last - first + 1)
        self.send_response(206)
        self.send_header("Content-Range", f"bytes {first}-{last}/{size}")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)


class NoRangeHandler(RangeHandler):
    def do_GET(self):
        with open(self.path_to_file, "rb") as handle:
            body = handle.read()
        self.send_response(200)
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)


class HttpSourceTest(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.payload = bytes(range(256)) * 40          # 10240 bytes
        self.path = os.path.join(self.tmp.name, "blob.bin")
        with open(self.path, "wb") as handle:
            handle.write(self.payload)
        RangeHandler.path_to_file = self.path
        self.server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), RangeHandler)
        self.thread = threading.Thread(target=self.server.serve_forever, daemon=True)
        self.thread.start()
        self.url = f"http://127.0.0.1:{self.server.server_port}/blob.bin"

    def tearDown(self):
        self.server.shutdown()
        self.server.server_close()
        self.thread.join(timeout=5)
        self.tmp.cleanup()

    def test_reads_exact_ranges_across_block_boundaries(self):
        with HttpRangeSource(self.url, chunk=1024, cache_blocks=2) as source:
            self.assertEqual(source.size, len(self.payload))
            self.assertEqual(source.read(0, 10), self.payload[:10])
            self.assertEqual(source.read(1000, 2000), self.payload[1000:3000])
            self.assertEqual(source.read(10000, 5000), self.payload[10000:])
            self.assertEqual(source.read(10240, 10), b"")

    def test_refuses_a_server_that_ignores_range(self):
        server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), NoRangeHandler)
        thread = threading.Thread(target=server.serve_forever, daemon=True)
        thread.start()
        try:
            url = f"http://127.0.0.1:{server.server_port}/blob.bin"
            source = HttpRangeSource(url, chunk=1024)
            with self.assertRaises(SourceError):
                source.read(0, 10)
        finally:
            server.shutdown()
            server.server_close()
            thread.join(timeout=5)


class LocalSourceTest(unittest.TestCase):
    def test_read_and_size(self):
        with tempfile.NamedTemporaryFile(delete=False) as handle:
            handle.write(b"abcdefghij")
            path = handle.name
        try:
            with LocalSource(path) as source:
                self.assertEqual(source.size, 10)
                self.assertEqual(source.read(2, 3), b"cde")
                self.assertEqual(source.read(8, 100), b"ij")
        finally:
            os.unlink(path)


class ZipTest(unittest.TestCase):
    def make_zip(self, path):
        with zipfile.ZipFile(path, "w", zipfile.ZIP_DEFLATED) as archive:
            archive.writestr("BuildManifest.plist", b"<plist>teste</plist>")
            archive.writestr("Firmware/all_flash/DeviceTree.j517ap.im4p", b"x" * 5000)
            info = zipfile.ZipInfo("stored.bin")
            info.compress_type = zipfile.ZIP_STORED
            archive.writestr(info, b"stored data")

    def test_members_and_extract(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = os.path.join(tmp, "test.ipsw")
            self.make_zip(path)
            with LocalSource(path) as source:
                archive = ZipArchive(source)
                self.assertEqual(archive.n_total, 3)
                self.assertIn("BuildManifest.plist", archive.members)
                self.assertEqual(archive.read("BuildManifest.plist"), b"<plist>teste</plist>")
                self.assertEqual(archive.read("stored.bin"), b"stored data")
                self.assertEqual(len(archive.read("Firmware/all_flash/DeviceTree.j517ap.im4p")), 5000)
                out = os.path.join(tmp, "out.bin")
                self.assertEqual(archive.extract("stored.bin", out), 11)
                with open(out, "rb") as handle:
                    self.assertEqual(handle.read(), b"stored data")

    def test_truncated_member_raises(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = os.path.join(tmp, "test.ipsw")
            self.make_zip(path)
            with open(path, "rb") as handle:
                data = bytearray(handle.read())
            # estraga o CRC no diretório central da entrada "stored.bin"
            at = data.rfind(b"stored.bin")
            central = data.rfind(b"PK\x01\x02", 0, at)
            struct.pack_into("<I", data, central + 16, 0xDEADBEEF)
            with open(path, "wb") as handle:
                handle.write(data)
            with LocalSource(path) as source:
                archive = ZipArchive(source)
                with self.assertRaises(Exception):
                    archive.read("stored.bin")

    def test_zip64_extra(self):
        body = struct.pack("<QQQ", 0x1_0000_0000, 0x2000_0000, 0x3000_0000)
        extra = struct.pack("<HH", 0x0001, len(body)) + body
        self.assertEqual(_zip64_extra(extra, 0xFFFFFFFF, 0xFFFFFFFF, 0xFFFFFFFF),
                         (0x1_0000_0000, 0x2000_0000, 0x3000_0000))

    def test_crc_check_passes(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = os.path.join(tmp, "test.ipsw")
            self.make_zip(path)
            with LocalSource(path) as source:
                archive = ZipArchive(source)
                for name in archive.order:
                    archive.crc_check(name)


if __name__ == "__main__":
    unittest.main()
