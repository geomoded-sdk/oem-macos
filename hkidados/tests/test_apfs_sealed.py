"""O leitor APFS selado e o decodificador LZBITMAP, com dados de verdade quando há IPSW.

    HKIPADOS_IPSW=cache/iPad_Pro_Spring_2021_27.2.1_24B156_Restore.ipsw \
        python3 -m unittest tests.test_apfs_sealed -v

Os testes sem IPSW usam estruturas montadas à mão: um bloco decmpfs tipo 14 com um
único bloco literal (0xff), que é como o `unzbm.c` do HakiOS reconhece um bloco que
não encolheu, e um IM4P pequeno. Nenhum deles precisa de firmware.
"""

import os
import struct
import tempfile
import unittest

from hkipados import apfs_sealed, apfsx, decmpfs, im4p
from hkipados.vendor.hakios import apfs

IPSW = os.environ.get("HKIPADOS_IPSW")


def literal_decmpfs(payload: bytes) -> tuple[bytes, bytes]:
    """Cabeçalho e resource fork de um arquivo tipo 14 guardado como um bloco literal."""
    header = b"fpmc" + struct.pack("<IQ", decmpfs.TYPE_LZBITMAP, len(payload))
    fork = struct.pack("<II", 8, 8 + len(payload) + 1) + b"\xff" + payload
    return header, fork


class LzbitmapTest(unittest.TestCase):
    def test_literal_block_round_trip(self):
        payload = bytes(range(256)) * 4
        header, fork = literal_decmpfs(payload)
        self.assertEqual(decmpfs.decompress(header, fork), payload)

    def test_wrong_type_is_refused(self):
        header = b"fpmc" + struct.pack("<IQ", 4, 16)
        with self.assertRaises(ValueError) as caught:
            decmpfs.decompress(header, b"\0" * 16)
        self.assertIn("LZBITMAP", str(caught.exception))

    def test_short_block_table_is_refused(self):
        header, fork = literal_decmpfs(b"x" * 100)
        with self.assertRaises(ValueError) as caught:
            decmpfs.decompress(header, fork[:4])
        self.assertIn("tabela", str(caught.exception))

    def test_library_builds(self):
        path, error = decmpfs.build_libzbitmap("build/lib/libzbitmap.so")
        self.assertIsNone(error)
        self.assertTrue(os.path.exists(path))


class Im4pLocateTest(unittest.TestCase):
    def test_small_im4p(self):
        payload = b"NXSB" + b"\0" * 60
        body = (b"\x16\x04IM4P" + b"\x16\x04rdsk" + b"\x16\x010"
                + b"\x04" + bytes([len(payload)]) + payload)
        data = b"\x30\x81" + bytes([len(body)]) + body
        kind, description, offset, length = im4p.locate(data)
        self.assertEqual(kind, "rdsk")
        self.assertEqual(description, "0")
        self.assertEqual(length, len(payload))
        self.assertEqual(data[offset:offset + length], payload)


class SealedBlockTest(unittest.TestCase):
    def test_noheader_detection(self):
        block = bytearray(4096)
        struct.pack_into("<H", block, 32, apfs.BTNODE_NOHEADER)
        self.assertTrue(apfs_sealed._is_noheader(bytes(block)))
        struct.pack_into("<Q", block, 0, 1)      # um obj_phys de verdade
        self.assertFalse(apfs_sealed._is_noheader(bytes(block)))


class RealCryptexTest(unittest.TestCase):
    """O cryptex de App do IPSW: 1155 arquivos, 18 422 104 bytes, todos conferidos."""

    @unittest.skipUnless(IPSW and os.path.exists(IPSW), "defina HKIPADOS_IPSW")
    def test_sealed_cryptex_matches_known_totals(self):
        from hkipados.source import LocalSource
        from hkipados.zipx import ZipArchive
        from hkipados.manifest import BuildManifest

        archive = ZipArchive(LocalSource(IPSW))
        manifest = BuildManifest.load(archive.read("BuildManifest.plist"))
        identity = manifest.identity("iPad13,4", "Erase")
        member = identity.path_of("Cryptex1,AppOS")
        self.assertIsNotNone(member)
        with tempfile.TemporaryDirectory() as tmp:
            image = os.path.join(tmp, "app.dmg")
            archive.extract(member, image)
            self.assertTrue(apfsx.is_apfs(image))
            container, volume = apfs_sealed.open_sealed(image)
            self.assertTrue(volume.sealed)
            stats = apfs_sealed.extract(volume, os.path.join(tmp, "out"),
                                        decoder=decmpfs.decompress)
            container.close()
            self.assertEqual(stats["errors"], [])
            self.assertEqual(stats["files"], 1155)
            self.assertEqual(stats["bytes"], 18422104)
            self.assertEqual(stats["decmpfs_streams"], 869)
            self.assertGreater(stats["hash_checked"], 4000)

    @unittest.skipUnless(IPSW and os.path.exists(IPSW), "defina HKIPADOS_IPSW")
    def test_ramdisk_is_im4p_wrapped_apfs(self):
        from hkipados.source import LocalSource
        from hkipados.zipx import ZipArchive
        from hkipados.manifest import BuildManifest

        archive = ZipArchive(LocalSource(IPSW))
        manifest = BuildManifest.load(archive.read("BuildManifest.plist"))
        identity = manifest.identity("iPad13,4", "Erase")
        member = identity.path_of("RestoreRamDisk")
        with tempfile.TemporaryDirectory() as tmp:
            image = os.path.join(tmp, "rd.dmg")
            archive.extract(member, image)
            with open(image, "rb") as handle:
                kind, _description, offset, length = im4p.locate(handle.read(4096))
            self.assertEqual(kind, "rdsk")
            raw = apfsx.to_raw(image, os.path.join(tmp, "rd.raw"))
            self.assertEqual(os.path.getsize(raw), length)
            container, volume = apfs_sealed.open_sealed(raw)
            self.assertFalse(volume.sealed)
            stats = apfs_sealed.extract(volume, os.path.join(tmp, "out"))
            container.close()
            self.assertEqual(stats["files"], 1260)
            self.assertEqual(stats["symlinks"], 306)
            self.assertEqual(stats["bytes"], 1236840790)


if __name__ == "__main__":
    unittest.main()
