"""Os formatos: BuildManifest, IM4P, Mach-O, AEA e a detecção de APFS selado."""

import plistlib
import struct
import unittest

import os
import tempfile

from hkipados import aea, apfsx, decmpfs, deps, im4p, macho
from hkipados.manifest import BuildManifest
from hkipados.source import LocalSource


def der(tag, body):
    if len(body) < 0x80:
        length = bytes([len(body)])
    else:
        raw = len(body).to_bytes(4, "big")
        length = bytes([0x84]) + raw
    return bytes([tag]) + length + body


def build_im4p(kind="krnl", description="KernelManagement_host-514.40.8",
               payload=b"bvx2dados", wrapped=True):
    inner = (der(0x16, b"IM4P") + der(0x16, kind.encode()) +
             der(0x16, description.encode()) + der(0x04, payload) +
             der(0x16, b"IM4M") + der(0x04, b"\x00"))
    return der(0x30, inner) if wrapped else inner


def build_macho():
    xml = plistlib.dumps({"platform-application": True, "task_for_pid-allow": True})
    blob = struct.pack(">III", 0xFADE0CC0, 20 + 8 + len(xml), 1)
    blob += struct.pack(">II", 5, 20)
    blob += struct.pack(">II", 0xFADE7171, 8 + len(xml)) + xml
    commands = b""
    commands += struct.pack("<IIIIII", macho.LC_BUILD_VERSION, 24, 2,
                            0x001A0400, 0x001A0400, 0)
    commands += struct.pack("<IIII", macho.LC_DYLD_CHAINED_FIXUPS, 16, 0, 0)
    dataoff = 32 + len(commands) + 16
    commands += struct.pack("<IIII", macho.LC_CODE_SIGNATURE, 16, dataoff, len(blob))
    header = struct.pack("<IiiIIIII", macho.MH_MAGIC_64, macho.CPU_TYPE_ARM64,
                         macho.CPU_SUBTYPE_ARM64E, 2, 3, len(commands), 0, 0)
    return header + commands + blob


class ManifestTest(unittest.TestCase):
    def test_identity_and_digest(self):
        raw = {
            "ProductVersion": "27.2.1",
            "ProductBuildVersion": "24B156",
            "SupportedProductTypes": ["iPad13,4"],
            "BuildIdentities": [{
                "Ap,ProductType": "iPad13,4",
                "Info": {"DeviceClass": "j517ap", "Variant": "Customer Erase Install (IPSW)",
                         "RestoreBehavior": "Erase", "VariantContents": {"OS": "Production"}},
                "Manifest": {
                    "KernelCache": {"Digest": b"\x01" * 20,
                                    "Info": {"Path": "kernelcache.release.ipad13",
                                             "Personalize": True}},
                },
            }],
        }
        manifest = BuildManifest.load(plistlib.dumps(raw))
        self.assertEqual(manifest.product_version, "27.2.1")
        self.assertEqual(manifest.product_build, "24B156")
        identity = manifest.identity("iPad13,4", "Erase")
        self.assertEqual(identity.device_class, "j517ap")
        component = identity.component("KernelCache")
        self.assertEqual(component.path, "kernelcache.release.ipad13")
        self.assertEqual(component.algorithm, "sha1")
        self.assertTrue(component.personalized)
        self.assertIsNone(manifest.identity("iPad99,9"))


class Im4pTest(unittest.TestCase):
    def test_wrapped_form(self):
        parsed = im4p.parse(build_im4p())
        self.assertEqual(parsed.type, "krnl")
        self.assertEqual(parsed.description, "KernelManagement_host-514.40.8")
        self.assertEqual(parsed.payload, b"bvx2dados")
        self.assertEqual(parsed.compression, "lzfse")
        self.assertEqual(parsed.prefix, 4)  # 0x30, o comprimento, e 0x16 0x04

    def test_classic_form(self):
        data = b"IM4P" + b"dtre" + der(0x16, b"EmbeddedDeviceTrees") + der(0x04, b"abc")
        parsed = im4p.parse(data)
        self.assertEqual(parsed.type, "dtre")
        self.assertEqual(parsed.payload, b"abc")
        self.assertIsNone(parsed.compression)

    def test_lzfse_without_module_is_explained(self):
        parsed = im4p.parse(build_im4p(payload=b"bvx2" + b"\x00" * 10))
        try:
            import lzfse  # noqa: F401
        except ImportError:
            with self.assertRaises(im4p.Im4pError):
                parsed.decompress()


class MachOTest(unittest.TestCase):
    def test_arm64e_build_version_and_entitlements(self):
        parsed = macho.parse(build_macho())
        self.assertEqual(parsed.arch, "arm64e")
        self.assertEqual(parsed.platform, "iOS")
        self.assertEqual(parsed.minos, "26.4.0")
        self.assertTrue(parsed.chained_fixups)
        self.assertEqual(parsed.filetype_name, "execute")
        self.assertIn("platform-application", parsed.entitlements_xml)
        self.assertEqual(sorted(parsed.as_dict()["entitlements"]),
                         ["platform-application", "task_for_pid-allow"])

    def test_rejects_garbage(self):
        with self.assertRaises(ValueError):
            macho.parse(b"not a mach-o at all............")


def build_linked_macho(name: str) -> bytes:
    """Um MH_EXECUTE mínimo com um LC_LOAD_DYLIB apontando para NAME."""
    payload = name.encode() + b"\0"
    size = (24 + len(payload) + 7) & ~7
    command = struct.pack("<IIIIII", macho.LC_LOAD_DYLIB, size, 24, 0, 0x10000, 0x10000)
    command += payload.ljust(size - 24, b"\0")
    header = struct.pack("<IiiIIIII", macho.MH_MAGIC_64, macho.CPU_TYPE_ARM64,
                         macho.CPU_SUBTYPE_ARM64E, 2, 1, len(command), 0, 0)
    return header + command


class DepsTest(unittest.TestCase):
    def test_dylibs_are_parsed_and_resolved(self):
        with tempfile.TemporaryDirectory() as root:
            os.makedirs(os.path.join(root, "bin"))
            with open(os.path.join(root, "bin", "tool"), "wb") as handle:
                handle.write(build_linked_macho("/usr/lib/libSystem.B.dylib"))
            report = deps.analyze_tree(root)
            self.assertEqual(report["binaries_scanned"], 1)
            self.assertEqual(report["dylibs_unique"], 1)
            self.assertEqual(report["dylibs_present"], 0)
            self.assertEqual(report["missing_in_shared_cache"], ["/usr/lib/libSystem.B.dylib"])
            os.makedirs(os.path.join(root, "usr", "lib"))
            with open(os.path.join(root, "usr", "lib", "libSystem.B.dylib"), "wb") as handle:
                handle.write(b"not really a mach-o")
            report = deps.analyze_tree(root)
            self.assertEqual(report["dylibs_present"], 1)
            self.assertEqual(report["missing"], {})

    def test_symlinked_binary_is_skipped(self):
        with tempfile.TemporaryDirectory() as root:
            with open(os.path.join(root, "real"), "wb") as handle:
                handle.write(build_linked_macho("/usr/lib/libobjc.A.dylib"))
            os.symlink("real", os.path.join(root, "link"))
            report = deps.analyze_tree(root)
            self.assertEqual([row["path"] for row in report["binaries"]], ["real"])


class AeaTest(unittest.TestCase):
    def test_header_fields(self):
        import tempfile, os
        body = b"com.apple.wkms.url\0https://wkms.sd.apple.com"
        body2 = b"com.apple.wkms.auth-data\0QUJD"
        header = b"AEA1" + struct.pack("<II", 1, 12 + 4 + len(body) + 4 + len(body2))
        header += struct.pack("<I", len(body)) + body
        header += struct.pack("<I", len(body2)) + body2
        with tempfile.NamedTemporaryFile(delete=False) as handle:
            handle.write(header)
            path = handle.name
        try:
            with LocalSource(path) as source:
                parsed = aea.probe(source)
            self.assertEqual(parsed.version, 1)
            self.assertEqual(parsed.key_service, "https://wkms.sd.apple.com")
            self.assertTrue(parsed.has_auth_data)
            self.assertFalse(parsed.as_dict()["decryptable_here"])
        finally:
            os.unlink(path)


class ApfsDetectionTest(unittest.TestCase):
    def test_sealed_node_is_recognised(self):
        block = bytes(24) + b"\x00" * 8 + struct.pack("<HHI", 0x19, 2, 3) + bytes(4096 - 40)
        flags = apfsx.node_flags(block)
        self.assertTrue(flags["zero_header"])
        self.assertTrue(flags["noheader"])
        self.assertTrue(flags["root"])
        self.assertFalse(flags["leaf"])
        self.assertEqual(flags["level"], 2)

    def test_raw_apfs_signature(self):
        data = bytearray(4096)
        data[32:36] = b"NXSB"
        struct.pack_into("<I", data, 36, 4096)
        struct.pack_into("<Q", data, 40, 1234)
        kind = apfsx.image_kind_bytes(bytes(data))
        self.assertEqual(kind["kind"], "apfs")
        self.assertEqual(kind["block_size"], 4096)
        self.assertEqual(kind["block_count"], 1234)

    def test_apfs_after_a_prefix(self):
        # o RestoreRamDisk tem 27 bytes antes do NXSB
        data = bytearray(27 + 4096)
        data[27 + 32:27 + 36] = b"NXSB"
        struct.pack_into("<I", data, 27 + 36, 4096)
        struct.pack_into("<Q", data, 27 + 40, 59904)
        kind = apfsx.image_kind_bytes(bytes(data))
        self.assertEqual(kind["kind"], "apfs")
        self.assertEqual(kind["apfs_offset"], 27)
        self.assertEqual(kind["block_count"], 59904)


class DecmpfsTest(unittest.TestCase):
    def header(self, kind=14, size=168864):
        return struct.pack("<IIQ", decmpfs.MAGIC, kind, size)

    def test_header_names_the_compression(self):
        header = decmpfs.parse(self.header())
        self.assertEqual(header.compression_type, 14)
        self.assertEqual(header.compression, "LZBITMAP")
        self.assertEqual(header.uncompressed_size, 168864)

    def test_bad_magic_is_rejected(self):
        with self.assertRaises(ValueError):
            decmpfs.parse(b"\x00" * 16)

    def test_resource_fork_container_and_fat_prefix(self):
        # o resource fork real começa com um cabeçalho de 16 bytes, depois ZBM
        blob = bytes(16) + b"ZBM\x09" + bytes(12) + b"\xca\xfe\xba\xbe" + struct.pack(">I", 2)
        self.assertEqual(decmpfs.resource_container(blob), "ZBM")
        found = decmpfs.find_fat(blob)
        self.assertIsNotNone(found)
        self.assertEqual(found["slices"], 2)
        self.assertEqual(found["offset"], 32)


if __name__ == "__main__":
    unittest.main()
