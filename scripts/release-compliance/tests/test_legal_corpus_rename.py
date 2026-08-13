from __future__ import annotations

import hashlib
import json
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[3]
RECEIPT = ROOT / "release" / "legal-corpus" / "RENAME-RECEIPT.json"


class LegalCorpusRenameTests(unittest.TestCase):
    def test_every_renamed_legal_blob_retains_its_recorded_sha256(self) -> None:
        receipt = json.loads(RECEIPT.read_text(encoding="utf-8"))
        self.assertEqual(
            receipt["contentTransformation"],
            "identifier-only-metadata-normalization",
        )
        self.assertEqual(receipt["thirdPartyLegalContentTransformation"], "none")
        self.assertEqual(len(receipt["mappings"]), 20)
        for mapping in receipt["mappings"]:
            destination = ROOT / mapping["to"]
            self.assertTrue(destination.is_file(), destination)
            self.assertEqual(
                hashlib.sha256(destination.read_bytes()).hexdigest(),
                mapping.get("currentSha256", mapping.get("sha256")),
            )
            if mapping["to"].startswith("release/legal-corpus/upstream/"):
                self.assertNotIn("transformation", mapping)
                self.assertIn("sha256", mapping)


if __name__ == "__main__":
    unittest.main()
