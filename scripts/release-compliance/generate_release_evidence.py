#!/usr/bin/env python3
"""Generate deterministic compliance evidence for the single Skill Deck product."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import shutil
import subprocess
import sys
from pathlib import Path
from typing import Any, Iterable, Mapping, Sequence
from urllib.parse import quote


TOOL_VERSION = "2.0.0"
TARGET = "x86_64-pc-windows-msvc"
CONTRACT_PATH = Path("release/compliance-contract.json")
PRODUCT_PATH = Path("product.json")
NOTICE_PATH = Path("src-tauri/resources/THIRD_PARTY_NOTICES.txt")
FORBIDDEN_DISTRIBUTED_TOOLS = {"harness-cli.exe", "license-keygen.exe"}
PRODUCT_LICENSE = "MPL-2.0"
PRODUCT_AUTHOR = "Ahmad Othman Ammar Adi"
PRODUCT_COPYRIGHT = f"Copyright (c) 2026 {PRODUCT_AUTHOR}"


class ComplianceError(RuntimeError):
    """A fail-closed release-evidence error."""


def _json_bytes(value: object) -> bytes:
    return (json.dumps(value, indent=2, sort_keys=True) + "\n").encode("utf-8")


def _read_json(path: Path) -> Any:
    try:
        return json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as exc:
        raise ComplianceError(f"cannot read JSON {path}: {exc}") from exc


def _sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def _receipt(path: Path, label: str) -> dict[str, object]:
    if not path.is_file():
        raise ComplianceError(f"required release input is missing: {label}")
    return {
        "path": label.replace("\\", "/"),
        "sizeBytes": path.stat().st_size,
        "sha256": _sha256_file(path),
    }


def _run(command: Sequence[str], cwd: Path) -> str:
    environment = dict(os.environ)
    environment.update(PYTHONUTF8="1", PYTHONIOENCODING="utf-8")
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
        completed = subprocess.run(
            platform_command,
            cwd=cwd,
            env=environment,
            check=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
        )
    except (OSError, subprocess.CalledProcessError) as exc:
        detail = ""
        if isinstance(exc, subprocess.CalledProcessError):
            detail = exc.stderr.decode("utf-8", errors="replace").strip()
        raise ComplianceError(
            f"command failed: {' '.join(command)}" + (f"\n{detail}" if detail else "")
        ) from exc
    return completed.stdout.decode("utf-8", errors="strict").strip()


def _run_json(command: Sequence[str], cwd: Path) -> Any:
    try:
        return json.loads(_run(command, cwd))
    except json.JSONDecodeError as exc:
        raise ComplianceError(f"command did not return JSON: {' '.join(command)}") from exc


def _purl(kind: str, name: str, version: str) -> str:
    if kind == "npm":
        encoded = quote(name, safe="/").replace("@", "%40")
    else:
        encoded = quote(name, safe=".-_+~")
    return f"pkg:{kind}/{encoded}@{quote(version, safe='.-_+~')}"


def _license(value: object) -> str:
    if isinstance(value, str) and value.strip():
        return value.strip()
    return "NOASSERTION"


def _cargo_components(root: Path) -> tuple[list[dict[str, object]], dict[str, list[str]]]:
    metadata = _run_json(
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
        root / "src-tauri",
    )
    packages = {item["id"]: item for item in metadata.get("packages", [])}
    nodes = {item["id"]: item for item in (metadata.get("resolve") or {}).get("nodes", [])}
    root_id = (metadata.get("resolve") or {}).get("root")
    if not root_id or root_id not in packages:
        raise ComplianceError("Cargo metadata has no resolvable root package")
    reachable: set[str] = set()
    stack = [root_id]
    while stack:
        package_id = stack.pop()
        if package_id in reachable:
            continue
        reachable.add(package_id)
        stack.extend(dep["pkg"] for dep in nodes.get(package_id, {}).get("deps", []))
    components: list[dict[str, object]] = []
    refs: dict[str, str] = {}
    for package_id in sorted(reachable):
        package = packages[package_id]
        name, version = str(package["name"]), str(package["version"])
        bom_ref = _purl("cargo", name, version)
        refs[package_id] = bom_ref
        components.append(
            {
                "type": "application" if package_id == root_id else "library",
                "bom-ref": bom_ref,
                "name": name,
                "version": version,
                "purl": bom_ref,
                "licenses": [{"license": {"name": _license(package.get("license"))}}],
            }
        )
    dependencies = {
        refs[package_id]: sorted(
            refs[dep["pkg"]]
            for dep in nodes.get(package_id, {}).get("deps", [])
            if dep["pkg"] in refs
        )
        for package_id in sorted(reachable)
    }
    return components, dependencies


def _walk_pnpm(node: Mapping[str, object]) -> Iterable[tuple[str, Mapping[str, object]]]:
    dependencies = node.get("dependencies") or {}
    if not isinstance(dependencies, Mapping):
        return
    for name, child in sorted(dependencies.items()):
        if isinstance(child, Mapping):
            yield str(name), child
            yield from _walk_pnpm(child)


def _npm_components(root: Path) -> list[dict[str, object]]:
    listing = _run_json(["pnpm", "list", "--prod", "--json", "--depth", "Infinity"], root)
    if not isinstance(listing, list) or not listing or not isinstance(listing[0], Mapping):
        raise ComplianceError("pnpm production listing is malformed")
    observed: dict[str, dict[str, object]] = {}
    for name, node in _walk_pnpm(listing[0]):
        version = str(node.get("version") or "")
        if not version:
            raise ComplianceError(f"pnpm component has no version: {name}")
        bom_ref = _purl("npm", name, version)
        observed[bom_ref] = {
            "type": "library",
            "bom-ref": bom_ref,
            "name": name,
            "version": version,
            "purl": bom_ref,
            "licenses": [{"license": {"name": _license(node.get("license"))}}],
        }
    return [observed[key] for key in sorted(observed)]


def _resource_receipts(root: Path, contract: Mapping[str, object]) -> list[dict[str, object]]:
    resources: list[dict[str, object]] = []
    runtime = contract["llamaRuntime"]
    runtime_root = root / "src-tauri" / "resources" / "llama"
    expected = set(runtime["files"])
    actual = {item.name for item in runtime_root.iterdir() if item.is_file()}
    if actual != expected:
        raise ComplianceError(
            f"llama.cpp runtime set differs from the contract: missing={sorted(expected-actual)}, extra={sorted(actual-expected)}"
        )
    for name in sorted(expected):
        resources.append(_receipt(runtime_root / name, f"src-tauri/resources/llama/{name}"))
    notice_contract = contract["noticeResources"]
    notice = root / notice_contract["notice"]
    notice_text = notice.read_text(encoding="utf-8")
    resources.append(_receipt(notice, str(notice_contract["notice"])))
    for entry in notice_contract["licenses"]:
        path = root / entry["path"]
        content = path.read_text(encoding="utf-8")
        for fragment in entry["requiredFragments"]:
            if fragment not in content:
                raise ComplianceError(f"required attribution fragment is absent from {entry['path']}")
        resources.append(_receipt(path, str(entry["path"])))
    if "llama.cpp" not in notice_text or "Jan-nano" not in notice_text:
        raise ComplianceError("third-party notices omit llama.cpp or Jan-nano attribution")
    return resources


def _artifacts(package_root: Path, allow_missing: bool) -> list[dict[str, object]]:
    release_root = package_root / "target" / "release"
    candidates: list[tuple[str, Path]] = []
    raw = release_root / "skill-deck.exe"
    if raw.is_file():
        candidates.append(("portable-executable", raw))
    for role, folder, pattern in (
        ("installer-msi", release_root / "bundle" / "msi", "*.msi"),
        ("installer-nsis", release_root / "bundle" / "nsis", "*.exe"),
    ):
        if folder.is_dir():
            candidates.extend((role, item) for item in sorted(folder.glob(pattern)))
    forbidden = sorted(
        str(path.relative_to(package_root)).replace("\\", "/")
        for path in package_root.rglob("*.exe")
        if path.name.casefold() in FORBIDDEN_DISTRIBUTED_TOOLS
    )
    if forbidden:
        raise ComplianceError(f"internal developer tools entered the package tree: {forbidden}")
    if not candidates and not allow_missing:
        raise ComplianceError("no Windows release artifacts were found")
    return [
        {
            "role": role,
            **_receipt(path, str(path.relative_to(package_root)).replace("\\", "/")),
        }
        for role, path in candidates
    ]


def _git_provenance(root: Path) -> dict[str, object]:
    commit = _run(["git", "rev-parse", "HEAD"], root)
    if len(commit) != 40 or any(character not in "0123456789abcdef" for character in commit):
        raise ComplianceError("Git commit is not a full lowercase SHA")
    return {
        "repository": "https://github.com/OthmanAdi/skill-deck",
        "commit": commit,
        "dirty": bool(_run(["git", "status", "--porcelain", "--untracked-files=all"], root)),
    }


def generate(args: argparse.Namespace) -> dict[str, object]:
    root = args.root.resolve()
    package_root = args.package_root.resolve()
    output = args.output.resolve()
    if output.exists():
        raise ComplianceError("output path already exists")
    product = _read_json(root / PRODUCT_PATH)
    contract = _read_json(root / CONTRACT_PATH)
    package = _read_json(root / "package.json")
    if product.get("distributionModel") != "single-unlocked-product":
        raise ComplianceError("product contract is not a single unlocked product")
    if contract.get("product", {}).get("distributionModel") != "single-unlocked-product":
        raise ComplianceError("compliance contract is not a single unlocked product")
    if package.get("license") != PRODUCT_LICENSE or package.get("author") != PRODUCT_AUTHOR:
        raise ComplianceError("package license or author is not synchronized")
    for label, metadata in (
        ("product contract", product),
        ("compliance contract", contract.get("product", {})),
    ):
        if (
            metadata.get("license") != PRODUCT_LICENSE
            or metadata.get("copyrightHolder") != PRODUCT_AUTHOR
            or metadata.get("publisher") != PRODUCT_AUTHOR
        ):
            raise ComplianceError(f"{label} license or publisher identity is not synchronized")
    version = str(package.get("version") or "")
    if version != product.get("version") or version != contract.get("productVersion"):
        raise ComplianceError("product version is not synchronized")

    cargo, cargo_edges = _cargo_components(root)
    npm = _npm_components(root)
    resources = _resource_receipts(root, contract)
    artifacts = _artifacts(package_root, args.allow_missing_artifacts)
    model = contract["model"]
    model_ref = _purl("generic", str(model["name"]), str(model["revision"]))
    model_component = {
        "type": "machine-learning-model",
        "bom-ref": model_ref,
        "name": model["name"],
        "version": model["revision"],
        "purl": model_ref,
        "licenses": [{"license": {"name": model["license"]}}],
        "hashes": [{"alg": "SHA-256", "content": model["sha256"]}],
    }
    components = sorted([*cargo, *npm, model_component], key=lambda item: str(item["bom-ref"]))
    bom = {
        "bomFormat": "CycloneDX",
        "specVersion": contract["cycloneDxSpecVersion"],
        "version": 1,
        "metadata": {
            "component": {
                "type": "application",
                "name": "Skill Deck",
                "version": version,
                "author": PRODUCT_AUTHOR,
                "copyright": PRODUCT_COPYRIGHT,
                "licenses": [{"license": {"id": PRODUCT_LICENSE}}],
            }
        },
        "components": components,
        "dependencies": [
            {"ref": key, "dependsOn": value} for key, value in sorted(cargo_edges.items())
        ],
    }
    inventory = [
        {
            "bomRef": item["bom-ref"],
            "declaredLicense": item["licenses"][0]["license"]["name"],
        }
        for item in components
    ]
    provenance = _git_provenance(root)
    evidence = {
        "schemaVersion": "skill-deck-release-evidence/v2",
        "toolVersion": TOOL_VERSION,
        "productId": "skill-deck",
        "productVersion": version,
        "distributionModel": "single-unlocked-product",
        "target": contract["target"],
        "source": provenance,
        "componentCount": len(components),
        "resourceCount": len(resources),
        "artifacts": artifacts,
        "notices": [_receipt(root / "LICENSE", "LICENSE"), _receipt(root / NOTICE_PATH, str(NOTICE_PATH))],
        "residualLicenseReview": {
            "status": "manual-review-required-before-distribution",
            "tokens": contract.get("manualLicenseReviewTokens", []),
        },
    }
    files = {
        f"skill-deck-{version}.cdx.json": _json_bytes(bom),
        "dependency-license-inventory.json": _json_bytes({"components": inventory}),
        "runtime-resource-receipts.json": _json_bytes({"files": resources}),
        "artifact-receipts.json": _json_bytes({"artifacts": artifacts}),
        "release-evidence.json": _json_bytes(evidence),
    }
    sums = [f"{hashlib.sha256(data).hexdigest()}  {name}" for name, data in sorted(files.items())]
    sums.extend(f"{item['sha256']}  external/{Path(str(item['path'])).name}" for item in artifacts)
    files["SHA256SUMS.txt"] = ("\n".join(sorted(sums)) + "\n").encode("utf-8")
    output.mkdir(parents=True)
    for name, data in sorted(files.items()):
        (output / name).write_bytes(data)
    return {
        "output": str(output),
        "componentCount": len(components),
        "resourceCount": len(resources),
        "artifactCount": len(artifacts),
        "evidenceSha256": hashlib.sha256(files["release-evidence.json"]).hexdigest(),
    }


def parse_args(argv: Sequence[str]) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, required=True)
    parser.add_argument("--package-root", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--allow-missing-artifacts", action="store_true")
    return parser.parse_args(argv)


def main(argv: Sequence[str] | None = None) -> int:
    args = parse_args(sys.argv[1:] if argv is None else argv)
    try:
        summary = generate(args)
    except (ComplianceError, OSError, KeyError, TypeError, ValueError) as exc:
        print(f"release-compliance: ERROR: {exc}", file=sys.stderr)
        return 2
    print(json.dumps(summary, indent=2, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
