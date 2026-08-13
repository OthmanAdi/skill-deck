from __future__ import annotations

import argparse
import importlib.util
import json
import tempfile
import unittest
from pathlib import Path
from unittest import mock


SCRIPT = Path(__file__).resolve().parents[1] / "generate_release_evidence.py"
SPEC = importlib.util.spec_from_file_location("release_evidence", SCRIPT)
assert SPEC and SPEC.loader
MODULE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MODULE)


class Fixture:
    def __init__(self, root: Path) -> None:
        self.root = root
        self.package_root = root / "src-tauri"
        root.mkdir(parents=True)
        (root / "release").mkdir()
        (root / "src-tauri" / "resources" / "llama").mkdir(parents=True)
        (root / "src-tauri" / "resources" / "licenses").mkdir()
        (root / "LICENSE").write_text("Mozilla Public License Version 2.0\n", encoding="utf-8")
        (root / "package.json").write_text(
            json.dumps(
                {
                    "name": "skill-deck",
                    "version": "0.3.0",
                    "author": MODULE.PRODUCT_AUTHOR,
                    "license": MODULE.PRODUCT_LICENSE,
                }
            ),
            encoding="utf-8",
        )
        (root / "product.json").write_text(
            json.dumps(
                {
                    "version": "0.3.0",
                    "copyrightHolder": MODULE.PRODUCT_AUTHOR,
                    "publisher": MODULE.PRODUCT_AUTHOR,
                    "license": MODULE.PRODUCT_LICENSE,
                    "distributionModel": "single-unlocked-product",
                }
            ),
            encoding="utf-8",
        )
        for name, content in {
            "THIRD_PARTY_NOTICES.txt": "llama.cpp and Jan-nano notices\n",
            "licenses/llama.cpp-MIT.txt": "llama legal marker\n",
            "licenses/Jan-nano-Apache-2.0.txt": "model legal marker\n",
            "licenses/Jan-nano-ATTRIBUTION.txt": "model attribution marker\n",
        }.items():
            (root / "src-tauri" / "resources" / name).write_text(content, encoding="utf-8")
        for name in ("llama-server.exe", "llama.dll"):
            (root / "src-tauri" / "resources" / "llama" / name).write_bytes(name.encode())
        contract = {
            "productVersion": "0.3.0",
            "target": MODULE.TARGET,
            "cycloneDxSpecVersion": "1.6",
            "product": {
                "distributionModel": "single-unlocked-product",
                "copyrightHolder": MODULE.PRODUCT_AUTHOR,
                "publisher": MODULE.PRODUCT_AUTHOR,
                "license": MODULE.PRODUCT_LICENSE,
            },
            "llamaRuntime": {"files": ["llama-server.exe", "llama.dll"]},
            "model": {
                "name": "Jan-nano fixture",
                "revision": "abc123",
                "license": "Apache-2.0",
                "sha256": "a" * 64,
            },
            "noticeResources": {
                "notice": "src-tauri/resources/THIRD_PARTY_NOTICES.txt",
                "licenses": [
                    {
                        "path": "src-tauri/resources/licenses/llama.cpp-MIT.txt",
                        "requiredFragments": ["llama legal marker"],
                    },
                    {
                        "path": "src-tauri/resources/licenses/Jan-nano-Apache-2.0.txt",
                        "requiredFragments": ["model legal marker"],
                    },
                    {
                        "path": "src-tauri/resources/licenses/Jan-nano-ATTRIBUTION.txt",
                        "requiredFragments": ["model attribution marker"],
                    },
                ],
            },
            "manualLicenseReviewTokens": ["MPL-2.0"],
        }
        (root / "release" / "compliance-contract.json").write_text(
            json.dumps(contract), encoding="utf-8"
        )

    def args(self, output: Path, allow_missing: bool = True) -> argparse.Namespace:
        return argparse.Namespace(
            root=self.root,
            package_root=self.package_root,
            output=output,
            allow_missing_artifacts=allow_missing,
        )


def fake_cargo() -> tuple[list[dict[str, object]], dict[str, list[str]]]:
    reference = "pkg:cargo/skill-deck@0.3.0"
    return (
        [
            {
                "type": "application",
                "bom-ref": reference,
                "name": "skill-deck",
                "version": "0.3.0",
                "purl": reference,
                "licenses": [{"license": {"name": MODULE.PRODUCT_LICENSE}}],
            }
        ],
        {reference: []},
    )


class ReleaseEvidenceTests(unittest.TestCase):
    def test_single_product_evidence_is_deterministic_and_complete(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            fixture = Fixture(Path(temporary) / "project")
            first = Path(temporary) / "first"
            second = Path(temporary) / "second"
            provenance = {
                "repository": "https://github.com/OthmanAdi/skill-deck",
                "commit": "1" * 40,
                "dirty": False,
            }
            with (
                mock.patch.object(MODULE, "_cargo_components", return_value=fake_cargo()),
                mock.patch.object(MODULE, "_npm_components", return_value=[]),
                mock.patch.object(MODULE, "_git_provenance", return_value=provenance),
            ):
                MODULE.generate(fixture.args(first))
                MODULE.generate(fixture.args(second))
            self.assertEqual(
                {p.name: p.read_bytes() for p in first.iterdir()},
                {p.name: p.read_bytes() for p in second.iterdir()},
            )
            evidence = json.loads((first / "release-evidence.json").read_text())
            self.assertEqual(evidence["distributionModel"], "single-unlocked-product")
            self.assertEqual(evidence["resourceCount"], 6)
            self.assertNotIn("edition", json.dumps(evidence).casefold())
            self.assertTrue((first / "skill-deck-0.3.0.cdx.json").is_file())
            self.assertTrue((first / "SHA256SUMS.txt").is_file())
            bom = json.loads((first / "skill-deck-0.3.0.cdx.json").read_text())
            self.assertEqual(bom["metadata"]["component"]["author"], MODULE.PRODUCT_AUTHOR)
            self.assertEqual(
                bom["metadata"]["component"]["licenses"],
                [{"license": {"id": MODULE.PRODUCT_LICENSE}}],
            )

    def test_package_license_and_author_must_match_public_identity(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            fixture = Fixture(Path(temporary) / "project")
            package_path = fixture.root / "package.json"
            package = json.loads(package_path.read_text())
            package["license"] = "MIT"
            package_path.write_text(json.dumps(package), encoding="utf-8")
            with self.assertRaisesRegex(MODULE.ComplianceError, "license or author"):
                MODULE.generate(fixture.args(Path(temporary) / "out"))

    def test_product_contract_must_be_single_unlocked_product(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            fixture = Fixture(Path(temporary) / "project")
            product_path = fixture.root / "product.json"
            product = json.loads(product_path.read_text())
            product["distributionModel"] = "segmented"
            product_path.write_text(json.dumps(product), encoding="utf-8")
            with self.assertRaisesRegex(MODULE.ComplianceError, "single unlocked product"):
                MODULE.generate(fixture.args(Path(temporary) / "out"))

    def test_internal_developer_tools_are_rejected_from_package_tree(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            fixture = Fixture(Path(temporary) / "project")
            forbidden = fixture.package_root / "target" / "release" / "examples"
            forbidden.mkdir(parents=True)
            (forbidden / "license-keygen.exe").write_bytes(b"MZ")
            with self.assertRaisesRegex(MODULE.ComplianceError, "internal developer tools"):
                MODULE._artifacts(fixture.package_root, allow_missing=True)

    def test_artifacts_are_required_without_explicit_source_only_mode(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            fixture = Fixture(Path(temporary) / "project")
            with self.assertRaisesRegex(MODULE.ComplianceError, "no Windows release artifacts"):
                MODULE._artifacts(fixture.package_root, allow_missing=False)


if __name__ == "__main__":
    unittest.main()
