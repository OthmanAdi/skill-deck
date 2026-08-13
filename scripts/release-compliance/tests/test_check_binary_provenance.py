from __future__ import annotations

import importlib.util
import json
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path


SCRIPT = Path(__file__).resolve().parents[1] / "check_binary_provenance.py"
SPEC = importlib.util.spec_from_file_location("binary_provenance", SCRIPT)
assert SPEC is not None and SPEC.loader is not None
MODULE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MODULE)


class BinaryProvenanceTests(unittest.TestCase):
    def test_long_cargo_and_dos_alias_paths_are_rejected(self) -> None:
        cargo = r"C:\Users\Fixture Account\.cargo"
        long_path = cargo + (
            r"\registry\src\index.crates.io-1949cf8c6b5b557f"
            r"\aws-lc-sys-0.41.0\aws-lc\crypto\source.c"
        )
        dos_alias = (
            r"C:\Users\FIXTUR~1\.cargo\registry\src"
            r"\INDEXC~1\AWS-LC~1.0\aws-lc\crypto\source.c"
        )
        findings = MODULE.scan_bytes(
            long_path.encode("ascii")
            + b"\x00"
            + dos_alias.encode("utf-16le"),
            project_roots=[
                r"C:\Users\Fixture Account\Documents\skill-deck-fixture"
            ],
            cargo_home=cargo,
            rustup_home=r"C:\Users\Fixture Account\.rustup",
            rustc_sysroot=(
                r"C:\Users\Fixture Account\.rustup\toolchains\stable"
            ),
        )
        self.assertGreater(findings[("cargo-home", "ansi")], 0)
        self.assertGreater(findings[("windows-user-root", "ansi")], 0)
        self.assertGreater(
            findings[("windows-user-root", "utf16le-or-be")],
            0,
        )

    def test_shifted_wide_alias_is_counted_once_as_ambiguous(self) -> None:
        cargo = r"C:\Users\builder\.cargo"
        source_path = cargo + r"\registry\src\crate\source.c"
        for prefix in (b"\x00", b"X\x00"):
            with self.subTest(prefix=prefix):
                findings = MODULE.scan_bytes(
                    prefix + source_path.encode("utf-16le"),
                    project_roots=[r"C:\Users\builder\project"],
                    cargo_home=cargo,
                    rustup_home=r"C:\Users\builder\.rustup",
                    rustc_sysroot=(
                        r"C:\Users\builder\.rustup\toolchains\stable"
                    ),
                )
                self.assertEqual(
                    findings[("cargo-home", "utf16le-or-be")],
                    1,
                )
                self.assertNotIn(("cargo-home", "utf16le"), findings)
                self.assertNotIn(("cargo-home", "utf16be"), findings)

    def test_real_wide_paths_keep_their_encoding_evidence(self) -> None:
        cargo = r"C:\Users\builder\.cargo"
        source_path = cargo + r"\registry\src\crate\source.c"
        utf16le = MODULE.scan_bytes(
            b"\xff\xfe" + source_path.encode("utf-16le"),
            project_roots=[r"C:\Users\builder\project"],
            cargo_home=cargo,
            rustup_home=r"C:\Users\builder\.rustup",
            rustc_sysroot=r"C:\Users\builder\.rustup\toolchains\stable",
        )
        utf16be = MODULE.scan_bytes(
            b"\xfe\xff" + source_path.encode("utf-16be"),
            project_roots=[r"C:\Users\builder\project"],
            cargo_home=cargo,
            rustup_home=r"C:\Users\builder\.rustup",
            rustc_sysroot=r"C:\Users\builder\.rustup\toolchains\stable",
        )
        self.assertGreater(utf16le[("cargo-home", "utf16le")], 0)
        self.assertGreater(utf16be[("cargo-home", "utf16be")], 0)
        self.assertNotIn(("cargo-home", "utf16le-or-be"), utf16le)
        self.assertNotIn(("cargo-home", "utf16le-or-be"), utf16be)

    def test_nul_terminated_real_wide_paths_keep_truthful_evidence(self) -> None:
        cargo = r"C:\Users\builder\.cargo"
        source_path = cargo + r"\registry\src\crate\source.c"
        fixtures = {
            "little-endian": (
                source_path.encode("utf-16le") + b"\x00\x00",
                "utf16le",
            ),
            "big-endian-overlap": (
                source_path.encode("utf-16be") + b"\x00\x00",
                "utf16le-or-be",
            ),
        }
        for label, (payload, expected_encoding) in fixtures.items():
            with self.subTest(label=label):
                findings = MODULE.scan_bytes(
                    payload,
                    project_roots=[r"C:\Users\builder\project"],
                    cargo_home=cargo,
                    rustup_home=r"C:\Users\builder\.rustup",
                    rustc_sysroot=(
                        r"C:\Users\builder\.rustup\toolchains\stable"
                    ),
                )
                self.assertEqual(
                    findings[("cargo-home", expected_encoding)],
                    1,
                )
                for encoding in (
                    "utf16le",
                    "utf16be",
                    "utf16le-or-be",
                ):
                    if encoding != expected_encoding:
                        self.assertNotIn(("cargo-home", encoding), findings)
                report = MODULE.build_report(
                    [("skill-deck.exe", payload)],
                    project_roots=[r"C:\Users\builder\project"],
                    cargo_home=cargo,
                    rustup_home=r"C:\Users\builder\.rustup",
                    rustc_sysroot=(
                        r"C:\Users\builder\.rustup\toolchains\stable"
                    ),
                )
                self.assertFalse(report["passed"])
                self.assertIn(
                    {
                        "category": "cargo-home",
                        "encoding": expected_encoding,
                        "count": 1,
                    },
                    report["findings"],
                )

    def test_detects_ansi_utf16le_and_utf16be_source_prefixes(self) -> None:
        project = r"C:\Users\Build User\skill deck"
        cargo = r"C:\Users\Build User\.cargo"
        rustup = r"D:\Rust Up"
        sysroot = r"D:\Rust Up\toolchains\stable"
        payload = (
            b"prefix "
            + (project + r"\src-tauri\src\main.rs").encode("ascii")
            + b"\xff\xfe"
            + (cargo + r"\registry\src\crate.rs").encode("utf-16le")
            + b"\xff\xfe"
            + (rustup + r"\settings.toml").encode("ascii")
            + b"\xff\xfe"
            + (sysroot + r"\lib\rustlib").encode("utf-16le")
            + b"\xfe\xff"
            + (project + r"\src-tauri\src\lib.rs").encode("utf-16be")
        )
        findings = MODULE.scan_bytes(
            payload,
            project_roots=[project],
            cargo_home=cargo,
            rustup_home=rustup,
            rustc_sysroot=sysroot,
        )
        self.assertGreater(findings[("project-root", "ansi")], 0)
        self.assertGreater(findings[("windows-user-root", "ansi")], 0)
        self.assertGreater(findings[("cargo-home", "utf16le")], 0)
        self.assertGreater(findings[("windows-user-root", "utf16le")], 0)
        self.assertGreater(findings[("rustup-home", "ansi")], 0)
        self.assertGreater(findings[("rustc-sysroot", "utf16le")], 0)
        self.assertGreater(findings[("project-root", "utf16be")], 0)
        self.assertGreater(findings[("windows-user-root", "utf16be")], 0)

    def test_detects_unix_runner_roots_in_all_encodings(self) -> None:
        payload = (
            b"/home/runner/work/skill-deck/src/main.rs\x00"
            + b"\xff\xfe"
            + "/Users/builder/project/source.rs".encode("utf-16le")
            + b"\xfe\xff"
            + "/home/ci/project/source.rs".encode("utf-16be")
        )
        findings = MODULE.scan_bytes(
            payload,
            project_roots=["/safe/project"],
            cargo_home="/safe/cargo",
            rustup_home="/safe/rustup",
            rustc_sysroot="/safe/sysroot",
        )
        self.assertGreater(findings[("unix-home-runner-root", "ansi")], 0)
        self.assertGreater(findings[("unix-home-runner-root", "utf16le")], 0)
        self.assertGreater(findings[("unix-home-runner-root", "utf16be")], 0)

    def test_bare_pdb_basename_in_executable_is_allowed(self) -> None:
        report = MODULE.build_report(
            [
                (
                    "skill-deck.exe",
                    b"RSDS\x00skill_deck.pdb\x00"
                    + "skill_deck.pdb".encode("utf-16le")
                    + "skill_deck.pdb".encode("utf-16be"),
                )
            ],
            project_roots=["/safe/project"],
            cargo_home="/safe/cargo",
            rustup_home="/safe/rustup",
            rustc_sysroot="/safe/sysroot",
        )
        self.assertTrue(report["passed"])
        self.assertEqual(report["findings"], [])

    def test_rooted_machine_pdb_path_in_executable_is_blocked(self) -> None:
        report = MODULE.build_report(
            [
                (
                    "skill-deck.exe",
                    rb"RSDS C:\Users\builder\project\target\skill_deck.pdb",
                )
            ],
            project_roots=[r"C:\Users\builder\project"],
            cargo_home=r"C:\Users\builder\.cargo",
            rustup_home=r"C:\Users\builder\.rustup",
            rustc_sysroot=r"C:\Users\builder\.rustup\toolchains\stable",
        )
        self.assertFalse(report["passed"])
        self.assertIn(
            "project-root",
            {item["category"] for item in report["findings"]},
        )
        self.assertNotIn(
            "pdb-reference",
            {item["category"] for item in report["findings"]},
        )

    def test_rejects_pdb_artifact_and_supports_multiple_artifacts(self) -> None:
        report = MODULE.build_report(
            [("skill-deck.exe", b"clean"), ("symbols.pdb", b"clean")],
            project_roots=["/safe/project"],
            cargo_home="/safe/cargo",
            rustup_home="/safe/rustup",
            rustc_sysroot="/safe/sysroot",
        )
        self.assertFalse(report["passed"])
        self.assertEqual(report["artifactCount"], 2)
        self.assertIn(
            {"category": "pdb-artifact", "encoding": "filename", "count": 1},
            report["findings"],
        )

    def test_distribution_tree_allows_clean_stage_and_rejects_nested_pdb(
        self,
    ) -> None:
        with tempfile.TemporaryDirectory() as temp:
            stage = Path(temp) / "stage"
            nested = stage / "licenses" / "nested"
            nested.mkdir(parents=True)
            (stage / "skill-deck.exe").write_bytes(b"clean executable")
            self.assertEqual(MODULE.count_distributed_pdbs([str(stage)]), 0)
            clean = MODULE.build_report(
                [],
                project_roots=["/safe/project"],
                cargo_home="/safe/cargo",
                rustup_home="/safe/rustup",
                rustc_sysroot="/safe/sysroot",
                pdb_distribution_count=0,
            )
            self.assertTrue(clean["passed"])
            clean_cli = subprocess.run(
                [
                    sys.executable,
                    "-B",
                    str(SCRIPT),
                    "--json",
                    "--reject-pdb-under",
                    str(stage),
                ],
                check=False,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                text=True,
            )
            self.assertEqual(clean_cli.returncode, 0)

            (nested / "symbols.PDB").write_bytes(b"symbols")
            pdb_count = MODULE.count_distributed_pdbs([str(stage)])
            self.assertEqual(pdb_count, 1)
            blocked = MODULE.build_report(
                [],
                project_roots=["/safe/project"],
                cargo_home="/safe/cargo",
                rustup_home="/safe/rustup",
                rustc_sysroot="/safe/sysroot",
                pdb_distribution_count=pdb_count,
            )
            blocked_cli = subprocess.run(
                [
                    sys.executable,
                    "-B",
                    str(SCRIPT),
                    "--json",
                    "--reject-pdb-under",
                    str(stage),
                ],
                check=False,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                text=True,
            )
        self.assertFalse(blocked["passed"])
        self.assertEqual(
            blocked["findings"],
            [{"category": "pdb-distribution", "encoding": "tree", "count": 1}],
        )
        self.assertEqual(blocked_cli.returncode, 1)
        self.assertNotIn(str(stage), blocked_cli.stdout)
        self.assertEqual(
            json.loads(blocked_cli.stdout)["findings"],
            [{"category": "pdb-distribution", "count": 1, "encoding": "tree"}],
        )
        with self.assertRaises(OSError):
            MODULE.count_distributed_pdbs([str(Path(temp) / "missing-stage")])

    def test_cli_emits_only_normalized_evidence_never_raw_paths(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            private_prefix = root / "A User" / "private-project"
            artifact = root / "candidate.exe"
            artifact.write_bytes(
                (str(private_prefix) + "/src/main.rs").encode("ascii")
            )
            result = subprocess.run(
                [
                    sys.executable,
                    "-B",
                    str(SCRIPT),
                    "--json",
                    "--project-root",
                    str(private_prefix),
                    str(artifact),
                ],
                check=False,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                text=True,
            )
        self.assertEqual(result.returncode, 1)
        report = json.loads(result.stdout)
        self.assertFalse(report["passed"])
        self.assertNotIn(str(private_prefix), result.stdout)
        self.assertNotIn(str(artifact), result.stdout)
        self.assertEqual(result.stderr, "")
        self.assertEqual(
            {item["category"] for item in report["findings"]},
            {"project-root", "windows-user-root"},
        )


if __name__ == "__main__":
    unittest.main()
