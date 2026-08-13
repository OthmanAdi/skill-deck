#!/usr/bin/env python3
"""Fail closed when release binaries disclose build-machine provenance."""

from __future__ import annotations

import argparse
import json
import os
import re
import subprocess
import sys
from collections import Counter
from pathlib import Path
from typing import Iterable, Sequence


SCHEMA = "skill-deck-binary-provenance/v1"
ANSI_STRINGS = re.compile(rb"[\x20-\x7e]{4,}")
UTF16LE_STRINGS = re.compile(rb"(?:[\x20-\x7e]\x00){4,}")
UTF16BE_STRINGS = re.compile(rb"(?:\x00[\x20-\x7e]){4,}")
ABSOLUTE_PATH_HINT = re.compile(r"(?i)(?:[a-z]:[\\/]|/[^/\s]+/)")
GENERIC_PATTERNS = (
    (
        "windows-user-root",
        re.compile(r"(?i)[a-z]:[\\/]+users[\\/]+[^\\/\s\"']+"),
    ),
    (
        "unix-home-runner-root",
        re.compile(
            r"(?i)(?:/(?:home|users)/[^/\s\"']+|"
            r"/(?:runner|opt/hostedtoolcache)(?:/|$))"
        ),
    ),
)


def _slash_variants(value: str) -> tuple[str, ...]:
    variants = {os.path.normpath(value)}
    if re.match(r"(?i)^[a-z]:[\\/]", value) or "\\" in value:
        variants.add(value.replace("\\", "/"))
        variants.add(value.replace("/", "\\"))
    return tuple(sorted(variants))


def _strings(data: bytes) -> Iterable[tuple[str, str]]:
    for match in ANSI_STRINGS.finditer(data):
        yield "ansi", match.group().decode("ascii")

    def wide_records(
        pattern: re.Pattern[bytes],
        encoding: str,
        bom: bytes,
    ) -> list[dict[str, object]]:
        records: list[dict[str, object]] = []
        for match in pattern.finditer(data):
            text = match.group().decode(encoding)
            records.append(
                {
                    "start": match.start(),
                    "end": match.end(),
                    "text": text,
                    "masked": [False] * len(text),
                    "authoritative": (
                        match.start() >= len(bom)
                        and data[match.start() - len(bom) : match.start()] == bom
                    ),
                }
            )
        return records

    utf16le = wide_records(UTF16LE_STRINGS, "utf-16le", b"\xff\xfe")
    utf16be = wide_records(UTF16BE_STRINGS, "utf-16be", b"\xfe\xff")
    ambiguous: list[str] = []

    be_cursor = 0
    for le_record in utf16le:
        le_start = int(le_record["start"])
        le_end = int(le_record["end"])
        le_text = str(le_record["text"])
        while (
            be_cursor < len(utf16be)
            and int(utf16be[be_cursor]["end"]) < le_start - 1
        ):
            be_cursor += 1
        candidate = be_cursor
        while (
            candidate < len(utf16be)
            and int(utf16be[candidate]["start"]) <= le_end + 1
        ):
            be_record = utf16be[candidate]
            be_start = int(be_record["start"])
            be_text = str(be_record["text"])
            alignment = be_start + 1 - le_start
            if alignment % 2 == 0:
                le_offset = alignment // 2
                be_overlap_start = max(0, -le_offset)
                be_overlap_end = min(
                    len(be_text),
                    len(le_text) - le_offset,
                )
                le_overlap_start = be_overlap_start + le_offset
                le_overlap_end = be_overlap_end + le_offset
                common = be_text[be_overlap_start:be_overlap_end]
                if (
                    len(common) >= 4
                    and le_text[le_overlap_start:le_overlap_end] == common
                    and ABSOLUTE_PATH_HINT.search(common)
                ):
                    le_mask = le_record["masked"]
                    be_mask = be_record["masked"]
                    assert isinstance(le_mask, list)
                    assert isinstance(be_mask, list)
                    if bool(le_record["authoritative"]) and not bool(
                        be_record["authoritative"]
                    ):
                        be_mask[be_overlap_start:be_overlap_end] = [True] * len(
                            common
                        )
                    elif bool(be_record["authoritative"]) and not bool(
                        le_record["authoritative"]
                    ):
                        le_mask[le_overlap_start:le_overlap_end] = [True] * len(
                            common
                        )
                    else:
                        le_mask[le_overlap_start:le_overlap_end] = [True] * len(
                            common
                        )
                        be_mask[be_overlap_start:be_overlap_end] = [True] * len(
                            common
                        )
                        ambiguous.append(common)
            candidate += 1

    def unmasked_segments(record: dict[str, object]) -> Iterable[str]:
        text = str(record["text"])
        mask = record["masked"]
        assert isinstance(mask, list)
        start: int | None = None
        for index, hidden in enumerate([*mask, True]):
            if not hidden and start is None:
                start = index
            elif hidden and start is not None:
                if index - start >= 4:
                    yield text[start:index]
                start = None

    for record in utf16le:
        for text in unmasked_segments(record):
            yield "utf16le", text
    for record in utf16be:
        for text in unmasked_segments(record):
            yield "utf16be", text
    for text in ambiguous:
        yield "utf16le-or-be", text


def _exact_roots(
    *,
    project_roots: Sequence[str],
    cargo_home: str | None,
    rustup_home: str | None,
    rustc_sysroot: str | None,
) -> dict[str, tuple[str, ...]]:
    home = Path.home()
    categories: dict[str, set[str]] = {
        "project-root": set(),
        "cargo-home": set(),
        "rustup-home": set(),
        "rustc-sysroot": set(),
    }
    for root in project_roots:
        for candidate in (root, os.path.abspath(root)):
            categories["project-root"].update(_slash_variants(candidate))
    resolved_cargo = cargo_home or os.environ.get("CARGO_HOME") or str(home / ".cargo")
    resolved_rustup = (
        rustup_home or os.environ.get("RUSTUP_HOME") or str(home / ".rustup")
    )
    for candidate in (resolved_cargo, os.path.abspath(resolved_cargo)):
        categories["cargo-home"].update(_slash_variants(candidate))
    for candidate in (resolved_rustup, os.path.abspath(resolved_rustup)):
        categories["rustup-home"].update(_slash_variants(candidate))
    if rustc_sysroot:
        for candidate in (rustc_sysroot, os.path.abspath(rustc_sysroot)):
            categories["rustc-sysroot"].update(_slash_variants(candidate))
    return {
        category: tuple(
            sorted({value.casefold(): value for value in values}.values())
        )
        for category, values in categories.items()
        if values
    }


def discover_rustc_sysroot() -> str | None:
    try:
        result = subprocess.run(
            ["rustc", "--print", "sysroot"],
            check=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.DEVNULL,
            text=True,
        )
    except (FileNotFoundError, subprocess.CalledProcessError):
        return None
    value = result.stdout.strip()
    return value or None


def scan_bytes(
    data: bytes,
    *,
    project_roots: Sequence[str] = (),
    cargo_home: str | None = None,
    rustup_home: str | None = None,
    rustc_sysroot: str | None = None,
) -> Counter[tuple[str, str]]:
    exact = _exact_roots(
        project_roots=project_roots,
        cargo_home=cargo_home,
        rustup_home=rustup_home,
        rustc_sysroot=rustc_sysroot,
    )
    findings: Counter[tuple[str, str]] = Counter()
    for encoding, text in _strings(data):
        folded = text.casefold()
        for category, roots in exact.items():
            count = sum(folded.count(root.casefold()) for root in roots)
            if count:
                findings[(category, encoding)] += count
        for category, pattern in GENERIC_PATTERNS:
            count = sum(1 for _match in pattern.finditer(text))
            if count:
                findings[(category, encoding)] += count
    return findings


def build_report(
    artifact_data: Sequence[tuple[str, bytes]],
    *,
    project_roots: Sequence[str] = (),
    cargo_home: str | None = None,
    rustup_home: str | None = None,
    rustc_sysroot: str | None = None,
    pdb_distribution_count: int = 0,
) -> dict[str, object]:
    findings: Counter[tuple[str, str]] = Counter()
    for artifact_name, data in artifact_data:
        findings.update(
            scan_bytes(
                data,
                project_roots=project_roots,
                cargo_home=cargo_home,
                rustup_home=rustup_home,
                rustc_sysroot=rustc_sysroot,
            )
        )
        if artifact_name.casefold().endswith(".pdb"):
            findings[("pdb-artifact", "filename")] += 1
    if pdb_distribution_count:
        findings[("pdb-distribution", "tree")] += pdb_distribution_count
    normalized = [
        {"category": category, "encoding": encoding, "count": count}
        for (category, encoding), count in sorted(findings.items())
    ]
    return {
        "schema": SCHEMA,
        "passed": not normalized,
        "artifactCount": len(artifact_data),
        "byteCount": sum(len(data) for _name, data in artifact_data),
        "findings": normalized,
    }


def count_distributed_pdbs(roots: Sequence[str]) -> int:
    def raise_walk_error(error: OSError) -> None:
        raise error

    count = 0
    seen: set[str] = set()
    for raw_root in roots:
        root = Path(raw_root)
        identity = os.path.abspath(root).casefold()
        if identity in seen:
            continue
        seen.add(identity)
        if not root.is_dir():
            raise OSError("distribution root is unavailable")
        for _current, _directories, files in os.walk(
            root,
            followlinks=False,
            onerror=raise_walk_error,
        ):
            count += sum(name.casefold().endswith(".pdb") for name in files)
    return count


class SafeArgumentParser(argparse.ArgumentParser):
    def error(self, _message: str) -> None:
        raise ValueError("invalid scanner arguments")


def parse_args(argv: Sequence[str] | None = None) -> argparse.Namespace:
    parser = SafeArgumentParser(add_help=True)
    parser.add_argument("--json", action="store_true")
    parser.add_argument("--project-root", action="append", default=[])
    parser.add_argument("--cargo-home")
    parser.add_argument("--rustup-home")
    parser.add_argument("--rustc-sysroot")
    parser.add_argument("--reject-pdb-under", action="append", default=[])
    parser.add_argument("artifacts", nargs="*")
    return parser.parse_args(argv)


def _operational_report(category: str) -> dict[str, object]:
    return {
        "schema": SCHEMA,
        "passed": False,
        "artifactCount": 0,
        "byteCount": 0,
        "findings": [{"category": category, "encoding": "none", "count": 1}],
    }


def main(argv: Sequence[str] | None = None) -> int:
    try:
        args = parse_args(argv)
    except SystemExit as exc:
        return int(exc.code or 0)
    except ValueError:
        print(json.dumps(_operational_report("invalid-arguments"), sort_keys=True))
        return 2
    if not args.artifacts and not args.reject_pdb_under:
        print(json.dumps(_operational_report("invalid-arguments"), sort_keys=True))
        return 2
    artifact_data: list[tuple[str, bytes]] = []
    try:
        for raw_path in args.artifacts:
            path = Path(raw_path)
            artifact_data.append((path.name, path.read_bytes()))
    except OSError:
        print(json.dumps(_operational_report("artifact-read-error"), sort_keys=True))
        return 2
    try:
        pdb_distribution_count = count_distributed_pdbs(args.reject_pdb_under)
    except OSError:
        print(
            json.dumps(
                _operational_report("distribution-tree-read-error"),
                sort_keys=True,
            )
        )
        return 2
    report = build_report(
        artifact_data,
        project_roots=args.project_root or [str(Path.cwd())],
        cargo_home=args.cargo_home,
        rustup_home=args.rustup_home,
        rustc_sysroot=args.rustc_sysroot or discover_rustc_sysroot(),
        pdb_distribution_count=pdb_distribution_count,
    )
    print(json.dumps(report, indent=2 if args.json else None, sort_keys=True))
    return 0 if report["passed"] else 1


if __name__ == "__main__":
    sys.exit(main())
