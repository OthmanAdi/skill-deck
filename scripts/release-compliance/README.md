# Skill Deck release compliance

This directory produces deterministic release evidence for one complete,
unlocked Skill Deck product. It does not install dependencies, publish an
artifact, sign a binary, access release secrets, or mutate a remote service.

`generate_release_evidence.py` verifies and records:

- the locked Windows x64 Cargo graph and production pnpm graph;
- a CycloneDX 1.6 SBOM and declared-license inventory;
- the exact pinned llama.cpp runtime set;
- Jan-nano revision, hash, license, and attribution fragments;
- source, notice, runtime, artifact, and checksum receipts;
- absence of developer-only executables from the package tree; and
- private-source Git provenance without absolute build-machine paths.

`check_binary_provenance.py` rejects embedded project, user-profile, Cargo,
Rustup, sysroot, and PDB paths. `check_windows_msi_payload.ps1` verifies that
the MSI contains exactly one expected application executable under the
expected Program Files directory, extracts it, and runs the provenance scan.
`run-tauri-remapped.mjs` prevents absolute build paths from entering Rust
artifacts in the first place.

The files under `release/legal-corpus`, `release/legal-corpus-policy.json`, and
`src-tauri/notice-corpus` retain the previously generated dependency notice
corpus for audit continuity. Former edition-specific schema and archive labels
were normalized to neutral legal-corpus identifiers; third-party legal bodies,
component inventory, source identities, and archive bytes remain unchanged.
Active product notices live under `src-tauri/resources` and are verified by the
single-product evidence generator.
`release/legal-corpus/RENAME-RECEIPT.json` records every old-to-new path and
SHA-256 digest and is independently checked against the original Git blobs.

Run focused tests:

```powershell
pnpm compliance:test
node --test scripts/release-compliance/tests/test_run_tauri_remapped.mjs
pwsh -NoProfile -File scripts/release-compliance/tests/test_check_windows_msi_payload.ps1
```

Generate source-bound evidence after Windows artifacts exist:

```powershell
$output = 'C:\path\to\fresh\skill-deck-release-evidence'
python -B scripts/release-compliance/generate_release_evidence.py `
  --root . `
  --package-root src-tauri `
  --output $output
```

For a source-only diagnostic pass, add `--allow-missing-artifacts`. This flag
must not be used for a release attestation.

Generated evidence deliberately retains a manual-license-review section. SBOM
identity and declared metadata do not replace review of upstream notice,
dual-license, reciprocal-source, or model-lineage obligations.
