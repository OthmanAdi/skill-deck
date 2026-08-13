#!/usr/bin/env python3
"""Build and verify Skill Deck's historical deterministic legal corpus.

The tool is intentionally Python-stdlib-only.  Normal scans execute only local,
offline dependency metadata commands.  JSON metadata overrides exist so the
same code can be tested with tiny fixtures without invoking Cargo or pnpm.
"""

from __future__ import annotations

import argparse
import base64
import hashlib
import io
import json
import os
import re
import shutil
import stat
import subprocess
import sys
import tomllib
import unicodedata
import zipfile
from collections import defaultdict
from pathlib import Path
from typing import Any, Iterable, Mapping, Sequence
from urllib.parse import unquote, urlsplit


SCHEMA_VERSION = "skill-deck-legal-notice-corpus/v1"
POLICY_SCHEMA_VERSION = "skill-deck-legal-notice-policy/v1"
SOURCE_SCHEMA_VERSION = "skill-deck-legal-notice-source/v1"
TARGET = "x86_64-pc-windows-msvc"
FIXED_ZIP_TIME = (1980, 1, 1, 0, 0, 0)
LEGAL_STEMS = (
    ("THIRD_PARTY", "third-party"),
    ("THIRD-PARTY", "third-party"),
    ("THIRDPARTY", "third-party"),
    ("COPYRIGHT", "copyright"),
    ("UNLICENSE", "license"),
    ("LICENSE", "license"),
    ("LICENCE", "license"),
    ("COPYING", "license"),
    ("NOTICE", "notice"),
    ("OFL", "license"),
)
RULE_FIELDS = {
    "expectedDeclaredLicense",
    "additionalLegalFiles",
    "supplementalLegalFiles",
    "branchFiles",
    "sourceOffer",
}
SUPPLEMENT_FIELDS = {
    "name",
    "path",
    "expectedSha256",
    "kind",
    "sourceUrl",
    "sourceCommit",
}
SUPPLEMENT_KINDS = {
    "license",
    "copyright",
    "notice",
    "third-party",
    "ofl",
    "unlicense",
    "other",
}
RECIPROCAL_LICENSES = {"mpl-2.0"}


class NoticeCorpusError(RuntimeError):
    """A fail-closed corpus generation or verification error."""


def _json_bytes(value: object) -> bytes:
    return (json.dumps(value, indent=2, sort_keys=True) + "\n").encode("utf-8")


def _canonical_json_bytes(value: object) -> bytes:
    return json.dumps(
        value, ensure_ascii=False, separators=(",", ":"), sort_keys=True
    ).encode("utf-8")


def _sha256_bytes(value: bytes) -> str:
    return hashlib.sha256(value).hexdigest()


def _sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    try:
        with path.open("rb") as stream:
            for chunk in iter(lambda: stream.read(1024 * 1024), b""):
                digest.update(chunk)
    except OSError as exc:
        raise NoticeCorpusError(f"cannot hash required file: {path.name}: {exc}") from exc
    return digest.hexdigest()


def _read_json(path: Path) -> Any:
    try:
        return json.loads(path.read_text(encoding="utf-8"))
    except (OSError, UnicodeDecodeError, json.JSONDecodeError) as exc:
        raise NoticeCorpusError(f"cannot read JSON {path.name}: {exc}") from exc


def _write_bytes(path: Path, data: bytes) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(data)


def _run_json(command: Sequence[str], cwd: Path) -> Any:
    environment = dict(os.environ)
    environment["CARGO_NET_OFFLINE"] = "true"
    environment["PYTHONUTF8"] = "1"
    environment["PYTHONIOENCODING"] = "utf-8"
    platform_command = list(command)
    if os.name == "nt":
        executable = shutil.which(command[0])
        if executable and Path(executable).suffix.casefold() in {".cmd", ".bat"}:
            platform_command = [
                os.environ.get("COMSPEC", "cmd.exe"),
                "/d",
                "/s",
                "/c",
                subprocess.list2cmdline([executable, *command[1:]]),
            ]
    try:
        result = subprocess.run(
            platform_command,
            cwd=cwd,
            env=environment,
            check=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
        )
        raw = result.stdout.decode("utf-8", errors="strict")
        return json.loads(raw)
    except (OSError, UnicodeDecodeError, json.JSONDecodeError, subprocess.CalledProcessError) as exc:
        detail = ""
        if isinstance(exc, subprocess.CalledProcessError):
            detail = exc.stderr.decode("utf-8", errors="replace").strip()
        raise NoticeCorpusError(
            f"offline metadata command failed: {' '.join(command)}"
            + (f": {detail}" if detail else "")
        ) from exc


def _license_name(value: object) -> str:
    if isinstance(value, str) and value.strip():
        return value.strip()
    if isinstance(value, Mapping):
        candidate = value.get("type") or value.get("name")
        if isinstance(candidate, str) and candidate.strip():
            return candidate.strip()
    if isinstance(value, list):
        names = sorted({_license_name(item) for item in value})
        if names:
            return " OR ".join(names)
    raise NoticeCorpusError("component has no usable declared license metadata")


def _component_id(ecosystem: str, name: str, version: str) -> str:
    if not ecosystem or not name or not version or any(
        character in name + version for character in "\r\n\0"
    ):
        raise NoticeCorpusError("component identity is malformed")
    return f"{ecosystem}:{name}@{version}"


def _cargo_lock_checksums(root: Path) -> dict[tuple[str, str, str], str]:
    path = root / "src-tauri" / "Cargo.lock"
    try:
        parsed = tomllib.loads(path.read_text(encoding="utf-8"))
    except (OSError, UnicodeDecodeError, tomllib.TOMLDecodeError) as exc:
        raise NoticeCorpusError(f"cannot read src-tauri/Cargo.lock: {exc}") from exc
    result: dict[tuple[str, str, str], str] = {}
    for package in parsed.get("package", []):
        if not isinstance(package, Mapping):
            continue
        checksum = package.get("checksum")
        if isinstance(checksum, str):
            result[
                (
                    str(package.get("name", "")),
                    str(package.get("version", "")),
                    str(package.get("source", "")),
                )
            ] = checksum
    return result


def _cargo_components(root: Path, metadata: object) -> list[dict[str, object]]:
    if not isinstance(metadata, Mapping):
        raise NoticeCorpusError("Cargo metadata root is malformed")
    resolve = metadata.get("resolve")
    if not isinstance(resolve, Mapping) or not isinstance(resolve.get("root"), str):
        raise NoticeCorpusError("Cargo metadata has no resolved root")
    root_id = str(resolve["root"])
    packages = {
        str(package["id"]): package
        for package in metadata.get("packages", [])
        if isinstance(package, Mapping) and "id" in package
    }
    nodes = {
        str(node["id"]): node
        for node in resolve.get("nodes", [])
        if isinstance(node, Mapping) and "id" in node
    }
    if root_id not in packages:
        raise NoticeCorpusError("Cargo root package is absent from metadata")

    included = {root_id}
    stack = [root_id]
    while stack:
        current = stack.pop()
        node = nodes.get(current)
        if not isinstance(node, Mapping):
            raise NoticeCorpusError(f"resolved Cargo node is missing: {current}")
        for dependency in node.get("deps", []):
            if not isinstance(dependency, Mapping):
                continue
            kinds = dependency.get("dep_kinds") or []
            if kinds and all(
                isinstance(kind, Mapping) and kind.get("kind") == "dev"
                for kind in kinds
            ):
                continue
            package_id = str(dependency.get("pkg", ""))
            if package_id not in packages:
                raise NoticeCorpusError(
                    f"resolved Cargo dependency package is missing: {package_id}"
                )
            if package_id not in included:
                included.add(package_id)
                stack.append(package_id)

    checksums = _cargo_lock_checksums(root)
    components: list[dict[str, object]] = []
    seen: set[str] = set()
    for package_id in sorted(included):
        if package_id == root_id:
            continue
        package = packages[package_id]
        name = str(package.get("name", ""))
        version = str(package.get("version", ""))
        component_id = _component_id("cargo", name, version)
        if component_id in seen:
            raise NoticeCorpusError(
                f"ambiguous duplicate Cargo component identity: {component_id}"
            )
        seen.add(component_id)
        source = str(package.get("source") or "")
        declared_value = package.get("license")
        if not declared_value and package.get("license_file"):
            declared_value = f"LicenseRef-File-{Path(str(package['license_file'])).name}"
        manifest_path = package.get("manifest_path")
        if not isinstance(manifest_path, str):
            raise NoticeCorpusError(f"Cargo component has no manifest path: {component_id}")
        source_root = Path(manifest_path).resolve().parent
        if not source_root.is_dir():
            raise NoticeCorpusError(f"Cargo source is missing: {component_id}")
        components.append(
            {
                "id": component_id,
                "ecosystem": "cargo",
                "name": name,
                "version": version,
                "declaredLicense": _license_name(declared_value),
                "sourceChecksumSha256": checksums.get((name, version, source)),
                "sourceIntegrity": None,
                "_sourceRoot": source_root,
            }
        )
    return components


def _flatten_pnpm(
    node: Mapping[str, object],
    metadata_root: Path,
    observed: dict[tuple[str, str], dict[str, object]],
) -> None:
    dependencies = node.get("dependencies") or {}
    if not isinstance(dependencies, Mapping):
        return
    for fallback_name, raw_dependency in dependencies.items():
        if not isinstance(raw_dependency, Mapping):
            continue
        name = str(raw_dependency.get("from") or fallback_name)
        version = str(raw_dependency.get("version") or "")
        raw_path = raw_dependency.get("path")
        if not version or not isinstance(raw_path, str):
            raise NoticeCorpusError(f"pnpm dependency lacks version/path: {name}")
        raw_package_root = Path(raw_path)
        package_root = (
            raw_package_root
            if raw_package_root.is_absolute()
            else metadata_root / raw_package_root
        ).resolve()
        try:
            package_root.relative_to(metadata_root)
        except ValueError as exc:
            raise NoticeCorpusError(
                f"pnpm package source escapes metadata root: {name}@{version}"
            ) from exc
        package_json = _read_json(package_root / "package.json")
        if package_json.get("name") != name or package_json.get("version") != version:
            raise NoticeCorpusError(f"pnpm package identity mismatch: {name}@{version}")
        key = (name, version)
        entry = {
            "license": _license_name(package_json.get("license")),
            "sourceRoot": package_root,
        }
        if key in observed and (
            observed[key]["license"] != entry["license"]
            or observed[key]["sourceRoot"] != entry["sourceRoot"]
        ):
            raise NoticeCorpusError(f"ambiguous pnpm package identity: {name}@{version}")
        observed[key] = entry
        _flatten_pnpm(raw_dependency, metadata_root, observed)


def _pnpm_integrities(lock_path: Path) -> dict[tuple[str, str], dict[str, str]]:
    try:
        text = lock_path.read_text(encoding="utf-8")
        package_section = text.split("\npackages:\n", 1)[1].split(
            "\nsnapshots:\n", 1
        )[0]
    except (OSError, UnicodeDecodeError, IndexError) as exc:
        raise NoticeCorpusError("pnpm-lock.yaml lacks readable package metadata") from exc
    result: dict[tuple[str, str], dict[str, str]] = {}
    blocks = re.split(
        r"(?m)(?=^  (?:'[^']+'|\"[^\"]+\"|[^ \n][^:\n]*):\s*$)",
        package_section,
    )
    for block in blocks:
        lines = block.splitlines()
        match = re.match(
            r"^  (?P<key>'[^']+'|\"[^\"]+\"|[^ \n][^:\n]*):\s*$",
            lines[0] if lines else "",
        )
        if not match:
            continue
        key = match.group("key")
        if key[:1] in {"'", '"'}:
            key = key[1:-1]
        if "@" not in key:
            continue
        name, version_with_peers = key.rsplit("@", 1)
        version = version_with_peers.split("(", 1)[0]
        integrity = re.search(
            r"integrity:\s*(sha(?:256|384|512)-[A-Za-z0-9+/=]+)", block
        )
        if not integrity:
            continue
        algorithm, encoded = integrity.group(1).split("-", 1)
        try:
            raw_digest = base64.b64decode(encoded, validate=True)
        except ValueError as exc:
            raise NoticeCorpusError(f"invalid pnpm integrity: {name}@{version}") from exc
        result[(name, version)] = {
            "algorithm": algorithm.upper().replace("SHA", "SHA-"),
            "content": raw_digest.hex(),
            "sri": integrity.group(1),
        }
    return result


def _pnpm_components(
    product_root: Path, metadata_root: Path, listing: object
) -> list[dict[str, object]]:
    projected_package = _read_json(product_root / "package.json")
    metadata_package = _read_json(metadata_root / "package.json")
    projected_dependencies = projected_package.get("dependencies") or {}
    if projected_dependencies != (metadata_package.get("dependencies") or {}):
        raise NoticeCorpusError(
            "metadata root production dependency contract differs from product root"
        )
    projected_lock = product_root / "pnpm-lock.yaml"
    metadata_lock = metadata_root / "pnpm-lock.yaml"
    if _sha256_file(projected_lock) != _sha256_file(metadata_lock):
        raise NoticeCorpusError(
            "metadata root pnpm-lock.yaml differs from product root"
        )
    if not isinstance(listing, list) or len(listing) != 1:
        raise NoticeCorpusError("pnpm list must return exactly one workspace root")
    listed_root = listing[0]
    if not isinstance(listed_root, Mapping):
        raise NoticeCorpusError("pnpm list workspace root is malformed")
    direct = listed_root.get("dependencies") or {}
    if not isinstance(direct, Mapping) or set(direct) != set(projected_dependencies):
        raise NoticeCorpusError("pnpm production direct dependency closure is incomplete")
    observed: dict[tuple[str, str], dict[str, object]] = {}
    _flatten_pnpm(listed_root, metadata_root, observed)
    integrities = _pnpm_integrities(projected_lock)
    components: list[dict[str, object]] = []
    for name, version in sorted(observed):
        entry = observed[(name, version)]
        components.append(
            {
                "id": _component_id("npm", name, version),
                "ecosystem": "npm",
                "name": name,
                "version": version,
                "declaredLicense": entry["license"],
                "sourceChecksumSha256": None,
                "sourceIntegrity": integrities.get((name, version)),
                "_sourceRoot": entry["sourceRoot"],
            }
        )
    return components


def _validated_supplement(
    component_id: str, raw_supplement: object
) -> dict[str, str]:
    if not isinstance(raw_supplement, Mapping):
        raise NoticeCorpusError(
            f"policy supplementalLegalFiles entry is malformed: {component_id}"
        )
    if set(raw_supplement) != SUPPLEMENT_FIELDS:
        missing = sorted(SUPPLEMENT_FIELDS - set(raw_supplement))
        unknown = sorted(set(raw_supplement) - SUPPLEMENT_FIELDS)
        raise NoticeCorpusError(
            f"policy supplementalLegalFiles fields are invalid for {component_id}: "
            f"missing={missing}, unknown={unknown}"
        )
    if not all(
        isinstance(raw_supplement[field], str) and raw_supplement[field]
        for field in SUPPLEMENT_FIELDS
    ):
        raise NoticeCorpusError(
            f"policy supplementalLegalFiles values are malformed: {component_id}"
        )
    name = str(raw_supplement["name"])
    if (
        Path(name).is_absolute()
        or Path(name).name != name
        or name in {".", ".."}
        or any(character in name for character in "\r\n\0/\\")
    ):
        raise NoticeCorpusError(
            f"supplement name must be one legal-source filename: {component_id}"
        )
    expected_sha256 = str(raw_supplement["expectedSha256"]).casefold()
    if not re.fullmatch(r"[0-9a-f]{64}", expected_sha256):
        raise NoticeCorpusError(
            f"supplement expectedSha256 is malformed: {component_id}:{name}"
        )
    kind = str(raw_supplement["kind"])
    if kind not in SUPPLEMENT_KINDS:
        raise NoticeCorpusError(
            f"supplement kind is unsupported: {component_id}:{name}"
        )
    source_commit = str(raw_supplement["sourceCommit"]).casefold()
    if not re.fullmatch(r"(?:[0-9a-f]{40}|[0-9a-f]{64})", source_commit):
        raise NoticeCorpusError(
            f"supplement sourceCommit must be a full commit hash: "
            f"{component_id}:{name}"
        )
    source_url = str(raw_supplement["sourceUrl"])
    parsed_url = urlsplit(source_url)
    decoded_segments = [
        unquote(segment).casefold()
        for segment in parsed_url.path.split("/")
        if segment
    ]
    if (
        parsed_url.scheme.casefold() != "https"
        or not parsed_url.hostname
        or parsed_url.username is not None
        or parsed_url.password is not None
        or parsed_url.fragment
        or source_commit not in decoded_segments
    ):
        raise NoticeCorpusError(
            f"supplement sourceUrl is not HTTPS-pinned to sourceCommit: "
            f"{component_id}:{name}"
        )
    return {
        "name": name,
        "path": str(raw_supplement["path"]),
        "expectedSha256": expected_sha256,
        "kind": kind,
        "sourceUrl": source_url,
        "sourceCommit": source_commit,
    }


def _load_policy(path: Path) -> dict[str, object]:
    policy = _read_json(path)
    if not isinstance(policy, Mapping):
        raise NoticeCorpusError("policy root must be an object")
    allowed_top = {"schemaVersion", "target", "componentRules"}
    unknown = set(policy) - allowed_top
    if unknown:
        raise NoticeCorpusError(f"policy has unknown fields: {sorted(unknown)}")
    if policy.get("schemaVersion") != POLICY_SCHEMA_VERSION:
        raise NoticeCorpusError(
            f"policy schemaVersion must be {POLICY_SCHEMA_VERSION}"
        )
    if policy.get("target") != TARGET:
        raise NoticeCorpusError(f"policy target must be {TARGET}")
    rules = policy.get("componentRules")
    if not isinstance(rules, Mapping):
        raise NoticeCorpusError("policy componentRules must be an object")
    normalized_rules: dict[str, object] = {}
    for component_id, raw_rule in rules.items():
        if not isinstance(component_id, str) or not isinstance(raw_rule, Mapping):
            raise NoticeCorpusError("policy component rule is malformed")
        unknown_rule = set(raw_rule) - RULE_FIELDS
        if unknown_rule:
            raise NoticeCorpusError(
                f"policy rule {component_id} has unknown fields: {sorted(unknown_rule)}"
            )
        if "expectedDeclaredLicense" in raw_rule and not isinstance(
            raw_rule["expectedDeclaredLicense"], str
        ):
            raise NoticeCorpusError(
                f"policy expectedDeclaredLicense is malformed: {component_id}"
            )
        for field in ("additionalLegalFiles",):
            if field in raw_rule and (
                not isinstance(raw_rule[field], list)
                or not all(isinstance(item, str) and item for item in raw_rule[field])
            ):
                raise NoticeCorpusError(f"policy {field} is malformed: {component_id}")
        supplements = raw_rule.get("supplementalLegalFiles", [])
        if not isinstance(supplements, list):
            raise NoticeCorpusError(
                f"policy supplementalLegalFiles is malformed: {component_id}"
            )
        normalized_supplements = [
            _validated_supplement(component_id, supplement)
            for supplement in supplements
        ]
        supplement_names = [item["name"].casefold() for item in normalized_supplements]
        if len(supplement_names) != len(set(supplement_names)):
            raise NoticeCorpusError(
                f"policy has case-ambiguous supplement names: {component_id}"
            )
        branch_files = raw_rule.get("branchFiles", {})
        if not isinstance(branch_files, Mapping) or not all(
            isinstance(branch, str)
            and isinstance(files, list)
            and files
            and all(isinstance(item, str) and item for item in files)
            for branch, files in branch_files.items()
        ):
            raise NoticeCorpusError(f"policy branchFiles is malformed: {component_id}")
        if "sourceOffer" in raw_rule and not isinstance(raw_rule["sourceOffer"], bool):
            raise NoticeCorpusError(f"policy sourceOffer is malformed: {component_id}")
        normalized_rule = dict(raw_rule)
        if normalized_supplements or "supplementalLegalFiles" in raw_rule:
            normalized_rule["supplementalLegalFiles"] = normalized_supplements
        normalized_rules[component_id] = normalized_rule
    return {
        "schemaVersion": POLICY_SCHEMA_VERSION,
        "target": TARGET,
        "componentRules": normalized_rules,
    }


def _legal_kind(filename: str) -> str | None:
    upper = filename.upper()
    for stem, kind in LEGAL_STEMS:
        if upper in {stem, stem + "S"} or (
            upper.startswith(stem)
            and len(upper) > len(stem)
            and upper[len(stem)] in "-_. "
        ):
            return kind
    return None


def _safe_relative_file(root: Path, relative: str, component_id: str) -> Path:
    candidate_relative = Path(relative)
    if (
        candidate_relative.is_absolute()
        or ".." in candidate_relative.parts
        or len(candidate_relative.parts) != 1
    ):
        raise NoticeCorpusError(
            f"policy legal file must be a top-level filename: {component_id}"
        )
    candidate = root / candidate_relative
    if not candidate.is_file() or _is_reparse_or_symlink(candidate):
        raise NoticeCorpusError(
            f"policy legal file is missing or not regular: {component_id}:{relative}"
        )
    return candidate


def _normalized_text(raw: bytes) -> tuple[bytes | None, str | None]:
    try:
        decoded = raw.decode("utf-8-sig", errors="strict")
    except UnicodeDecodeError as exc:
        return None, f"not UTF-8: byte {exc.start}"
    if "\0" in decoded:
        return None, "contains NUL"
    decoded = unicodedata.normalize("NFC", decoded)
    decoded = decoded.replace("\r\n", "\n").replace("\r", "\n")
    decoded = "\n".join(line.rstrip(" \t") for line in decoded.split("\n"))
    return decoded.encode("utf-8"), None


def _license_branches(expression: str) -> list[str]:
    branches = [
        branch.strip().strip("() ")
        for branch in re.split(r"\s+OR\s+|\s*/\s*", expression, flags=re.IGNORECASE)
    ]
    return sorted({branch for branch in branches if branch})


def _branch_tokens(branch: str) -> set[str]:
    normalized = re.sub(r"[^a-z0-9]+", "", branch.casefold())
    tokens = {normalized}
    aliases = {
        "apache": "apache",
        "mit": "mit",
        "bsd": "bsd",
        "mpl": "mpl",
        "ofl": "ofl",
        "unlicense": "unlicense",
        "isc": "isc",
        "lgpl": "lgpl",
        "agpl": "agpl",
        "gpl": "gpl",
        "zlib": "zlib",
    }
    for needle, token in aliases.items():
        if needle in normalized:
            tokens.add(token)
    return {token for token in tokens if token}


def _is_reciprocal_branch(branch: str) -> bool:
    normalized = branch.strip().strip("() ").casefold()
    return normalized in RECIPROCAL_LICENSES


def _source_is_bound_to_branch(
    source: Mapping[str, object], branch: str
) -> bool:
    normalized = source.get("_normalized")
    if not isinstance(normalized, bytes):
        return False
    content = normalized.decode("utf-8").casefold()
    compact_name = re.sub(
        r"[^a-z0-9]+", "", str(source["fileName"]).casefold()
    )
    normalized_branch = branch.strip().strip("() ").casefold()
    if normalized_branch == "mpl-2.0":
        content_bound = (
            "mozilla public license" in content
            and re.search(r"(?:version\s*)?2(?:\.0)?", content) is not None
        )
        if "provenance" in source:
            return content_bound
        return content_bound or (
            "mpl" in compact_name
            and ("20" in compact_name or "2" in compact_name)
        )
    tokens = _branch_tokens(branch)
    return any(token in compact_name or token in content for token in tokens)


def _resolve_supplement_path(
    metadata_root: Path, relative: str, component_id: str, name: str
) -> Path:
    requested = Path(relative)
    if (
        requested.is_absolute()
        or requested.drive
        or not requested.parts
        or any(part in {"", ".", ".."} for part in requested.parts)
    ):
        raise NoticeCorpusError(
            f"supplement path must remain below release/legal-corpus/upstream: "
            f"{component_id}:{name}"
        )
    resolved_metadata_root = metadata_root.resolve()
    current = resolved_metadata_root
    supplement_root: Path | None = None
    all_parts = ("release", "legal-corpus", "upstream", *requested.parts)
    for index, part in enumerate(all_parts):
        if not current.is_dir():
            raise NoticeCorpusError(
                f"supplement path parent is missing: {component_id}:{name}"
            )
        try:
            matches = [
                child
                for child in current.iterdir()
                if child.name.casefold() == part.casefold()
            ]
        except OSError as exc:
            raise NoticeCorpusError(
                f"cannot inspect supplement path: {component_id}:{name}"
            ) from exc
        if not matches:
            raise NoticeCorpusError(
                f"supplement path is missing: {component_id}:{name}"
            )
        if len(matches) != 1:
            raise NoticeCorpusError(
                f"supplement path is case-ambiguous: {component_id}:{name}"
            )
        current = matches[0]
        if _is_reparse_or_symlink(current):
            raise NoticeCorpusError(
                f"supplement path contains a link or reparse point: "
                f"{component_id}:{name}"
            )
        if index == 2:
            supplement_root = current.resolve()
    assert supplement_root is not None
    candidate = current.resolve()
    try:
        candidate.relative_to(supplement_root)
    except ValueError as exc:
        raise NoticeCorpusError(
            f"supplement path escapes release/legal-corpus/upstream: "
            f"{component_id}:{name}"
        ) from exc
    if not candidate.is_file() or _is_reparse_or_symlink(candidate):
        raise NoticeCorpusError(
            f"supplement is not a regular file: {component_id}:{name}"
        )
    return candidate


def _discover_legal(
    component: dict[str, object],
    rule: Mapping[str, object],
    metadata_root: Path,
) -> tuple[list[dict[str, object]], list[dict[str, str]]]:
    root = component["_sourceRoot"]
    assert isinstance(root, Path)
    component_id = str(component["id"])
    candidates: dict[str, tuple[Path, str, dict[str, str] | None]] = {}
    casefold_names: dict[str, str] = {}
    try:
        children = sorted(root.iterdir(), key=lambda item: (item.name.casefold(), item.name))
    except OSError as exc:
        raise NoticeCorpusError(f"cannot inspect component source: {component_id}") from exc
    for child in children:
        kind = _legal_kind(child.name)
        if kind is None:
            continue
        if _is_reparse_or_symlink(child):
            raise NoticeCorpusError(
                f"discovered legal source is not a regular non-reparse file: "
                f"{component_id}:{child.name}"
            )
        if child.is_dir():
            continue
        if not child.is_file():
            raise NoticeCorpusError(
                f"discovered legal source is not a regular non-reparse file: "
                f"{component_id}:{child.name}"
            )
        folded = child.name.casefold()
        if folded in casefold_names:
            raise NoticeCorpusError(
                f"case-ambiguous legal source names: {component_id}"
            )
        casefold_names[folded] = child.name
        candidates[child.name] = (child, kind, None)
    for filename in rule.get("additionalLegalFiles", []):
        path = _safe_relative_file(root, str(filename), component_id)
        folded = path.name.casefold()
        prior = casefold_names.get(folded)
        if prior is not None and prior != path.name:
            raise NoticeCorpusError(
                f"case-ambiguous policy legal source: {component_id}:{filename}"
            )
        casefold_names[folded] = path.name
        candidates.setdefault(
            path.name, (path, _legal_kind(path.name) or "other", None)
        )
    for supplement in rule.get("supplementalLegalFiles", []):
        assert isinstance(supplement, Mapping)
        name = str(supplement["name"])
        folded = name.casefold()
        if folded in casefold_names:
            raise NoticeCorpusError(
                f"supplement name is case-ambiguous with another legal source: "
                f"{component_id}:{name}"
            )
        path = _resolve_supplement_path(
            metadata_root,
            str(supplement["path"]),
            component_id,
            name,
        )
        casefold_names[folded] = name
        candidates[name] = (
            path,
            str(supplement["kind"]),
            {
                "sourceUrl": str(supplement["sourceUrl"]),
                "sourceCommit": str(supplement["sourceCommit"]),
                "expectedSha256": str(supplement["expectedSha256"]),
            },
        )

    unresolved: list[dict[str, str]] = []
    sources: list[dict[str, object]] = []
    if not candidates:
        unresolved.append(
            {
                "componentId": component_id,
                "code": "missing-license-source",
                "detail": "no top-level license, copyright, notice, third-party, OFL, or UNLICENSE file",
            }
        )
    for filename, (path, kind, provenance) in sorted(
        candidates.items(), key=lambda item: (item[0].casefold(), item[0])
    ):
        raw = path.read_bytes()
        raw_sha256 = _sha256_bytes(raw)
        if provenance is not None and raw_sha256 != provenance["expectedSha256"]:
            raise NoticeCorpusError(
                f"supplement SHA-256 mismatch: {component_id}:{filename}"
            )
        normalized, decode_error = _normalized_text(raw)
        source: dict[str, object] = {
            "fileName": filename,
            "kind": kind,
            "rawSha256": raw_sha256,
            "rawSizeBytes": len(raw),
            "textSha256": _sha256_bytes(normalized) if normalized is not None else None,
            "_normalized": normalized,
        }
        if provenance is not None:
            if decode_error is not None:
                raise NoticeCorpusError(
                    f"supplement must be regular UTF-8 text: "
                    f"{component_id}:{filename}: {decode_error}"
                )
            source["provenance"] = {
                "sourceUrl": provenance["sourceUrl"],
                "sourceCommit": provenance["sourceCommit"],
            }
        sources.append(source)
        if decode_error is not None:
            unresolved.append(
                {
                    "componentId": component_id,
                    "code": "undecodable-legal-source",
                    "detail": f"{filename}: {decode_error}",
                }
            )

    branches = _license_branches(str(component["declaredLicense"]))
    branch_policy = rule.get("branchFiles", {})
    assert isinstance(branch_policy, Mapping)
    source_names = {str(source["fileName"]) for source in sources}
    folded_sources = {name.casefold(): name for name in source_names}
    if branch_policy:
        unknown_branches = set(branch_policy) - set(branches)
        if unknown_branches:
            raise NoticeCorpusError(
                f"policy branch does not match declared license for {component_id}: "
                f"{sorted(unknown_branches)}"
            )
        for branch, filenames in branch_policy.items():
            for filename in filenames:
                if str(filename).casefold() not in folded_sources:
                    raise NoticeCorpusError(
                        f"policy branch file is not a discovered legal source: "
                        f"{component_id}:{filename}"
                    )
    source_by_name = {
        str(source["fileName"]).casefold(): source for source in sources
    }
    reciprocal_branches = [
        branch for branch in branches if _is_reciprocal_branch(branch)
    ]
    if reciprocal_branches and rule.get("sourceOffer") is not True:
        raise NoticeCorpusError(
            f"reciprocal-license component must set sourceOffer true: "
            f"{component_id}"
        )
    for branch in reciprocal_branches:
        selected_names = branch_policy.get(branch)
        if not isinstance(selected_names, list) or not selected_names:
            raise NoticeCorpusError(
                f"reciprocal license requires an explicit branchFiles mapping: "
                f"{component_id}:{branch}"
            )
        selected_sources = [
            source_by_name[str(filename).casefold()]
            for filename in selected_names
        ]
        if not any(
            _source_is_bound_to_branch(source, branch)
            for source in selected_sources
        ):
            raise NoticeCorpusError(
                f"selected legal source is not bound to reciprocal branch: "
                f"{component_id}:{branch}"
            )
    if len(branches) > 1:
        for branch in branches:
            explicitly_mapped = branch in branch_policy
            tokens = _branch_tokens(branch)
            automatically_mapped = any(
                any(token in re.sub(r"[^a-z0-9]+", "", name.casefold()) for token in tokens)
                for name in source_names
            )
            if not explicitly_mapped and not automatically_mapped:
                unresolved.append(
                    {
                        "componentId": component_id,
                        "code": "unmapped-license-branch",
                        "detail": f"no exact source file mapping for declared branch {branch}",
                    }
                )
    return sources, unresolved


def _public_source(source: Mapping[str, object]) -> dict[str, object]:
    public = {
        key: source[key]
        for key in ("fileName", "kind", "rawSha256", "rawSizeBytes", "textSha256")
    }
    if "provenance" in source:
        public["provenance"] = source["provenance"]
    return public


def _scan(
    product_root: Path,
    metadata_root: Path,
    policy_path: Path,
    cargo_metadata_path: Path | None,
    pnpm_list_path: Path | None,
) -> dict[str, object]:
    product_root = product_root.resolve()
    metadata_root = metadata_root.resolve()
    if not product_root.is_dir() or not metadata_root.is_dir():
        raise NoticeCorpusError("product root and metadata root must exist")
    policy = _load_policy(policy_path)
    cargo_metadata = (
        _read_json(cargo_metadata_path)
        if cargo_metadata_path is not None
        else _run_json(
            [
                "cargo",
                "metadata",
                "--locked",
                "--offline",
                "--format-version",
                "1",
                "--filter-platform",
                TARGET,
            ],
            product_root / "src-tauri",
        )
    )
    pnpm_listing = (
        _read_json(pnpm_list_path)
        if pnpm_list_path is not None
        else _run_json(
            ["pnpm", "list", "--prod", "--json", "--depth", "Infinity"],
            metadata_root,
        )
    )
    components = [
        *_cargo_components(product_root, cargo_metadata),
        *_pnpm_components(product_root, metadata_root, pnpm_listing),
    ]
    components.sort(key=lambda item: str(item["id"]))
    ids = [str(component["id"]) for component in components]
    if len(ids) != len(set(ids)):
        raise NoticeCorpusError("duplicate cross-graph component identity")
    rules = policy["componentRules"]
    assert isinstance(rules, Mapping)
    unmatched_rules = sorted(set(rules) - set(ids))
    if unmatched_rules:
        raise NoticeCorpusError(
            f"policy contains rules for absent components: {unmatched_rules}"
        )

    unresolved: list[dict[str, str]] = []
    for component in components:
        component_id = str(component["id"])
        rule = rules.get(component_id, {})
        assert isinstance(rule, Mapping)
        expected = rule.get("expectedDeclaredLicense")
        if expected is not None and expected != component["declaredLicense"]:
            raise NoticeCorpusError(
                f"policy declared-license mismatch for {component_id}: "
                f"expected {expected!r}, observed {component['declaredLicense']!r}"
            )
        sources, component_unresolved = _discover_legal(
            component, rule, metadata_root
        )
        component["legalSources"] = sources
        component["sourceOffer"] = bool(rule.get("sourceOffer", False))
        unresolved.extend(component_unresolved)

    inventory_rows = []
    for component in components:
        inventory_rows.append(
            {
                "id": component["id"],
                "ecosystem": component["ecosystem"],
                "name": component["name"],
                "version": component["version"],
                "declaredLicense": component["declaredLicense"],
                "sourceChecksumSha256": component["sourceChecksumSha256"],
                "sourceIntegrity": component["sourceIntegrity"],
                "sourceOffer": component["sourceOffer"],
                "legalSources": [
                    _public_source(source)
                    for source in component["legalSources"]
                ],
            }
        )
    inventory_sha256 = _sha256_bytes(_canonical_json_bytes(inventory_rows))
    return {
        "schemaVersion": SCHEMA_VERSION,
        "target": TARGET,
        "inventorySha256": inventory_sha256,
        "components": components,
        "unresolvedDecisions": sorted(
            unresolved,
            key=lambda item: (item["componentId"], item["code"], item["detail"]),
        ),
        "inputs": {
            "cargoLockSha256": _sha256_file(
                product_root / "src-tauri" / "Cargo.lock"
            ),
            "pnpmLockSha256": _sha256_file(product_root / "pnpm-lock.yaml"),
            "policySha256": _sha256_file(policy_path),
        },
    }


def _public_scan(scan: Mapping[str, object]) -> dict[str, object]:
    return {
        "schemaVersion": scan["schemaVersion"],
        "target": scan["target"],
        "inventorySha256": scan["inventorySha256"],
        "inputs": scan["inputs"],
        "components": [
            {
                "id": component["id"],
                "ecosystem": component["ecosystem"],
                "name": component["name"],
                "version": component["version"],
                "declaredLicense": component["declaredLicense"],
                "sourceChecksumSha256": component["sourceChecksumSha256"],
                "sourceIntegrity": component["sourceIntegrity"],
                "sourceOffer": component["sourceOffer"],
                "legalSources": [
                    _public_source(source)
                    for source in component["legalSources"]
                ],
            }
            for component in scan["components"]
        ],
        "unresolvedDecisions": scan["unresolvedDecisions"],
    }


def _assert_no_local_paths(data: bytes, roots: Iterable[Path]) -> None:
    text = data.decode("utf-8", errors="strict")
    normalized = text.replace("\\", "/").casefold()
    for root in roots:
        root_text = str(root.resolve()).replace("\\", "/").casefold()
        if root_text and root_text in normalized:
            raise NoticeCorpusError("generated output leaks an input root path")
    if re.search(
        r"(?i)(?:(?<![a-z0-9+.-])[a-z]:[\\/]|\\\\\?\\|file://)",
        text,
    ):
        raise NoticeCorpusError("generated output contains a local path")


def _render_notice(
    scan: Mapping[str, object],
    source_receipt: Mapping[str, object] | None,
) -> tuple[bytes, dict[str, object]]:
    if scan["unresolvedDecisions"]:
        raise NoticeCorpusError(
            "unresolved legal decisions prevent generation: "
            f"{len(scan['unresolvedDecisions'])}"
        )
    output = bytearray()
    component_index: list[dict[str, object]] = []
    text_index: list[dict[str, object]] = []

    def append(value: str | bytes) -> None:
        output.extend(value.encode("utf-8") if isinstance(value, str) else value)

    header = {
        "schemaVersion": SCHEMA_VERSION,
        "target": scan["target"],
        "inventorySha256": scan["inventorySha256"],
        "cargoLockSha256": scan["inputs"]["cargoLockSha256"],
        "pnpmLockSha256": scan["inputs"]["pnpmLockSha256"],
        "componentCount": len(scan["components"]),
        "sourceArtifact": (
            {
                "fileName": source_receipt["fileName"],
                "sha256": source_receipt["sha256"],
            }
            if source_receipt is not None
            else None
        ),
    }
    append("SKILL DECK FREE THIRD-PARTY NOTICE CORPUS\n")
    append("HEADER " + json.dumps(header, ensure_ascii=False, sort_keys=True) + "\n\n")

    text_components: dict[str, set[str]] = defaultdict(set)
    text_sources: dict[str, set[str]] = defaultdict(set)
    text_payloads: dict[str, bytes] = {}
    for component in scan["components"]:
        source_records = []
        text_ids = []
        for source in component["legalSources"]:
            text_id = str(source["textSha256"])
            normalized = source["_normalized"]
            assert isinstance(normalized, bytes)
            text_payloads[text_id] = normalized
            text_components[text_id].add(str(component["id"]))
            text_sources[text_id].add(str(source["fileName"]))
            text_ids.append(text_id)
            source_records.append(_public_source(source))
        record = {
            "id": component["id"],
            "ecosystem": component["ecosystem"],
            "name": component["name"],
            "version": component["version"],
            "declaredLicense": component["declaredLicense"],
            "sourceChecksumSha256": component["sourceChecksumSha256"],
            "sourceIntegrity": component["sourceIntegrity"],
            "sourceOffer": component["sourceOffer"],
            "legalSources": source_records,
            "textCoverage": sorted(set(text_ids)),
        }
        start = len(output)
        encoded = (
            "COMPONENT "
            + json.dumps(record, ensure_ascii=False, sort_keys=True)
            + "\n"
        ).encode("utf-8")
        append(encoded)
        end = len(output)
        component_index.append(
            {
                "id": component["id"],
                "byteOffset": start,
                "byteLength": end - start,
                "recordSha256": _sha256_bytes(encoded),
                "textCoverage": sorted(set(text_ids)),
                "legalSources": source_records,
            }
        )
    append("\n")

    for text_id in sorted(text_payloads):
        payload = text_payloads[text_id]
        metadata = {
            "textSha256": text_id,
            "components": sorted(text_components[text_id]),
            "sourceFileNames": sorted(text_sources[text_id]),
        }
        start = len(output)
        append("TEXT " + json.dumps(metadata, ensure_ascii=False, sort_keys=True) + "\n")
        append(payload)
        if not payload.endswith(b"\n"):
            append(b"\n")
        append("END-TEXT\n")
        end = len(output)
        text_index.append(
            {
                **metadata,
                "byteOffset": start,
                "byteLength": end - start,
                "blockSha256": _sha256_bytes(bytes(output[start:end])),
            }
        )

    notice = bytes(output)
    index = {
        "schemaVersion": SCHEMA_VERSION,
        "target": scan["target"],
        "inventorySha256": scan["inventorySha256"],
        "noticeSha256": _sha256_bytes(notice),
        "noticeSizeBytes": len(notice),
        "sourceArtifact": (
            {
                "fileName": source_receipt["fileName"],
                "sha256": source_receipt["sha256"],
            }
            if source_receipt is not None
            else None
        ),
        "components": component_index,
        "textBlocks": text_index,
        "coverage": {
            "componentCount": len(component_index),
            "componentIds": [item["id"] for item in component_index],
            "legalSourceCount": sum(
                len(item["legalSources"]) for item in component_index
            ),
            "deduplicatedTextBlockCount": len(text_index),
            "allComponentsCovered": all(item["textCoverage"] for item in component_index),
        },
        "unresolvedDecisions": scan["unresolvedDecisions"],
    }
    return notice, index


def _is_reparse_or_symlink(path: Path) -> bool:
    if path.is_symlink():
        return True
    try:
        attributes = getattr(path.lstat(), "st_file_attributes", 0)
    except OSError as exc:
        raise NoticeCorpusError(f"cannot inspect source entry: {path.name}") from exc
    return bool(attributes & getattr(stat, "FILE_ATTRIBUTE_REPARSE_POINT", 0x400))


def _source_prefix(component: Mapping[str, object]) -> str:
    safe_name = re.sub(r"[^A-Za-z0-9._-]+", "_", str(component["name"])).strip("._")
    safe_version = re.sub(
        r"[^A-Za-z0-9._+-]+", "_", str(component["version"])
    ).strip("._")
    if not safe_name or not safe_version:
        raise NoticeCorpusError(f"unsafe source archive identity: {component['id']}")
    return f"sources/{component['ecosystem']}/{safe_name}/{safe_version}"


def _source_archive(scan: Mapping[str, object], filename: str) -> tuple[bytes, dict[str, object]]:
    if scan["unresolvedDecisions"]:
        raise NoticeCorpusError("unresolved legal decisions prevent source archive")
    selected = [item for item in scan["components"] if item["sourceOffer"]]
    manifest_components: list[dict[str, object]] = []
    archive_files: dict[str, bytes] = {}
    for component in selected:
        root = component["_sourceRoot"]
        assert isinstance(root, Path)
        prefix = _source_prefix(component)
        file_receipts: list[dict[str, object]] = []
        try:
            entries = sorted(
                root.rglob("*"),
                key=lambda item: item.relative_to(root).as_posix(),
            )
        except OSError as exc:
            raise NoticeCorpusError(f"cannot enumerate source: {component['id']}") from exc
        for path in entries:
            if _is_reparse_or_symlink(path):
                raise NoticeCorpusError(
                    f"source archive refuses link/reparse entry: {component['id']}"
                )
            if not path.is_file():
                continue
            relative = path.relative_to(root).as_posix()
            if relative.startswith("/") or ".." in Path(relative).parts:
                raise NoticeCorpusError(f"unsafe source archive path: {component['id']}")
            data = path.read_bytes()
            archive_name = f"{prefix}/{relative}"
            if archive_name in archive_files:
                raise NoticeCorpusError(f"duplicate source archive path: {archive_name}")
            archive_files[archive_name] = data
            file_receipts.append(
                {
                    "path": relative,
                    "sizeBytes": len(data),
                    "sha256": _sha256_bytes(data),
                }
            )
        if not file_receipts:
            raise NoticeCorpusError(f"source-offer component has no source: {component['id']}")
        manifest_components.append(
            {
                "id": component["id"],
                "declaredLicense": component["declaredLicense"],
                "sourceChecksumSha256": component["sourceChecksumSha256"],
                "sourceIntegrity": component["sourceIntegrity"],
                "archivePrefix": prefix,
                "files": file_receipts,
            }
        )
    manifest = {
        "schemaVersion": SOURCE_SCHEMA_VERSION,
        "inventorySha256": scan["inventorySha256"],
        "components": manifest_components,
    }
    archive_files["MANIFEST.json"] = _json_bytes(manifest)
    buffer = io.BytesIO()
    with zipfile.ZipFile(buffer, mode="w", compression=zipfile.ZIP_STORED) as archive:
        for archive_name, data in sorted(archive_files.items()):
            info = zipfile.ZipInfo(archive_name, date_time=FIXED_ZIP_TIME)
            info.create_system = 3
            info.compress_type = zipfile.ZIP_STORED
            info.external_attr = (stat.S_IFREG | 0o644) << 16
            archive.writestr(info, data)
    zip_bytes = buffer.getvalue()
    receipt = {
        "schemaVersion": SOURCE_SCHEMA_VERSION,
        "fileName": Path(filename).name,
        "sha256": _sha256_bytes(zip_bytes),
        "sizeBytes": len(zip_bytes),
        "inventorySha256": scan["inventorySha256"],
        "componentIds": [item["id"] for item in selected],
    }
    return zip_bytes, receipt


def _common_scan(args: argparse.Namespace) -> dict[str, object]:
    return _scan(
        args.product_root,
        args.metadata_root,
        args.policy,
        args.cargo_metadata_json,
        args.pnpm_list_json,
    )


def command_scan(args: argparse.Namespace) -> dict[str, object]:
    scan = _common_scan(args)
    rendered = _json_bytes(_public_scan(scan))
    _assert_no_local_paths(rendered, (args.product_root, args.metadata_root))
    _write_bytes(args.output, rendered)
    return {
        "output": args.output.name,
        "componentCount": len(scan["components"]),
        "unresolvedDecisionCount": len(scan["unresolvedDecisions"]),
        "inventorySha256": scan["inventorySha256"],
        "sha256": _sha256_bytes(rendered),
    }


def command_generate(args: argparse.Namespace) -> dict[str, object]:
    scan = _common_scan(args)
    source_offer = any(component["sourceOffer"] for component in scan["components"])
    supplied_source_args = (
        args.source_zip is not None,
        args.source_receipt is not None,
    )
    if supplied_source_args[0] != supplied_source_args[1]:
        raise NoticeCorpusError(
            "--source-zip and --source-receipt must be supplied together"
        )
    if source_offer and supplied_source_args != (True, True):
        raise NoticeCorpusError(
            "source-offer policy requires both --source-zip and --source-receipt"
        )
    paths = [args.notice_output.resolve(), args.index_output.resolve()]
    if args.source_zip is not None and args.source_receipt is not None:
        paths.extend(
            [args.source_zip.resolve(), args.source_receipt.resolve()]
        )
    if len(paths) != len(set(paths)):
        raise NoticeCorpusError("generate input and output paths must be distinct")

    receipt: dict[str, object] | None = None
    if args.source_zip is not None and args.source_receipt is not None:
        zip_bytes, receipt = _source_archive(scan, args.source_zip.name)
        receipt_bytes = _json_bytes(receipt)
        _compare_file(args.source_zip, zip_bytes, "source ZIP")
        _compare_file(args.source_receipt, receipt_bytes, "source receipt")
    notice, index = _render_notice(scan, receipt)
    index_bytes = _json_bytes(index)
    _assert_no_local_paths(notice, (args.product_root, args.metadata_root))
    _assert_no_local_paths(index_bytes, (args.product_root, args.metadata_root))
    _write_bytes(args.notice_output, notice)
    _write_bytes(args.index_output, index_bytes)
    return {
        "noticeOutput": args.notice_output.name,
        "indexOutput": args.index_output.name,
        "componentCount": len(scan["components"]),
        "textBlockCount": len(index["textBlocks"]),
        "inventorySha256": scan["inventorySha256"],
        "noticeSha256": _sha256_bytes(notice),
        "indexSha256": _sha256_bytes(index_bytes),
        "sourceZipSha256": receipt["sha256"] if receipt is not None else None,
    }


def command_build_source(args: argparse.Namespace) -> dict[str, object]:
    scan = _common_scan(args)
    zip_bytes, receipt = _source_archive(scan, args.zip_output.name)
    receipt_bytes = _json_bytes(receipt)
    _assert_no_local_paths(receipt_bytes, (args.product_root, args.metadata_root))
    _write_bytes(args.zip_output, zip_bytes)
    _write_bytes(args.receipt_output, receipt_bytes)
    return receipt


def _compare_file(path: Path, expected: bytes, label: str) -> None:
    try:
        actual = path.read_bytes()
    except OSError as exc:
        raise NoticeCorpusError(f"cannot read expected {label}: {path.name}") from exc
    if actual != expected:
        raise NoticeCorpusError(
            f"{label} drift: expected {_sha256_bytes(expected)}, "
            f"observed {_sha256_bytes(actual)}"
        )


def command_verify(args: argparse.Namespace) -> dict[str, object]:
    scan = _common_scan(args)
    source_offer = any(component["sourceOffer"] for component in scan["components"])
    supplied_source_args = (args.source_zip is not None, args.source_receipt is not None)
    if source_offer and supplied_source_args != (True, True):
        raise NoticeCorpusError(
            "source-offer policy requires both --source-zip and --source-receipt"
        )
    if supplied_source_args[0] != supplied_source_args[1]:
        raise NoticeCorpusError(
            "--source-zip and --source-receipt must be supplied together"
        )
    receipt: dict[str, object] | None = None
    if args.source_zip is not None and args.source_receipt is not None:
        zip_bytes, receipt = _source_archive(scan, args.source_zip.name)
        _compare_file(args.source_zip, zip_bytes, "source ZIP")
        _compare_file(args.source_receipt, _json_bytes(receipt), "source receipt")
    notice, index = _render_notice(scan, receipt)
    index_bytes = _json_bytes(index)
    _assert_no_local_paths(notice, (args.product_root, args.metadata_root))
    _assert_no_local_paths(index_bytes, (args.product_root, args.metadata_root))
    _compare_file(args.notice, notice, "notice corpus")
    _compare_file(args.index, index_bytes, "notice index")
    return {
        "verified": True,
        "componentCount": len(scan["components"]),
        "inventorySha256": scan["inventorySha256"],
        "noticeSha256": _sha256_bytes(notice),
        "indexSha256": _sha256_bytes(index_bytes),
        "sourceZipSha256": receipt["sha256"] if receipt is not None else None,
    }


def _add_common(parser: argparse.ArgumentParser) -> None:
    parser.add_argument("--product-root", type=Path, required=True)
    parser.add_argument("--metadata-root", type=Path, required=True)
    parser.add_argument("--policy", type=Path, required=True)
    parser.add_argument(
        "--cargo-metadata-json",
        type=Path,
        help="fixture/offline override; otherwise cargo metadata is executed",
    )
    parser.add_argument(
        "--pnpm-list-json",
        type=Path,
        help="fixture/offline override; otherwise pnpm list --prod is executed",
    )


def parse_args(argv: Sequence[str]) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    subparsers = parser.add_subparsers(dest="command", required=True)
    scan = subparsers.add_parser("scan", help="scan the release dependency graph")
    _add_common(scan)
    scan.add_argument("--output", type=Path, required=True)
    scan.set_defaults(handler=command_scan)

    generate = subparsers.add_parser("generate", help="generate notice and index")
    _add_common(generate)
    generate.add_argument("--notice-output", type=Path, required=True)
    generate.add_argument("--index-output", type=Path, required=True)
    generate.add_argument("--source-zip", type=Path)
    generate.add_argument("--source-receipt", type=Path)
    generate.set_defaults(handler=command_generate)

    build_source = subparsers.add_parser(
        "build-source", help="build deterministic reciprocal-license source ZIP"
    )
    _add_common(build_source)
    build_source.add_argument("--zip-output", type=Path, required=True)
    build_source.add_argument("--receipt-output", type=Path, required=True)
    build_source.set_defaults(handler=command_build_source)

    verify = subparsers.add_parser("verify", help="regenerate and byte-verify outputs")
    _add_common(verify)
    verify.add_argument("--notice", type=Path, required=True)
    verify.add_argument("--index", type=Path, required=True)
    verify.add_argument("--source-zip", type=Path)
    verify.add_argument("--source-receipt", type=Path)
    verify.set_defaults(handler=command_verify)
    return parser.parse_args(argv)


def main(argv: Sequence[str] | None = None) -> int:
    args = parse_args(sys.argv[1:] if argv is None else argv)
    try:
        summary = args.handler(args)
    except (NoticeCorpusError, OSError, ValueError, KeyError) as exc:
        print(f"legal-notice-corpus: ERROR: {exc}", file=sys.stderr)
        return 2
    print(json.dumps(summary, indent=2, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
