"""O teste que usa o IPSW de verdade. Só roda quando você aponta para ele.

    HKIPADOS_IPSW=cache/iPad_Pro_Spring_2021_27.2.1_24B156_Restore.ipsw \
        python3 -m unittest tests.test_integration -v

Nada aqui é obrigatório para a suíte: o projeto não distribui firmware.
"""

import os
import unittest

from hkipados import analyze, compat

IPSW = os.environ.get("HKIPADOS_IPSW")
URL = os.environ.get("HKIPADOS_URL")


class RealIpswTest(unittest.TestCase):
    @unittest.skipUnless(IPSW and os.path.exists(IPSW), "defina HKIPADOS_IPSW")
    def test_analyze_real_ipsw(self):
        report = analyze.analyze_ipsw(IPSW, do_hash=False, decompress=True)
        self.assertEqual(report["manifest"]["product_version"], "27.2.1")
        self.assertEqual(report["manifest"]["product_build"], "24B156")
        self.assertEqual(report["target"]["board"], "j517ap")
        self.assertEqual(report["kernel"]["type"], "krnl")
        self.assertEqual(report["kernel"]["arch"], "arm64e")
        self.assertIn("Darwin", report["kernel"]["darwin_version"])
        kinds = {image["component"]: image["kind"] for image in report["images"]}
        self.assertEqual(kinds.get("OS"), "aea")
        self.assertEqual(kinds.get("Cryptex1,SystemOS"), "aea")
        self.assertEqual(kinds.get("Cryptex1,AppOS"), "apfs")
        findings = compat.evaluate(report)
        self.assertGreater(compat.summarize(findings).get(compat.BLOCKED, 0), 0)

    @unittest.skipUnless(URL, "defina HKIPADOS_URL")
    def test_read_manifest_over_http_without_downloading_the_ipsw(self):
        from hkipados.source import open_source
        from hkipados.zipx import ZipArchive
        from hkipados.manifest import BuildManifest
        source = open_source(URL)
        archive = ZipArchive(source)
        manifest = BuildManifest.load(archive.read("BuildManifest.plist"))
        self.assertEqual(manifest.product_build, "24B156")
        self.assertLess(source.bytes_read, 8 << 20)  # não baixou o IPSW
        source.close()


if __name__ == "__main__":
    unittest.main()
