"""As regras de compatibilidade: o veredito não pode ser otimista."""

import unittest

from hkipados import compat


class CompatTest(unittest.TestCase):
    def test_minimal_report_still_finds_the_blockers(self):
        results = compat.evaluate({
            "target": {"device": "iPad13,4", "version": "27.2.1", "build": "24B156",
                       "size": 1, "sha256": "x", "platform": "t8103 (Apple M1)"},
            "kernel": {"description": "KernelManagement_host-514.40.8",
                       "arch": "arm64e", "cpusubtype": "0x80000002",
                       "darwin": "Darwin Kernel Version 26.0.0"},
            "images": [{"component": "OS", "path": "141-10293-225.dmg.aea",
                        "kind": "aea", "size": 8321499136}],
            "userland": {"launchd": ["/tmp/ramdisk/sbin/launchd"], "files": 1269},
        })
        statuses = {item["area"] + "/" + item["requirement"]: item["status"]
                    for item in results}
        self.assertEqual(statuses["IPSW/Baixar o firmware indicado"], compat.OK)
        self.assertEqual(statuses["Sistema de arquivos/Montar o volume do sistema do IPSW"],
                         compat.BLOCKED)
        self.assertEqual(statuses["Kernel/A arquitetura do binário bate com a do kernel"],
                         compat.BLOCKED)
        self.assertGreaterEqual(compat.summarize(results).get(compat.BLOCKED, 0), 8)

    def test_every_row_has_a_reason(self):
        for item in compat.evaluate({}):
            self.assertTrue(item["reason"], item)
            self.assertIn(item["status"], (compat.OK, compat.PARTIAL,
                                           compat.BLOCKED, compat.UNTESTED))

    def test_markdown(self):
        text = compat.render_markdown(compat.evaluate({}))
        self.assertIn("| Área | Requisito | Estado | Motivo |", text)
        self.assertIn("BLOQUEADO", text)


if __name__ == "__main__":
    unittest.main()
