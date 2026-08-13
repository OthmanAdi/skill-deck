from __future__ import annotations

import argparse
import hashlib
import importlib.util
import json
import os
import shutil
import tempfile
import unittest
import zipfile
from contextlib import nullcontext
from pathlib import Path
from unittest import mock


SCRIPT = Path(__file__).resolve().parents[1] / "legal_notice_corpus.py"
FIXTURES = Path(__file__).resolve().parent / "fixtures" / "legal-notices"
SPEC = importlib.util.spec_from_file_location("legal_notice_corpus", SCRIPT)
assert SPEC is not None and SPEC.loader is not None
MODULE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MODULE)


class CorpusFixture:
    def __init__(self, root: Path) -> None:
        self.root = root
        self.product = root / "product"
        self.metadata = root / "metadata"
        self.cargo_source = root / "cargo-registry" / "dual-crate-1.0.0"
        self.npm_source = (
            self.metadata / "node_modules" / ".pnpm" / "npm-shared@2.0.0"
            / "node_modules" / "npm-shared"
        )
        self.outputs = root / "outputs"
        self.upstream = (
            self.metadata / "release" / "legal-corpus" / "upstream"
        )
        self.source_commit = "1" * 40
        for path in (
            self.product / "src-tauri",
            self.metadata,
            self.cargo_source / "src",
            self.npm_source,
            self.outputs,
        ):
            path.mkdir(parents=True)

        (self.product / "package.json").write_text(
            json.dumps(
                {
                    "name": "skill-deck",
                    "version": "0.3.0",
                    "dependencies": {"npm-shared": "2.0.0"},
                }
            ),
            encoding="utf-8",
        )
        shutil.copy2(self.product / "package.json", self.metadata / "package.json")
        lock = """lockfileVersion: '9.0'

packages:

  npm-shared@2.0.0:
    resolution: {integrity: sha512-YWJj}

snapshots:

  npm-shared@2.0.0: {}
"""
        (self.product / "pnpm-lock.yaml").write_text(lock, encoding="utf-8")
        (self.metadata / "pnpm-lock.yaml").write_text(lock, encoding="utf-8")
        (self.product / "src-tauri" / "Cargo.lock").write_text(
            """version = 3

[[package]]
name = "skill-deck"
version = "0.3.0"

[[package]]
name = "dual-crate"
version = "1.0.0"
source = "registry+https://example.invalid/index"
checksum = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"

[[package]]
name = "dev-only"
version = "9.0.0"
source = "registry+https://example.invalid/index"
checksum = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
""",
            encoding="utf-8",
        )
        (self.cargo_source / "Cargo.toml").write_text(
            '[package]\nname = "dual-crate"\nversion = "1.0.0"\n',
            encoding="utf-8",
        )
        (self.cargo_source / "src" / "lib.rs").write_text(
            "pub fn tiny() {}\n", encoding="utf-8"
        )
        shutil.copy2(FIXTURES / "LICENSE-MIT", self.cargo_source / "LICENSE-MIT")
        shutil.copy2(
            FIXTURES / "LICENSE-APACHE", self.cargo_source / "LICENSE-APACHE"
        )
        shutil.copy2(FIXTURES / "NOTICE", self.cargo_source / "NOTICE")
        (self.npm_source / "package.json").write_text(
            json.dumps(
                {"name": "npm-shared", "version": "2.0.0", "license": "MIT"}
            ),
            encoding="utf-8",
        )
        shutil.copy2(FIXTURES / "LICENSE-SHARED", self.npm_source / "LICENSE")

        root_id = "path+file:///fixture/skill-deck#0.3.0"
        dual_id = (
            "registry+https://example.invalid/index#dual-crate@1.0.0"
        )
        dev_id = "registry+https://example.invalid/index#dev-only@9.0.0"
        self.cargo_metadata = self.root / "cargo-metadata.json"
        self.cargo_metadata.write_text(
            json.dumps(
                {
                    "packages": [
                        {
                            "id": root_id,
                            "name": "skill-deck",
                            "version": "0.3.0",
                            "manifest_path": str(
                                self.product / "src-tauri" / "Cargo.toml"
                            ),
                            "license": "MIT",
                        },
                        {
                            "id": dual_id,
                            "name": "dual-crate",
                            "version": "1.0.0",
                            "manifest_path": str(
                                self.cargo_source / "Cargo.toml"
                            ),
                            "license": "MIT OR Apache-2.0",
                            "source": "registry+https://example.invalid/index",
                        },
                        {
                            "id": dev_id,
                            "name": "dev-only",
                            "version": "9.0.0",
                            "manifest_path": str(
                                self.root / "unused" / "Cargo.toml"
                            ),
                            "license": "MIT",
                            "source": "registry+https://example.invalid/index",
                        },
                    ],
                    "resolve": {
                        "root": root_id,
                        "nodes": [
                            {
                                "id": root_id,
                                "deps": [
                                    {
                                        "pkg": dual_id,
                                        "dep_kinds": [
                                            {"kind": None, "target": None}
                                        ],
                                    },
                                    {
                                        "pkg": dev_id,
                                        "dep_kinds": [
                                            {"kind": "dev", "target": None}
                                        ],
                                    },
                                ],
                            },
                            {"id": dual_id, "deps": []},
                            {"id": dev_id, "deps": []},
                        ],
                    },
                },
                indent=2,
            ),
            encoding="utf-8",
        )
        self.pnpm_list = self.root / "pnpm-list.json"
        self.pnpm_list.write_text(
            json.dumps(
                [
                    {
                        "name": "skill-deck",
                        "version": "0.3.0",
                        "dependencies": {
                            "npm-shared": {
                                "from": "npm-shared",
                                "version": "2.0.0",
                                "path": str(self.npm_source),
                                "dependencies": {},
                            }
                        },
                    }
                ],
                indent=2,
            ),
            encoding="utf-8",
        )
        self.policy = self.root / "policy.json"
        self.write_policy()

    def write_policy(
        self,
        *,
        expected_license: str = "MIT OR Apache-2.0",
        source_offer: bool = True,
        supplements: list[dict[str, str]] | None = None,
        branch_files: dict[str, list[str]] | None = None,
    ) -> None:
        cargo_rule: dict[str, object] = {
            "expectedDeclaredLicense": expected_license,
            "branchFiles": branch_files
            or {
                "MIT": ["LICENSE-MIT"],
                "Apache-2.0": ["LICENSE-APACHE"],
            },
            "sourceOffer": source_offer,
        }
        if supplements is not None:
            cargo_rule["supplementalLegalFiles"] = supplements
        self.policy.write_text(
            json.dumps(
                {
                    "schemaVersion": MODULE.POLICY_SCHEMA_VERSION,
                    "target": MODULE.TARGET,
                    "componentRules": {
                        "cargo:dual-crate@1.0.0": cargo_rule
                    },
                },
                indent=2,
                sort_keys=True,
            )
            + "\n",
            encoding="utf-8",
        )

    def install_supplement(
        self,
        *,
        name: str = "UPSTREAM-BRANCH-B",
        relative: str = "dual-crate/LICENSE-APACHE",
    ) -> dict[str, str]:
        destination = self.upstream / Path(relative)
        destination.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(FIXTURES / "LICENSE-APACHE", destination)
        digest = hashlib.sha256(destination.read_bytes()).hexdigest()
        return {
            "name": name,
            "path": relative,
            "expectedSha256": digest,
            "kind": "license",
            "sourceUrl": (
                "https://raw.githubusercontent.com/example/dual-crate/"
                f"{self.source_commit}/LICENSE-APACHE"
            ),
            "sourceCommit": self.source_commit,
        }

    def common(self) -> dict[str, object]:
        return {
            "product_root": self.product,
            "metadata_root": self.metadata,
            "policy": self.policy,
            "cargo_metadata_json": self.cargo_metadata,
            "pnpm_list_json": self.pnpm_list,
        }

    def build_all(self) -> tuple[Path, Path, Path, Path]:
        source_zip = self.outputs / "skill-deck-mpl-sources.zip"
        source_receipt = self.outputs / "skill-deck-mpl-sources.json"
        MODULE.command_build_source(
            argparse.Namespace(
                **self.common(),
                zip_output=source_zip,
                receipt_output=source_receipt,
            )
        )
        notice = self.outputs / "THIRD_PARTY_NOTICES.txt"
        index = self.outputs / "THIRD_PARTY_NOTICES.index.json"
        MODULE.command_generate(
            argparse.Namespace(
                **self.common(),
                notice_output=notice,
                index_output=index,
                source_zip=source_zip,
                source_receipt=source_receipt,
            )
        )
        return notice, index, source_zip, source_receipt

    def verify_args(
        self, notice: Path, index: Path, source_zip: Path, source_receipt: Path
    ) -> argparse.Namespace:
        return argparse.Namespace(
            **self.common(),
            notice=notice,
            index=index,
            source_zip=source_zip,
            source_receipt=source_receipt,
        )


class LegalNoticeCorpusTests(unittest.TestCase):
    def test_scan_excludes_dev_edges_and_includes_every_license_branch(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            fixture = CorpusFixture(Path(temp))
            scan = MODULE._scan(
                fixture.product,
                fixture.metadata,
                fixture.policy,
                fixture.cargo_metadata,
                fixture.pnpm_list,
            )
            self.assertEqual(
                [item["id"] for item in scan["components"]],
                ["cargo:dual-crate@1.0.0", "npm:npm-shared@2.0.0"],
            )
            cargo = scan["components"][0]
            self.assertEqual(
                [item["fileName"] for item in cargo["legalSources"]],
                ["LICENSE-APACHE", "LICENSE-MIT", "NOTICE"],
            )
            self.assertEqual(scan["unresolvedDecisions"], [])

    def test_generation_is_reproducible_deduplicated_and_never_calls_tools(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            fixture = CorpusFixture(Path(temp))
            with mock.patch.object(
                MODULE.subprocess,
                "run",
                side_effect=AssertionError("network/tool execution forbidden"),
            ):
                notice, index, source_zip, source_receipt = fixture.build_all()
                first = (
                    notice.read_bytes(),
                    index.read_bytes(),
                    source_zip.read_bytes(),
                    source_receipt.read_bytes(),
                )
                fixture.build_all()
                second = (
                    notice.read_bytes(),
                    index.read_bytes(),
                    source_zip.read_bytes(),
                    source_receipt.read_bytes(),
                )
            self.assertEqual(first, second)
            parsed_index = json.loads(index.read_text(encoding="utf-8"))
            self.assertEqual(parsed_index["coverage"]["legalSourceCount"], 4)
            self.assertEqual(
                parsed_index["coverage"]["deduplicatedTextBlockCount"], 3
            )
            self.assertTrue(parsed_index["coverage"]["allComponentsCovered"])

    def test_normalization_strips_only_line_trailing_ascii_whitespace(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            fixture = CorpusFixture(Path(temp))
            legal_path = fixture.cargo_source / "COPYRIGHT-TRAILING"
            raw = b"Holder  \r\nLicense\t\r\nNo final newline \t"
            legal_path.write_bytes(raw)
            scan = MODULE._scan(
                fixture.product,
                fixture.metadata,
                fixture.policy,
                fixture.cargo_metadata,
                fixture.pnpm_list,
            )
            source = next(
                item
                for item in scan["components"][0]["legalSources"]
                if item["fileName"] == legal_path.name
            )
            self.assertEqual(source["rawSha256"], hashlib.sha256(raw).hexdigest())
            self.assertEqual(source["rawSizeBytes"], len(raw))
            self.assertEqual(
                source["_normalized"],
                b"Holder\nLicense\nNo final newline",
            )
            self.assertEqual(
                MODULE._normalized_text(b"Has final \t\r\n")[0],
                b"Has final\n",
            )

            notice, index, source_zip, source_receipt = fixture.build_all()
            first = (
                notice.read_bytes(),
                index.read_bytes(),
                source_zip.read_bytes(),
                source_receipt.read_bytes(),
            )
            fixture.build_all()
            self.assertEqual(
                first,
                (
                    notice.read_bytes(),
                    index.read_bytes(),
                    source_zip.read_bytes(),
                    source_receipt.read_bytes(),
                ),
            )
            notice_text = notice.read_text(encoding="utf-8")
            self.assertFalse(
                any(line.endswith((" ", "\t")) for line in notice_text.splitlines())
            )

            original_inventory = scan["inventorySha256"]
            legal_path.write_bytes(
                b"Holder \r\nLicense\t\r\nNo final newline\t"
            )
            changed_scan = MODULE._scan(
                fixture.product,
                fixture.metadata,
                fixture.policy,
                fixture.cargo_metadata,
                fixture.pnpm_list,
            )
            self.assertNotEqual(
                original_inventory, changed_scan["inventorySha256"]
            )
            with self.assertRaisesRegex(MODULE.NoticeCorpusError, "drift"):
                MODULE.command_verify(
                    fixture.verify_args(
                        notice, index, source_zip, source_receipt
                    )
                )

    def test_source_offer_zip_is_deterministic_exact_and_path_clean(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            fixture = CorpusFixture(Path(temp))
            notice, index, source_zip, source_receipt = fixture.build_all()
            receipt = json.loads(source_receipt.read_text(encoding="utf-8"))
            self.assertEqual(
                receipt["componentIds"], ["cargo:dual-crate@1.0.0"]
            )
            self.assertEqual(
                receipt["sha256"], MODULE._sha256_file(source_zip)
            )
            with zipfile.ZipFile(source_zip) as archive:
                names = archive.namelist()
                self.assertEqual(names, sorted(names))
                self.assertIn("MANIFEST.json", names)
                self.assertNotIn("npm-shared", "\n".join(names))
                for info in archive.infolist():
                    self.assertEqual(info.date_time, MODULE.FIXED_ZIP_TIME)
                    self.assertFalse(Path(info.filename).is_absolute())
                    self.assertNotIn("..", Path(info.filename).parts)
                    self.assertEqual((info.external_attr >> 16) & 0o777, 0o644)
            combined = notice.read_text("utf-8") + index.read_text("utf-8")
            self.assertNotIn(str(fixture.root), combined)
            self.assertNotRegex(combined, r"(?i)[a-z]:[\\/]")
            self.assertIn(source_zip.name, combined)
            self.assertIn(receipt["sha256"], combined)

    def test_verify_detects_notice_zip_and_source_text_tampering(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            fixture = CorpusFixture(Path(temp))
            notice, index, source_zip, source_receipt = fixture.build_all()
            args = fixture.verify_args(notice, index, source_zip, source_receipt)
            self.assertTrue(MODULE.command_verify(args)["verified"])

            original_notice = notice.read_bytes()
            notice.write_bytes(original_notice + b"tampered\n")
            with self.assertRaisesRegex(MODULE.NoticeCorpusError, "notice corpus drift"):
                MODULE.command_verify(args)
            notice.write_bytes(original_notice)

            original_zip = source_zip.read_bytes()
            source_zip.write_bytes(original_zip + b"tampered")
            with self.assertRaisesRegex(MODULE.NoticeCorpusError, "source ZIP drift"):
                MODULE.command_verify(args)
            source_zip.write_bytes(original_zip)

            (fixture.cargo_source / "LICENSE-MIT").write_text(
                "changed upstream license text\n", encoding="utf-8"
            )
            with self.assertRaisesRegex(MODULE.NoticeCorpusError, "drift"):
                MODULE.command_verify(args)

    def test_generate_rejects_forged_receipt_and_nonexistent_zip_before_writes(
        self,
    ) -> None:
        with tempfile.TemporaryDirectory() as temp:
            fixture = CorpusFixture(Path(temp))
            _notice, _index, source_zip, source_receipt = fixture.build_all()
            forged = json.loads(source_receipt.read_text(encoding="utf-8"))
            forged["sha256"] = "0" * 64
            source_receipt.write_text(
                json.dumps(forged, indent=2, sort_keys=True) + "\n",
                encoding="utf-8",
            )
            notice_output = fixture.outputs / "forged-notice.txt"
            index_output = fixture.outputs / "forged-index.json"
            args = argparse.Namespace(
                **fixture.common(),
                notice_output=notice_output,
                index_output=index_output,
                source_zip=source_zip,
                source_receipt=source_receipt,
            )
            with self.assertRaisesRegex(
                MODULE.NoticeCorpusError, "source receipt drift"
            ):
                MODULE.command_generate(args)
            self.assertFalse(notice_output.exists())
            self.assertFalse(index_output.exists())

            MODULE.command_build_source(
                argparse.Namespace(
                    **fixture.common(),
                    zip_output=source_zip,
                    receipt_output=source_receipt,
                )
            )
            args.source_zip = fixture.outputs / "does-not-exist.zip"
            with self.assertRaisesRegex(
                MODULE.NoticeCorpusError, "cannot read expected source ZIP"
            ):
                MODULE.command_generate(args)
            self.assertFalse(notice_output.exists())
            self.assertFalse(index_output.exists())

    def test_single_mpl_rejects_unrelated_legal_files(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            fixture = CorpusFixture(Path(temp))
            npm_package = json.loads(
                (fixture.npm_source / "package.json").read_text(encoding="utf-8")
            )
            npm_package["license"] = "MPL-2.0"
            (fixture.npm_source / "package.json").write_text(
                json.dumps(npm_package), encoding="utf-8"
            )
            policy = json.loads(fixture.policy.read_text(encoding="utf-8"))
            policy["componentRules"]["npm:npm-shared@2.0.0"] = {
                "expectedDeclaredLicense": "MPL-2.0",
                "branchFiles": {"MPL-2.0": ["LICENSE"]},
                "sourceOffer": True,
            }
            fixture.policy.write_text(json.dumps(policy), encoding="utf-8")
            with self.assertRaisesRegex(
                MODULE.NoticeCorpusError,
                "selected legal source is not bound to reciprocal branch",
            ):
                MODULE._scan(
                    fixture.product,
                    fixture.metadata,
                    fixture.policy,
                    fixture.cargo_metadata,
                    fixture.pnpm_list,
                )

    def test_mpl_rejects_false_source_offer(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            fixture = CorpusFixture(Path(temp))
            npm_package = json.loads(
                (fixture.npm_source / "package.json").read_text(encoding="utf-8")
            )
            npm_package["license"] = "MPL-2.0"
            (fixture.npm_source / "package.json").write_text(
                json.dumps(npm_package), encoding="utf-8"
            )
            policy = json.loads(fixture.policy.read_text(encoding="utf-8"))
            policy["componentRules"]["npm:npm-shared@2.0.0"] = {
                "expectedDeclaredLicense": "MPL-2.0",
                "branchFiles": {"MPL-2.0": ["LICENSE"]},
                "sourceOffer": False,
            }
            fixture.policy.write_text(json.dumps(policy), encoding="utf-8")
            with self.assertRaisesRegex(
                MODULE.NoticeCorpusError,
                "reciprocal-license component must set sourceOffer true",
            ):
                MODULE._scan(
                    fixture.product,
                    fixture.metadata,
                    fixture.policy,
                    fixture.cargo_metadata,
                    fixture.pnpm_list,
                )

    def test_policy_bound_canonical_mpl_supplement_generates_and_verifies_all(
        self,
    ) -> None:
        with tempfile.TemporaryDirectory() as temp:
            fixture = CorpusFixture(Path(temp))
            npm_package = json.loads(
                (fixture.npm_source / "package.json").read_text(encoding="utf-8")
            )
            npm_package["license"] = "MPL-2.0"
            (fixture.npm_source / "package.json").write_text(
                json.dumps(npm_package), encoding="utf-8"
            )
            relative = "npm-shared/MPL-2.0.txt"
            canonical = fixture.upstream / relative
            canonical.parent.mkdir(parents=True, exist_ok=True)
            canonical.write_text(
                "Mozilla Public License Version 2.0\n"
                "Canonical fixture for policy binding.\n",
                encoding="utf-8",
            )
            supplement = {
                "name": "CANONICAL-SPDX-TEXT",
                "path": relative,
                "expectedSha256": hashlib.sha256(
                    canonical.read_bytes()
                ).hexdigest(),
                "kind": "license",
                "sourceUrl": (
                    "https://raw.githubusercontent.com/example/npm-shared/"
                    f"{fixture.source_commit}/MPL-2.0.txt"
                ),
                "sourceCommit": fixture.source_commit,
            }
            policy = json.loads(fixture.policy.read_text(encoding="utf-8"))
            policy["componentRules"]["npm:npm-shared@2.0.0"] = {
                "expectedDeclaredLicense": "MPL-2.0",
                "supplementalLegalFiles": [supplement],
                "branchFiles": {
                    "MPL-2.0": ["CANONICAL-SPDX-TEXT"]
                },
                "sourceOffer": True,
            }
            fixture.policy.write_text(json.dumps(policy), encoding="utf-8")

            notice, index, source_zip, source_receipt = fixture.build_all()
            verified = MODULE.command_verify(
                fixture.verify_args(
                    notice, index, source_zip, source_receipt
                )
            )
            self.assertTrue(verified["verified"])
            self.assertEqual(
                verified["sourceZipSha256"], MODULE._sha256_file(source_zip)
            )
            notice_text = notice.read_text(encoding="utf-8")
            self.assertIn("CANONICAL-SPDX-TEXT", notice_text)
            self.assertIn("Mozilla Public License Version 2.0", notice_text)
            with zipfile.ZipFile(source_zip) as archive:
                manifest = json.loads(archive.read("MANIFEST.json"))
            self.assertEqual(
                manifest["components"][1]["id"],
                "npm:npm-shared@2.0.0",
            )

    def test_discovered_legal_file_reparse_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            fixture = CorpusFixture(Path(temp))
            original = MODULE._is_reparse_or_symlink
            guarded = fixture.cargo_source / "LICENSE-MIT"
            with mock.patch.object(
                MODULE,
                "_is_reparse_or_symlink",
                side_effect=lambda path: (
                    (
                        path.name == guarded.name
                        and path.parent.name == guarded.parent.name
                    )
                    or original(path)
                ),
            ):
                with self.assertRaisesRegex(
                    MODULE.NoticeCorpusError,
                    "discovered legal source is not a regular non-reparse file",
                ):
                    MODULE._scan(
                        fixture.product,
                        fixture.metadata,
                        fixture.policy,
                        fixture.cargo_metadata,
                        fixture.pnpm_list,
                    )

    def test_ordinary_legal_named_directory_is_not_a_legal_text_candidate(
        self,
    ) -> None:
        with tempfile.TemporaryDirectory() as temp:
            fixture = CorpusFixture(Path(temp))
            third_party = fixture.cargo_source / "third_party"
            third_party.mkdir()
            (third_party / "README").write_text(
                "ordinary vendored source directory\n", encoding="utf-8"
            )
            scan = MODULE._scan(
                fixture.product,
                fixture.metadata,
                fixture.policy,
                fixture.cargo_metadata,
                fixture.pnpm_list,
            )
            cargo_sources = scan["components"][0]["legalSources"]
            self.assertNotIn(
                "third_party",
                [source["fileName"] for source in cargo_sources],
            )

    def test_scan_reports_missing_license_and_generate_fails_closed(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            fixture = CorpusFixture(Path(temp))
            fixture.write_policy(source_offer=False)
            (fixture.npm_source / "LICENSE").unlink()
            scan = MODULE._scan(
                fixture.product,
                fixture.metadata,
                fixture.policy,
                fixture.cargo_metadata,
                fixture.pnpm_list,
            )
            self.assertEqual(
                [item["code"] for item in scan["unresolvedDecisions"]],
                ["missing-license-source"],
            )
            args = argparse.Namespace(
                **fixture.common(),
                notice_output=fixture.outputs / "notice.txt",
                index_output=fixture.outputs / "index.json",
                source_zip=None,
                source_receipt=None,
            )
            with self.assertRaisesRegex(
                MODULE.NoticeCorpusError, "unresolved legal decisions"
            ):
                MODULE.command_generate(args)

    def test_policy_license_mismatch_and_absent_component_rule_fail(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            fixture = CorpusFixture(Path(temp))
            fixture.write_policy(expected_license="MIT")
            with self.assertRaisesRegex(
                MODULE.NoticeCorpusError, "declared-license mismatch"
            ):
                MODULE._scan(
                    fixture.product,
                    fixture.metadata,
                    fixture.policy,
                    fixture.cargo_metadata,
                    fixture.pnpm_list,
                )
            policy = json.loads(fixture.policy.read_text(encoding="utf-8"))
            policy["componentRules"]["npm:absent@1.0.0"] = {
                "expectedDeclaredLicense": "MIT"
            }
            fixture.policy.write_text(json.dumps(policy), encoding="utf-8")
            with self.assertRaisesRegex(
                MODULE.NoticeCorpusError, "rules for absent components"
            ):
                MODULE._scan(
                    fixture.product,
                    fixture.metadata,
                    fixture.policy,
                    fixture.cargo_metadata,
                    fixture.pnpm_list,
                )

    def test_valid_supplement_maps_branch_and_renders_pinned_provenance(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            fixture = CorpusFixture(Path(temp))
            (fixture.cargo_source / "LICENSE-APACHE").unlink()
            supplement = fixture.install_supplement()
            fixture.write_policy(
                supplements=[supplement],
                branch_files={
                    "MIT": ["LICENSE-MIT"],
                    "Apache-2.0": ["UPSTREAM-BRANCH-B"],
                },
            )
            scan = MODULE._scan(
                fixture.product,
                fixture.metadata,
                fixture.policy,
                fixture.cargo_metadata,
                fixture.pnpm_list,
            )
            self.assertEqual(scan["unresolvedDecisions"], [])
            cargo_sources = scan["components"][0]["legalSources"]
            supplemented = next(
                item
                for item in cargo_sources
                if item["fileName"] == "UPSTREAM-BRANCH-B"
            )
            self.assertEqual(
                supplemented["provenance"],
                {
                    "sourceUrl": supplement["sourceUrl"],
                    "sourceCommit": fixture.source_commit,
                },
            )
            notice, index, _source_zip, _source_receipt = fixture.build_all()
            notice_text = notice.read_text(encoding="utf-8")
            parsed_index = json.loads(index.read_text(encoding="utf-8"))
            self.assertIn(supplement["sourceUrl"], notice_text)
            self.assertIn(fixture.source_commit, notice_text)
            self.assertIn(
                supplement["sourceUrl"], json.dumps(parsed_index, sort_keys=True)
            )
            self.assertNotIn(str(fixture.upstream), notice_text)
            self.assertNotIn(
                str(fixture.upstream), json.dumps(parsed_index, sort_keys=True)
            )

    def test_supplement_hash_mismatch_fails_closed(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            fixture = CorpusFixture(Path(temp))
            supplement = fixture.install_supplement()
            supplement["expectedSha256"] = "0" * 64
            fixture.write_policy(supplements=[supplement])
            with self.assertRaisesRegex(
                MODULE.NoticeCorpusError, "supplement SHA-256 mismatch"
            ):
                MODULE._scan(
                    fixture.product,
                    fixture.metadata,
                    fixture.policy,
                    fixture.cargo_metadata,
                    fixture.pnpm_list,
                )

    def test_supplement_rejects_traversal_and_absolute_paths(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            fixture = CorpusFixture(Path(temp))
            supplement = fixture.install_supplement()
            bad_paths = [
                "../outside/LICENSE",
                str((fixture.root / "outside" / "LICENSE").resolve()),
            ]
            for bad_path in bad_paths:
                with self.subTest(path=bad_path):
                    supplement["path"] = bad_path
                    fixture.write_policy(supplements=[supplement])
                    with self.assertRaisesRegex(
                        MODULE.NoticeCorpusError,
                        "supplement path must remain below",
                    ):
                        MODULE._scan(
                            fixture.product,
                            fixture.metadata,
                            fixture.policy,
                            fixture.cargo_metadata,
                            fixture.pnpm_list,
                        )

    def test_supplement_rejects_unpinned_url_and_commit_mismatch(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            fixture = CorpusFixture(Path(temp))
            supplement = fixture.install_supplement()
            invalid = [
                {
                    **supplement,
                    "sourceUrl": supplement["sourceUrl"].replace(
                        "https://", "http://", 1
                    ),
                },
                {
                    **supplement,
                    "sourceUrl": supplement["sourceUrl"].replace(
                        fixture.source_commit, "2" * 40
                    ),
                },
                {**supplement, "sourceCommit": "1" * 12},
            ]
            for item in invalid:
                with self.subTest(item=item):
                    fixture.write_policy(supplements=[item])
                    with self.assertRaisesRegex(
                        MODULE.NoticeCorpusError,
                        "sourceUrl is not HTTPS-pinned|full commit hash",
                    ):
                        MODULE._scan(
                            fixture.product,
                            fixture.metadata,
                            fixture.policy,
                            fixture.cargo_metadata,
                            fixture.pnpm_list,
                        )

    def test_supplement_rejects_symlink_when_platform_allows_it(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            fixture = CorpusFixture(Path(temp))
            supplement = fixture.install_supplement()
            linked_path = fixture.upstream / supplement["path"]
            fixture.write_policy(supplements=[supplement])
            patcher = None
            if hasattr(os, "symlink"):
                linked_path.unlink()
                external = fixture.root / "outside-license"
                shutil.copy2(FIXTURES / "LICENSE-APACHE", external)
                try:
                    linked_path.symlink_to(external)
                except OSError:
                    shutil.copy2(FIXTURES / "LICENSE-APACHE", linked_path)
                    original = MODULE._is_reparse_or_symlink
                    patcher = mock.patch.object(
                        MODULE,
                        "_is_reparse_or_symlink",
                        side_effect=lambda path: (
                            (
                                path.name == linked_path.name
                                and path.parent.name == linked_path.parent.name
                            )
                            or original(path)
                        ),
                    )
            else:
                original = MODULE._is_reparse_or_symlink
                patcher = mock.patch.object(
                    MODULE,
                    "_is_reparse_or_symlink",
                    side_effect=lambda path: (
                        (
                            path.name == linked_path.name
                            and path.parent.name == linked_path.parent.name
                        )
                        or original(path)
                    ),
                )
            context = patcher if patcher is not None else nullcontext()
            with context:
                with self.assertRaisesRegex(
                    MODULE.NoticeCorpusError, "link or reparse point"
                ):
                    MODULE._scan(
                        fixture.product,
                        fixture.metadata,
                        fixture.policy,
                        fixture.cargo_metadata,
                        fixture.pnpm_list,
                    )

    def test_supplement_names_are_case_unambiguous(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            fixture = CorpusFixture(Path(temp))
            first = fixture.install_supplement(name="Audited-License")
            second = {**first, "name": "audited-license"}
            fixture.write_policy(supplements=[first, second])
            with self.assertRaisesRegex(
                MODULE.NoticeCorpusError, "case-ambiguous supplement names"
            ):
                MODULE._scan(
                    fixture.product,
                    fixture.metadata,
                    fixture.policy,
                    fixture.cargo_metadata,
                    fixture.pnpm_list,
                )

    def test_default_metadata_commands_are_locked_offline_prod_and_targeted(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            fixture = CorpusFixture(Path(temp))
            responses = [
                json.loads(fixture.cargo_metadata.read_text(encoding="utf-8")),
                json.loads(fixture.pnpm_list.read_text(encoding="utf-8")),
            ]
            commands: list[list[str]] = []

            def fake_run_json(command: list[str], cwd: Path) -> object:
                commands.append(command)
                return responses.pop(0)

            with mock.patch.object(MODULE, "_run_json", side_effect=fake_run_json):
                MODULE._scan(
                    fixture.product,
                    fixture.metadata,
                    fixture.policy,
                    None,
                    None,
                )
            self.assertEqual(
                commands[0],
                [
                    "cargo",
                    "metadata",
                    "--locked",
                    "--offline",
                    "--format-version",
                    "1",
                    "--filter-platform",
                    "x86_64-pc-windows-msvc",
                ],
            )
            self.assertEqual(
                commands[1],
                ["pnpm", "list", "--prod", "--json", "--depth", "Infinity"],
            )


if __name__ == "__main__":
    unittest.main()
